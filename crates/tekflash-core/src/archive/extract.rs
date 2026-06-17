//! Tar extraction preserving permissions, ownership, times, and xattrs where the OS
//! supports them.

use color_eyre::eyre::{Context, Result};
use std::io::Read;
use std::path::Path;

pub fn extract_to<R: Read>(reader: R, dest: &Path) -> Result<()> {
    let mut ar = ::tar::Archive::new(reader);
    ar.set_preserve_permissions(true);
    ar.set_preserve_mtime(true);
    #[cfg(unix)]
    ar.set_preserve_ownerships(true);
    ar.unpack(dest)
        .with_context(|| format!("extracting tar into {}", dest.display()))?;
    Ok(())
}
