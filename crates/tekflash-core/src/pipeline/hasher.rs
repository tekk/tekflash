//! Streaming hashers. BLAKE3 is the default — it's faster than memcpy on modern CPUs and
//! provides 256-bit cryptographic strength. SHA-256 is kept as an option for users who
//! want to interop with existing image-distribution `.sha256` sidecars.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashKind {
    Blake3,
    Sha256,
}

pub enum Hasher {
    // BLAKE3's state is ~1.9 KiB — box it so the enum stays small (`Sha256` is ~112 B).
    Blake3(Box<blake3::Hasher>),
    Sha256(Box<sha2::Sha256>),
}

impl Hasher {
    pub fn new(kind: HashKind) -> Self {
        match kind {
            HashKind::Blake3 => Hasher::Blake3(Box::new(blake3::Hasher::new())),
            HashKind::Sha256 => Hasher::Sha256(Box::new(<sha2::Sha256 as sha2::Digest>::new())),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::Blake3(h) => {
                h.update(data);
            }
            Hasher::Sha256(h) => {
                use sha2::Digest;
                h.update(data);
            }
        }
    }

    /// Finish and return the digest as a lowercase hex string.
    pub fn finalize_hex(self) -> String {
        match self {
            Hasher::Blake3(h) => h.finalize().to_hex().to_string(),
            Hasher::Sha256(h) => {
                use sha2::Digest;
                let bytes = (*h).finalize();
                let mut s = String::with_capacity(bytes.len() * 2);
                for b in bytes {
                    use std::fmt::Write;
                    let _ = write!(&mut s, "{b:02x}");
                }
                s
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blake3_matches_known_vector() {
        let mut h = Hasher::new(HashKind::Blake3);
        h.update(b"abc");
        // BLAKE3("abc") KAT vector.
        assert_eq!(
            h.finalize_hex(),
            "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
    }

    #[test]
    fn sha256_matches_known_vector() {
        let mut h = Hasher::new(HashKind::Sha256);
        h.update(b"abc");
        assert_eq!(
            h.finalize_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
