//! Password-mode AEAD encryption.
//!
//! Argon2id derives a 256-bit data key from a user password and a 16-byte random salt.
//! That key drives ChaCha20-Poly1305 framing: payload is split into 16 KiB plaintext
//! frames, each sealed with a 12-byte counter nonce. Each frame's 16-byte AEAD tag is
//! appended; truncation, reordering, and bit-flipping all fail authentication.
//!
//! Container layout (envelope written ahead of the AEAD frames):
//!
//! ```text
//!   off  len  field
//!   0    4    magic "TFE1"
//!   4    1    version (1)
//!   5    1    scheme (1=password-argon2id-chacha20p1305)
//!   6    2    reserved
//!   8    4    Argon2id m (KiB)
//!   12   4    Argon2id t
//!   16   4    Argon2id p
//!   20   4    frame_size_log2 (e.g. 14 = 16 KiB)
//!   24   16   salt
//!   40   ..   AEAD frames: [frame_payload][16-byte tag] × N, final frame has high bit set
//! ```
//!
//! ML-KEM (post-quantum recipient mode) wraps the same data key with `ml-kem` and
//! prepends the KEM ciphertext after the envelope header. Recipient mode lands in a
//! follow-up; this commit ships the password-mode primitives plus a round-trip test.

use color_eyre::eyre::{eyre, Result};

const MAGIC: &[u8; 4] = b"TFE1";
const VERSION: u8 = 1;
const SCHEME_PASSWORD: u8 = 1;
const FRAME_SIZE_LOG2: u32 = 14; // 16 KiB
const TAG_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const SALT_LEN: usize = 16;
const KEY_LEN: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct PasswordParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl Default for PasswordParams {
    fn default() -> Self {
        // 256 MiB memory, t=3, p=4: comfortably PQ-safe and fast enough on modern HW.
        Self {
            m_kib: 256 * 1024,
            t: 3,
            p: 4,
        }
    }
}

/// Encrypt `plaintext` with a password. Returns the full envelope (header + frames).
pub fn encrypt_password(
    password: &[u8],
    params: PasswordParams,
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::ChaCha20Poly1305;
    use rand::RngCore;

    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    let key = derive_key(password, &salt, params)?;
    let cipher = ChaCha20Poly1305::new(&key.into());

    let mut out = Vec::with_capacity(40 + plaintext.len() + TAG_LEN * 64);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(SCHEME_PASSWORD);
    out.extend_from_slice(&[0u8; 2]); // reserved
    out.extend_from_slice(&params.m_kib.to_le_bytes());
    out.extend_from_slice(&params.t.to_le_bytes());
    out.extend_from_slice(&params.p.to_le_bytes());
    out.extend_from_slice(&FRAME_SIZE_LOG2.to_le_bytes());
    out.extend_from_slice(&salt);

    let frame_size = 1usize << FRAME_SIZE_LOG2;
    let mut counter: u64 = 0;
    let total = plaintext.len();
    let mut offset = 0;
    while offset < total {
        let end = (offset + frame_size).min(total);
        let is_last = end == total;
        let nonce_bytes = make_nonce(counter, is_last);
        let nonce = nonce_bytes.into();
        // Bind the AEAD to (counter, last-flag) so reordering or truncation fails
        // authentication.
        let mut aad = [0u8; 9];
        aad[..8].copy_from_slice(&counter.to_le_bytes());
        aad[8] = is_last as u8;
        let ct = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: &plaintext[offset..end],
                    aad: &aad,
                },
            )
            .map_err(|e| eyre!("AEAD encrypt failed: {e}"))?;
        out.extend_from_slice(&ct);
        counter += 1;
        offset = end;
    }
    Ok(out)
}

/// Decrypt an envelope produced by `encrypt_password`.
pub fn decrypt_password(password: &[u8], envelope: &[u8]) -> Result<Vec<u8>> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::ChaCha20Poly1305;

    if envelope.len() < 40 {
        return Err(eyre!("envelope too short"));
    }
    if &envelope[..4] != MAGIC {
        return Err(eyre!("bad magic; not a tekflash envelope"));
    }
    if envelope[4] != VERSION {
        return Err(eyre!("unsupported envelope version {}", envelope[4]));
    }
    if envelope[5] != SCHEME_PASSWORD {
        return Err(eyre!("unsupported scheme {}", envelope[5]));
    }
    let m_kib = u32::from_le_bytes(envelope[8..12].try_into().unwrap());
    let t = u32::from_le_bytes(envelope[12..16].try_into().unwrap());
    let p = u32::from_le_bytes(envelope[16..20].try_into().unwrap());
    let frame_log = u32::from_le_bytes(envelope[20..24].try_into().unwrap());
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&envelope[24..40]);

    let key = derive_key(password, &salt, PasswordParams { m_kib, t, p })?;
    let cipher = ChaCha20Poly1305::new(&key.into());

    let frame_size = 1usize << frame_log;
    let mut out = Vec::with_capacity(envelope.len());
    let mut pos = 40;
    let mut counter: u64 = 0;
    while pos < envelope.len() {
        let remaining = envelope.len() - pos;
        let take = if remaining > frame_size + TAG_LEN {
            frame_size + TAG_LEN
        } else {
            remaining
        };
        let is_last = pos + take == envelope.len();
        let nonce_bytes = make_nonce(counter, is_last);
        let nonce = nonce_bytes.into();
        let mut aad = [0u8; 9];
        aad[..8].copy_from_slice(&counter.to_le_bytes());
        aad[8] = is_last as u8;
        let pt = cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &envelope[pos..pos + take],
                    aad: &aad,
                },
            )
            .map_err(|e| eyre!("AEAD decrypt failed at frame {counter}: {e}"))?;
        out.extend_from_slice(&pt);
        pos += take;
        counter += 1;
    }
    Ok(out)
}

fn derive_key(password: &[u8], salt: &[u8], params: PasswordParams) -> Result<[u8; KEY_LEN]> {
    use argon2::{Algorithm, Argon2, Params, Version};
    let p = Params::new(params.m_kib, params.t, params.p, Some(KEY_LEN))
        .map_err(|e| eyre!("Argon2 params: {e}"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, p);
    let mut out = [0u8; KEY_LEN];
    argon
        .hash_password_into(password, salt, &mut out)
        .map_err(|e| eyre!("Argon2id derivation: {e}"))?;
    Ok(out)
}

fn make_nonce(counter: u64, is_last: bool) -> [u8; NONCE_LEN] {
    let mut n = [0u8; NONCE_LEN];
    n[..8].copy_from_slice(&counter.to_le_bytes());
    n[8] = is_last as u8;
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_params() -> PasswordParams {
        // Argon2id m=8 MiB, t=1, p=1 — fast enough for tests, still cryptographically
        // valid. Production uses the default (256 MiB, t=3).
        PasswordParams {
            m_kib: 8 * 1024,
            t: 1,
            p: 1,
        }
    }

    #[test]
    fn password_roundtrips_small_payload() {
        let payload = b"hello tekflash, this is a test payload";
        let env = encrypt_password(b"correct horse", fast_params(), payload).unwrap();
        let back = decrypt_password(b"correct horse", &env).unwrap();
        assert_eq!(back, payload);
    }

    #[test]
    fn password_roundtrips_multi_frame_payload() {
        // 64 KiB > one 16 KiB frame → forces multi-frame path.
        let payload: Vec<u8> = (0u32..65_000).map(|i| (i & 0xff) as u8).collect();
        let env = encrypt_password(b"pw", fast_params(), &payload).unwrap();
        let back = decrypt_password(b"pw", &env).unwrap();
        assert_eq!(back, payload);
    }

    #[test]
    fn wrong_password_fails_authentication() {
        let env = encrypt_password(b"right", fast_params(), b"secret").unwrap();
        assert!(decrypt_password(b"wrong", &env).is_err());
    }

    #[test]
    fn truncation_fails_authentication() {
        let payload: Vec<u8> = (0u32..40_000).map(|i| (i & 0xff) as u8).collect();
        let env = encrypt_password(b"pw", fast_params(), &payload).unwrap();
        let truncated = &env[..env.len() - 1];
        assert!(decrypt_password(b"pw", truncated).is_err());
    }

    #[test]
    fn bit_flip_fails_authentication() {
        let env_orig = encrypt_password(b"pw", fast_params(), b"abcdef").unwrap();
        let mut env = env_orig.clone();
        let last = env.len() - 1;
        env[last] ^= 0x01;
        assert!(decrypt_password(b"pw", &env).is_err());
    }

    #[test]
    fn envelope_starts_with_tekflash_magic() {
        let env = encrypt_password(b"pw", fast_params(), b"x").unwrap();
        assert_eq!(&env[..4], MAGIC);
        assert_eq!(env[4], VERSION);
        assert_eq!(env[5], SCHEME_PASSWORD);
    }
}
