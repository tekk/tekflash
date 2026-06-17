//! Sidecar `.tfmanifest.json` describing a backup or archive.
//!
//! Written next to every backup/archive so a future restore (possibly on a different
//! machine) has everything it needs: codec, hash, encryption parameters (never the key
//! itself), source device metadata, byte counts. Also used by `--resume`.

use crate::pipeline::compress::{Codec, CompressionLevel};
use crate::pipeline::hasher::HashKind;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub tekflash_version: String,
    pub created: String, // RFC 3339
    pub host: Option<String>,
    pub source: SourceInfo,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub hash_kind: HashKind,
    pub hash_hex: String,
    pub codec: Codec,
    pub level: CompressionLevel,
    #[serde(default)]
    pub encryption: Option<EncryptionInfo>,
    #[serde(default)]
    pub sparse_extents: Vec<SparseExtent>,
    #[serde(default)]
    pub last_good_offset: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    pub path: PathBuf,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionInfo {
    /// "password-argon2id-chacha20poly1305" or "mlkem768-chacha20poly1305".
    pub scheme: String,
    /// Per-archive random salt, base64 (password mode only).
    #[serde(default)]
    pub salt_b64: Option<String>,
    /// Argon2id parameters used (password mode only).
    #[serde(default)]
    pub kdf: Option<KdfParams>,
    /// ML-KEM ciphertext that encapsulates the data key (recipient mode only).
    #[serde(default)]
    pub kem_ciphertext_b64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SparseExtent {
    pub offset: u64,
    pub length: u64,
}

impl Manifest {
    pub fn write_alongside(&self, image_path: &std::path::Path) -> std::io::Result<()> {
        let p = image_path.with_extension(format!(
            "{}.tfmanifest.json",
            image_path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
        ));
        let f = std::fs::File::create(p)?;
        serde_json::to_writer_pretty(f, self)?;
        Ok(())
    }
}
