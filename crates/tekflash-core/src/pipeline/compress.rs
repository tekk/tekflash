//! Compression codecs behind a uniform `Codec` enum.
//!
//! Each codec exposes the same shape: a `read_decode(reader) -> reader` for the flash
//! side and a `write_encode(writer) -> writer` for the backup side. We use the std
//! `Read`/`Write` traits and bridge to tokio at the pipeline boundary, which keeps the
//! codec wrappers simple and synchronous.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    None,
    Zstd,
    Lz4,
    Brotli,
    Xz,
    Gzip,
    Bzip2,
}

impl Codec {
    /// Default extension (without leading dot) for this codec when used as the outer
    /// codec of a backup image.
    pub fn extension(self) -> &'static str {
        match self {
            Codec::None => "img",
            Codec::Zstd => "zst",
            Codec::Lz4 => "lz4",
            Codec::Brotli => "br",
            Codec::Xz => "xz",
            Codec::Gzip => "gz",
            Codec::Bzip2 => "bz2",
        }
    }

    pub fn human(self) -> &'static str {
        match self {
            Codec::None => "none",
            Codec::Zstd => "zstd",
            Codec::Lz4 => "lz4",
            Codec::Brotli => "brotli",
            Codec::Xz => "xz",
            Codec::Gzip => "gzip",
            Codec::Bzip2 => "bzip2",
        }
    }
}

/// Compression level. Each codec maps this onto its own range; the wrapper picks a
/// sensible default if the level is out of range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompressionLevel(pub i32);

impl Default for CompressionLevel {
    fn default() -> Self {
        Self(3)
    }
}

/// Wrap a writer with a streaming encoder for `codec` at `level`. Returns a `Box<dyn
/// Write>` so call sites don't need to know about each codec's concrete type.
pub fn encoder<'a, W: std::io::Write + Send + 'a>(
    codec: Codec,
    level: CompressionLevel,
    writer: W,
) -> std::io::Result<Box<dyn std::io::Write + Send + 'a>> {
    Ok(match codec {
        Codec::None => Box::new(writer),
        Codec::Zstd => {
            let enc = zstd::stream::Encoder::new(writer, level.0)?;
            // TODO: wire multi-thread via the `zstdmt` feature once we add it. Single-
            // threaded zstd is still very fast at level 3; multi-thread is an
            // optimization for high-level backups.
            Box::new(enc.auto_finish())
        }
        Codec::Lz4 => Box::new(Lz4FinishOnDrop(Some(lz4_flex::frame::FrameEncoder::new(
            writer,
        )))),
        Codec::Brotli => {
            // brotli quality 0-11. Map level 0-22 → 0-11.
            let q = (level.0.clamp(0, 22) * 11 / 22) as u32;
            Box::new(brotli::CompressorWriter::new(writer, 4096, q, 22))
        }
        Codec::Xz => Box::new(xz2::write::XzEncoder::new(
            writer,
            level.0.clamp(0, 9) as u32,
        )),
        Codec::Gzip => Box::new(flate2::write::GzEncoder::new(
            writer,
            flate2::Compression::new(level.0.clamp(0, 9) as u32),
        )),
        Codec::Bzip2 => Box::new(bzip2::write::BzEncoder::new(
            writer,
            bzip2::Compression::new(level.0.clamp(0, 9) as u32),
        )),
    })
}

/// Wrap a reader with a streaming decoder for `codec`.
pub fn decoder<'a, R: std::io::Read + Send + 'a>(
    codec: Codec,
    reader: R,
) -> std::io::Result<Box<dyn std::io::Read + Send + 'a>> {
    Ok(match codec {
        Codec::None => Box::new(reader),
        Codec::Zstd => Box::new(zstd::stream::Decoder::new(reader)?),
        Codec::Lz4 => Box::new(lz4_flex::frame::FrameDecoder::new(reader)),
        Codec::Brotli => Box::new(brotli::Decompressor::new(reader, 4096)),
        Codec::Xz => Box::new(xz2::read::XzDecoder::new(reader)),
        Codec::Gzip => Box::new(flate2::read::GzDecoder::new(reader)),
        Codec::Bzip2 => Box::new(bzip2::read::BzDecoder::new(reader)),
    })
}

/// lz4_flex's `FrameEncoder` does not write the LZ4 frame trailer on `Drop`. Wrap it so
/// the trailer is flushed when the encoder goes out of scope, matching the other codecs.
struct Lz4FinishOnDrop<W: std::io::Write>(Option<lz4_flex::frame::FrameEncoder<W>>);

impl<W: std::io::Write> std::io::Write for Lz4FinishOnDrop<W> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.as_mut().expect("encoder taken").write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.as_mut().expect("encoder taken").flush()
    }
}

impl<W: std::io::Write> Drop for Lz4FinishOnDrop<W> {
    fn drop(&mut self) {
        if let Some(enc) = self.0.take() {
            let _ = enc.finish();
        }
    }
}

impl From<super::format::InputFormat> for Codec {
    fn from(f: super::format::InputFormat) -> Self {
        use super::format::InputFormat as F;
        match f {
            F::Raw | F::Tar | F::Encrypted => Codec::None,
            F::Zstd => Codec::Zstd,
            F::Xz => Codec::Xz,
            F::Gzip => Codec::Gzip,
            F::Bzip2 => Codec::Bzip2,
            F::Lz4 => Codec::Lz4,
            F::Brotli => Codec::Brotli,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn roundtrip(codec: Codec) {
        let payload: Vec<u8> = (0u32..200_000).map(|i| (i % 251) as u8).collect();

        let mut compressed = Vec::new();
        {
            let mut enc = encoder(codec, CompressionLevel(3), &mut compressed).unwrap();
            enc.write_all(&payload).unwrap();
        }

        let mut decompressed = Vec::new();
        let mut dec = decoder(codec, std::io::Cursor::new(&compressed)).unwrap();
        dec.read_to_end(&mut decompressed).unwrap();

        assert_eq!(decompressed, payload, "roundtrip failed for {codec:?}");
    }

    #[test]
    fn roundtrip_all_codecs() {
        for c in [
            Codec::Zstd,
            Codec::Lz4,
            Codec::Brotli,
            Codec::Xz,
            Codec::Gzip,
            Codec::Bzip2,
        ] {
            roundtrip(c);
        }
    }

    #[test]
    fn roundtrip_none_is_identity() {
        roundtrip(Codec::None);
    }
}
