//! Sidecar manifest serde roundtrip.

use tekflash_core::manifest::{EncryptionInfo, KdfParams, Manifest, SourceInfo};
use tekflash_core::pipeline::compress::{Codec, CompressionLevel};
use tekflash_core::pipeline::hasher::HashKind;

#[test]
fn manifest_roundtrips_through_json() {
    let m = Manifest {
        schema_version: 1,
        tekflash_version: env!("CARGO_PKG_VERSION").to_string(),
        created: "2026-06-17T17:42:00Z".to_string(),
        host: Some("test-host".to_string()),
        source: SourceInfo {
            path: "/dev/disk5".into(),
            vendor: Some("SanDisk".to_string()),
            model: Some("Ultra USB 3.0".to_string()),
            serial: Some("ABCDEF".to_string()),
            size_bytes: 31_914_983_424,
        },
        bytes_in: 31_914_983_424,
        bytes_out: 1_023_456_789,
        hash_kind: HashKind::Blake3,
        hash_hex: "deadbeefcafebabe".repeat(4),
        codec: Codec::Zstd,
        level: CompressionLevel(19),
        encryption: Some(EncryptionInfo {
            scheme: "password-argon2id-chacha20poly1305".to_string(),
            salt_b64: Some("c2FsdHk=".to_string()),
            kdf: Some(KdfParams {
                m_kib: 262_144,
                t: 3,
                p: 4,
            }),
            kem_ciphertext_b64: None,
        }),
        sparse_extents: vec![],
        last_good_offset: None,
    };

    let s = serde_json::to_string_pretty(&m).expect("serialize");
    let back: Manifest = serde_json::from_str(&s).expect("deserialize");
    assert_eq!(back.tekflash_version, m.tekflash_version);
    assert_eq!(back.source.size_bytes, m.source.size_bytes);
    assert_eq!(back.codec, Codec::Zstd);
    assert_eq!(back.level, CompressionLevel(19));
    assert_eq!(back.hash_kind, HashKind::Blake3);
    assert!(back.encryption.is_some());
    assert_eq!(
        back.encryption.unwrap().scheme,
        "password-argon2id-chacha20poly1305"
    );
}
