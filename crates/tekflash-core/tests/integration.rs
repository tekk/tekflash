//! End-to-end tests that exercise the public API the binary crate uses.

use std::io::{Read, Write};
use tekflash_core::pipeline::{
    compress::{decoder, encoder, Codec, CompressionLevel},
    format::{detect, InputFormat},
    hasher::{HashKind, Hasher},
};

/// Compress, decompress, and confirm the round-trip survives — for every codec, against
/// non-trivial data that won't fit in a single internal buffer.
#[test]
fn end_to_end_compress_decompress_for_every_codec() {
    let payload: Vec<u8> = (0u64..400_000)
        .map(|i| (i.wrapping_mul(31)) as u8)
        .collect();

    for codec in [
        Codec::None,
        Codec::Zstd,
        Codec::Lz4,
        Codec::Brotli,
        Codec::Xz,
        Codec::Gzip,
        Codec::Bzip2,
    ] {
        let mut compressed = Vec::new();
        {
            let mut enc = encoder(codec, CompressionLevel(3), &mut compressed).unwrap();
            enc.write_all(&payload).unwrap();
        }
        let mut got = Vec::new();
        let mut dec = decoder(codec, std::io::Cursor::new(&compressed)).unwrap();
        dec.read_to_end(&mut got).unwrap();
        assert_eq!(got, payload, "round-trip failed for {codec:?}");
    }
}

/// `detect()` consistently agrees with the encoder's actual output magic bytes.
#[test]
fn detect_matches_encoder_output_for_compressed_codecs() {
    for (codec, expected) in [
        (Codec::Zstd, InputFormat::Zstd),
        (Codec::Lz4, InputFormat::Lz4),
        (Codec::Xz, InputFormat::Xz),
        (Codec::Gzip, InputFormat::Gzip),
        (Codec::Bzip2, InputFormat::Bzip2),
    ] {
        let mut buf = Vec::new();
        {
            let mut enc = encoder(codec, CompressionLevel(3), &mut buf).unwrap();
            enc.write_all(b"hello tekflash").unwrap();
        }
        let head = &buf[..6.min(buf.len())];
        assert_eq!(detect(head), expected, "magic-byte mismatch for {codec:?}");
    }
}

/// BLAKE3 streaming hash matches the well-known KAT for an empty input.
#[test]
fn blake3_empty_input_matches_kat() {
    let h = Hasher::new(HashKind::Blake3);
    assert_eq!(
        h.finalize_hex(),
        "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
    );
}

/// SHA-256 streaming hash matches the well-known KAT for an empty input.
#[test]
fn sha256_empty_input_matches_kat() {
    let h = Hasher::new(HashKind::Sha256);
    assert_eq!(
        h.finalize_hex(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

/// Streaming hash for a multi-chunk update equals the all-at-once hash.
#[test]
fn streaming_hash_matches_one_shot() {
    let payload: Vec<u8> = (0..100_000u32).map(|i| (i & 0xff) as u8).collect();
    let one_shot = blake3::hash(&payload).to_hex().to_string();

    let mut h = Hasher::new(HashKind::Blake3);
    for chunk in payload.chunks(7919) {
        h.update(chunk);
    }
    assert_eq!(h.finalize_hex(), one_shot);
}

/// Magic-byte detection prefers the encrypted-envelope magic over anything else.
#[test]
fn encrypted_magic_wins_over_other_signatures() {
    let mut buf = b"TFE1".to_vec();
    buf.extend_from_slice(&[0x28, 0xB5, 0x2F, 0xFD]); // would otherwise look like zstd
    assert_eq!(detect(&buf), InputFormat::Encrypted);
}
