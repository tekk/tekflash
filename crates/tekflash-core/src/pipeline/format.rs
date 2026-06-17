//! Input format detection by magic bytes.
//!
//! We deliberately don't trust file extensions — a user might rename `ubuntu.iso` to
//! `ubuntu.img.bin` or vice versa, and the user requirement lists half a dozen
//! interchangeable zstd suffixes. Magic-byte detection looks at the first 6 bytes of the
//! stream (and one ISO-specific probe at offset 0x8001).

/// What the input looks like, decided from a peek at its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    /// Raw block image — `.img`, `.bin`, `.iso`, `.raw`, etc. Written through verbatim.
    Raw,
    /// zstd-compressed (`.zst` / `.zsd` / `.zstd`).
    Zstd,
    /// XZ / LZMA (`.xz`).
    Xz,
    /// gzip (`.gz`).
    Gzip,
    /// bzip2 (`.bz2`).
    Bzip2,
    /// LZ4 framed (`.lz4`).
    Lz4,
    /// Brotli (`.br`).
    Brotli,
    /// POSIX tar (uncompressed). Compressed tars are detected as their outer codec; we
    /// only see naked tar here when the user passes `.tar`.
    Tar,
    /// tekflash encrypted envelope (custom container, see `pipeline::crypto`).
    Encrypted,
}

impl InputFormat {
    pub fn is_compressed(self) -> bool {
        matches!(
            self,
            InputFormat::Zstd
                | InputFormat::Xz
                | InputFormat::Gzip
                | InputFormat::Bzip2
                | InputFormat::Lz4
                | InputFormat::Brotli
        )
    }

    pub fn human(self) -> &'static str {
        match self {
            InputFormat::Raw => "raw image",
            InputFormat::Zstd => "zstd",
            InputFormat::Xz => "xz",
            InputFormat::Gzip => "gzip",
            InputFormat::Bzip2 => "bzip2",
            InputFormat::Lz4 => "lz4",
            InputFormat::Brotli => "brotli",
            InputFormat::Tar => "tar",
            InputFormat::Encrypted => "tekflash encrypted",
        }
    }
}

/// Inspect the first few bytes (and the ISO 9660 probe offset) and decide what we're
/// dealing with. `iso_probe` may be `None` if the caller hasn't seeked to 0x8001 — that
/// only matters for distinguishing a `.iso` from a generic `.img`, both of which decode
/// to `Raw` here. ISO detection is a UI cue, not a pipeline choice.
pub fn detect(head: &[u8]) -> InputFormat {
    // tekflash envelope: "TFE1" magic at byte 0.
    if head.len() >= 4 && &head[0..4] == b"TFE1" {
        return InputFormat::Encrypted;
    }
    // zstd: 0x28 B5 2F FD
    if head.len() >= 4 && head[0] == 0x28 && head[1] == 0xB5 && head[2] == 0x2F && head[3] == 0xFD {
        return InputFormat::Zstd;
    }
    // xz: FD 37 7A 58 5A 00
    if head.len() >= 6
        && head[0] == 0xFD
        && head[1] == 0x37
        && head[2] == 0x7A
        && head[3] == 0x58
        && head[4] == 0x5A
        && head[5] == 0x00
    {
        return InputFormat::Xz;
    }
    // gzip: 1F 8B
    if head.len() >= 2 && head[0] == 0x1F && head[1] == 0x8B {
        return InputFormat::Gzip;
    }
    // bzip2: 'B' 'Z' 'h'
    if head.len() >= 3 && &head[0..3] == b"BZh" {
        return InputFormat::Bzip2;
    }
    // LZ4 framed: 04 22 4D 18
    if head.len() >= 4 && head[0] == 0x04 && head[1] == 0x22 && head[2] == 0x4D && head[3] == 0x18 {
        return InputFormat::Lz4;
    }
    // Brotli has no fixed magic. We rely on the file extension upstream and treat any
    // unrecognized stream as Raw.
    // tar: at offset 257 the bytes "ustar" appear, but at offset 0 there's no magic.
    // For a .tar passed in via CLI we trust the extension; pure magic detection of tar
    // requires a 512-byte header which we don't peek here.
    InputFormat::Raw
}

/// Convenience: detect from a file path's extension as a *hint* only. Used by the file
/// browser preview and to pick a default output extension. Magic-byte detect is the
/// authoritative answer for actual processing.
pub fn detect_by_extension(path: &std::path::Path) -> Option<InputFormat> {
    let s = path.to_string_lossy().to_ascii_lowercase();
    if s.ends_with(".zst") || s.ends_with(".zstd") || s.ends_with(".zsd") {
        Some(InputFormat::Zstd)
    } else if s.ends_with(".xz") {
        Some(InputFormat::Xz)
    } else if s.ends_with(".gz") {
        Some(InputFormat::Gzip)
    } else if s.ends_with(".bz2") {
        Some(InputFormat::Bzip2)
    } else if s.ends_with(".lz4") {
        Some(InputFormat::Lz4)
    } else if s.ends_with(".br") {
        Some(InputFormat::Brotli)
    } else if s.ends_with(".tar") {
        Some(InputFormat::Tar)
    } else if s.ends_with(".iso")
        || s.ends_with(".img")
        || s.ends_with(".bin")
        || s.ends_with(".raw")
    {
        Some(InputFormat::Raw)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_zstd() {
        assert_eq!(detect(&[0x28, 0xB5, 0x2F, 0xFD, 0, 0]), InputFormat::Zstd);
    }

    #[test]
    fn detects_xz() {
        assert_eq!(
            detect(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]),
            InputFormat::Xz
        );
    }

    #[test]
    fn detects_gzip() {
        assert_eq!(detect(&[0x1F, 0x8B, 0x08, 0x00]), InputFormat::Gzip);
    }

    #[test]
    fn detects_bzip2() {
        assert_eq!(detect(b"BZh91AY"), InputFormat::Bzip2);
    }

    #[test]
    fn detects_lz4_framed() {
        assert_eq!(detect(&[0x04, 0x22, 0x4D, 0x18, 0, 0]), InputFormat::Lz4);
    }

    #[test]
    fn defaults_to_raw() {
        assert_eq!(detect(&[0u8; 6]), InputFormat::Raw);
        assert_eq!(detect(&[0xDE, 0xAD, 0xBE, 0xEF, 1, 2]), InputFormat::Raw);
    }

    #[test]
    fn ext_aliases_for_zstd() {
        use std::path::Path;
        for n in &["a.zst", "a.zstd", "a.zsd", "BIG.IMG.ZSD"] {
            assert_eq!(detect_by_extension(Path::new(n)), Some(InputFormat::Zstd));
        }
    }
}
