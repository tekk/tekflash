//! Archive include/exclude selection used by the TUI source picker and the
//! CLI `--exclude` glob set.
//!
//! State of any path `p` relative to the source root is derived (not stored
//! per-entry):
//!
//! - `Fully`: a path-or-ancestor is in `included` and no ancestor between `p`
//!   and that included ancestor is in `excluded`.
//! - `Empty`: not included.
//!
//! When `included` is empty, the selection is treated as "include everything"
//! (the historical behaviour of the CLI archive command). Glob exclusions
//! still apply.

use globset::{Glob, GlobSet, GlobSetBuilder};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct ArchiveSelection {
    /// Absolute paths the user explicitly checked.
    pub included: HashSet<PathBuf>,
    /// Absolute paths the user explicitly un-checked under an included ancestor.
    pub excluded: HashSet<PathBuf>,
    /// Compiled glob excludes matched against the *relative* path from the
    /// source root. None when no globs were provided.
    pub excludes_glob: Option<GlobSet>,
}

impl ArchiveSelection {
    /// True when `included` is empty (and therefore the selection means "all").
    pub fn includes_everything(&self) -> bool {
        self.included.is_empty()
    }

    /// Decide whether an absolute path should be archived. Honours `included`,
    /// `excluded`, and any compiled glob set.
    pub fn is_included(&self, abs_path: &Path, source_root: &Path) -> bool {
        // 1. Glob check against the path relative to the source root.
        if let Some(globs) = self.excludes_glob.as_ref() {
            let rel = abs_path.strip_prefix(source_root).unwrap_or(abs_path);
            if globs.is_match(rel) {
                return false;
            }
        }

        // 2. Explicit exclude wins over implicit include via an ancestor.
        if self.is_explicitly_excluded(abs_path) {
            return false;
        }

        // 3. If included is empty, the selection is "everything".
        if self.included.is_empty() {
            return true;
        }

        // 4. Otherwise, the path is included iff some ancestor (including the
        //    path itself) is in `included`, AND no closer ancestor up to that
        //    included root is in `excluded`.
        let mut cursor: Option<&Path> = Some(abs_path);
        while let Some(p) = cursor {
            if self.included.contains(p) {
                return true; // already passed the explicit-excluded test above
            }
            if self.excluded.contains(p) {
                return false;
            }
            cursor = p.parent();
        }
        false
    }

    fn is_explicitly_excluded(&self, abs_path: &Path) -> bool {
        let mut cursor: Option<&Path> = Some(abs_path);
        while let Some(p) = cursor {
            if self.excluded.contains(p) {
                return true;
            }
            if self.included.contains(p) {
                // Reached an included ancestor without hitting an excluded one.
                return false;
            }
            cursor = p.parent();
        }
        false
    }

    /// Build a glob set from user patterns. Returns the first compile error
    /// (with pattern context) if any pattern is invalid.
    pub fn compile_globs(patterns: &[String]) -> Result<Option<GlobSet>, globset::Error> {
        if patterns.is_empty() {
            return Ok(None);
        }
        let mut builder = GlobSetBuilder::new();
        for p in patterns {
            builder.add(Glob::new(p)?);
        }
        Ok(Some(builder.build()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_includes_everything() {
        let sel = ArchiveSelection::default();
        assert!(sel.includes_everything());
    }

    #[cfg(unix)]
    #[test]
    fn default_selection_includes_any_path() {
        let sel = ArchiveSelection::default();
        let root = PathBuf::from("/tmp/root");
        assert!(sel.is_included(&PathBuf::from("/tmp/root/a/b.txt"), &root));
        assert!(sel.is_included(&PathBuf::from("/tmp/root/anything"), &root));
    }

    #[cfg(unix)]
    #[test]
    fn included_ancestor_pulls_in_descendants() {
        let root = PathBuf::from("/tmp/root");
        let mut included = HashSet::new();
        included.insert(PathBuf::from("/tmp/root/a"));
        let sel = ArchiveSelection {
            included,
            excluded: HashSet::new(),
            excludes_glob: None,
        };
        assert!(sel.is_included(&PathBuf::from("/tmp/root/a"), &root));
        assert!(sel.is_included(&PathBuf::from("/tmp/root/a/b.txt"), &root));
        assert!(!sel.is_included(&PathBuf::from("/tmp/root/c.txt"), &root));
    }

    #[cfg(unix)]
    #[test]
    fn excluded_subtree_under_included_ancestor() {
        let root = PathBuf::from("/tmp/root");
        let mut included = HashSet::new();
        included.insert(PathBuf::from("/tmp/root/a"));
        let mut excluded = HashSet::new();
        excluded.insert(PathBuf::from("/tmp/root/a/b"));
        let sel = ArchiveSelection {
            included,
            excluded,
            excludes_glob: None,
        };
        assert!(!sel.is_included(&PathBuf::from("/tmp/root/a/b/x.txt"), &root));
        assert!(!sel.is_included(&PathBuf::from("/tmp/root/a/b"), &root));
        assert!(sel.is_included(&PathBuf::from("/tmp/root/a/c.txt"), &root));
    }

    #[test]
    fn compile_globs_none_when_empty() {
        let out = ArchiveSelection::compile_globs(&[]).expect("ok");
        assert!(out.is_none());
    }

    #[test]
    fn compile_globs_matches_patterns() {
        let patterns = vec!["*.tmp".to_string(), "node_modules/**".to_string()];
        let globs = ArchiveSelection::compile_globs(&patterns)
            .expect("compile ok")
            .expect("some");
        assert!(globs.is_match("foo.tmp"));
        assert!(globs.is_match("node_modules/x"));
        assert!(!globs.is_match("keep.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn glob_exclude_blocks_explicitly_included_file() {
        let root = PathBuf::from("/tmp/root");
        let mut included = HashSet::new();
        included.insert(PathBuf::from("/tmp/root/a/junk.tmp"));
        let globs = ArchiveSelection::compile_globs(&["*.tmp".to_string()])
            .expect("ok")
            .expect("some");
        let sel = ArchiveSelection {
            included,
            excluded: HashSet::new(),
            excludes_glob: Some(globs),
        };
        // Even though the file is explicitly in `included`, the glob filter
        // checked first should reject it.
        assert!(!sel.is_included(&PathBuf::from("/tmp/root/a/junk.tmp"), &root));
    }
}
