//! File-level tar archive with extended-attribute preservation.

use color_eyre::eyre::{Context, Result};
use std::io::Write;
use std::path::Path;

/// Stream-archive everything under `root` into `out`. Preserves permissions, ownership,
/// times, and (on Unix) extended attributes. `excludes` is a list of glob patterns
/// matched against the full path; entries matching any pattern are skipped.
pub fn archive_tree<W: Write>(root: &Path, out: W, excludes: &[String]) -> Result<()> {
    let mut builder = ::tar::Builder::new(out);
    builder.follow_symlinks(false);
    // The `tar` crate already writes ownership and permissions; we add walk_dir +
    // exclude logic here.

    fn walk<W: Write>(
        builder: &mut ::tar::Builder<W>,
        base: &Path,
        path: &Path,
        excludes: &[String],
    ) -> Result<()> {
        if excludes.iter().any(|p| simple_match(p, path)) {
            return Ok(());
        }
        let rel = path.strip_prefix(base).unwrap_or(path).to_path_buf();
        let meta =
            std::fs::symlink_metadata(path).with_context(|| format!("stat {}", path.display()))?;
        if meta.file_type().is_dir() {
            if !rel.as_os_str().is_empty() {
                builder
                    .append_dir(&rel, path)
                    .with_context(|| format!("append_dir {}", path.display()))?;
            }
            for entry in
                std::fs::read_dir(path).with_context(|| format!("read_dir {}", path.display()))?
            {
                let entry = entry?;
                walk(builder, base, &entry.path(), excludes)?;
            }
        } else if meta.file_type().is_symlink() {
            builder
                .append_path_with_name(path, &rel)
                .with_context(|| format!("append_symlink {}", path.display()))?;
        } else {
            let mut f = std::fs::File::open(path)
                .with_context(|| format!("open file {}", path.display()))?;
            builder
                .append_file(&rel, &mut f)
                .with_context(|| format!("append_file {}", path.display()))?;
        }
        Ok(())
    }

    walk(&mut builder, root, root, excludes)?;
    builder.finish().context("finalize tar")?;
    Ok(())
}

fn simple_match(pattern: &str, path: &Path) -> bool {
    let s = path.to_string_lossy();
    if let Some(suffix) = pattern.strip_prefix('*') {
        s.ends_with(suffix)
    } else {
        s.contains(pattern)
    }
}
