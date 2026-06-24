//! File-level archive (tar) — skeleton.
//!
//! Real implementations of `tar` and `extract` come in the follow-up task; the module is
//! declared here so the public surface is stable.

/// Container format for file-level archives.
///
/// `Tar` is the default and feeds through the existing pipeline + outer codec
/// (zstd / xz / gzip / etc., chosen via `BackupParams::codec`). `Zip` and
/// `SevenZ` bundle compression themselves; the outer `codec` field is ignored
/// when those are selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArchiveFormat {
    #[default]
    Tar,
    Zip,
    SevenZ,
}

impl ArchiveFormat {
    /// Human-readable name for headers / help / errors.
    pub fn human(self) -> &'static str {
        match self {
            ArchiveFormat::Tar => "tar",
            ArchiveFormat::Zip => "zip",
            ArchiveFormat::SevenZ => "7z",
        }
    }

    /// Build the right extension for a saved archive given the codec extension
    /// the user picked. For Tar we sandwich the codec ext after `.tar`
    /// (e.g. `.tar.zst`). For Zip and 7z the codec is baked into the container
    /// so the outer codec ext is ignored.
    pub fn extension(self, outer_codec_ext: &str) -> String {
        match self {
            ArchiveFormat::Tar => format!(".tar{outer_codec_ext}"),
            ArchiveFormat::Zip => ".zip".to_string(),
            ArchiveFormat::SevenZ => ".7z".to_string(),
        }
    }
}

pub mod extract;
pub mod selection;
pub mod tar;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_tar() {
        assert_eq!(ArchiveFormat::default(), ArchiveFormat::Tar);
    }

    #[test]
    fn tar_extension_with_codec_ext() {
        assert_eq!(ArchiveFormat::Tar.extension(".zst"), ".tar.zst");
        assert_eq!(ArchiveFormat::Tar.extension(".gz"), ".tar.gz");
        assert_eq!(ArchiveFormat::Tar.extension(""), ".tar");
    }

    #[test]
    fn zip_and_sevenz_ignore_outer_codec() {
        assert_eq!(ArchiveFormat::Zip.extension(".zst"), ".zip");
        assert_eq!(ArchiveFormat::Zip.extension(""), ".zip");
        assert_eq!(ArchiveFormat::SevenZ.extension(".zst"), ".7z");
        assert_eq!(ArchiveFormat::SevenZ.extension(""), ".7z");
    }

    #[test]
    fn human_names() {
        assert_eq!(ArchiveFormat::Tar.human(), "tar");
        assert_eq!(ArchiveFormat::Zip.human(), "zip");
        assert_eq!(ArchiveFormat::SevenZ.human(), "7z");
    }
}
