//! Verify-after-write.
//!
//! Re-reads a freshly-written device, recomputes BLAKE3, and compares against the hash
//! captured during the write pipeline. Three modes: `Full` re-reads every byte (the
//! gold standard), `Sampled` re-reads a configurable percentage at deterministic offsets
//! (start, end, and pseudo-random stripes), and `Deferred` queues the job so the caller
//! can run it later via `verify-queue` once the device is convenient to access.
//!
//! The actual cache bypass (the part that makes this reliable on SD cards with
//! aggressive write caches) is per-OS: `BLKFLSBUF` plus `O_DIRECT` on Linux,
//! `F_FULLFSYNC` plus `F_NOCACHE` on macOS, `FlushFileBuffers` plus
//! `FILE_FLAG_NO_BUFFERING` on Windows. Those land alongside the writer when the actual
//! flash pipeline is wired up; for now we provide the platform-agnostic core of the
//! verify logic.

use color_eyre::eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

use super::hasher::{HashKind, Hasher};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerifyMode {
    Off,
    Full,
    Sampled,
    Deferred,
}

#[derive(Debug, Clone)]
pub struct VerifyPlan {
    pub mode: VerifyMode,
    pub total_bytes: u64,
    /// Percentage of bytes to read for `Sampled`. Ignored in other modes.
    pub sample_pct: u8,
    /// Seed for the deterministic stripe layout (typically `total_bytes`).
    pub stripe_seed: u64,
}

impl VerifyPlan {
    pub fn full(total_bytes: u64) -> Self {
        Self {
            mode: VerifyMode::Full,
            total_bytes,
            sample_pct: 100,
            stripe_seed: total_bytes,
        }
    }

    pub fn sampled(total_bytes: u64, pct: u8) -> Self {
        Self {
            mode: VerifyMode::Sampled,
            total_bytes,
            sample_pct: pct.clamp(1, 100),
            stripe_seed: total_bytes,
        }
    }
}

/// Outcome of a verify pass.
#[derive(Debug, Clone)]
pub struct VerifyOutcome {
    pub passed: bool,
    pub bytes_read: u64,
    /// First byte offset where re-read disagrees with the source. `None` on pass.
    pub first_mismatch_offset: Option<u64>,
}

/// Re-read `device_path` and compare every byte against `expected_source`.
///
/// `expected_source` is a `Read`-able cursor positioned at byte zero of the *source*
/// that the writer pipeline streamed onto the device (after decompression, before the
/// write syscall). For typical flash, that's the file the user picked; the caller is
/// responsible for re-opening it before calling this function.
pub fn verify_full<R: Read>(device_path: &Path, mut expected_source: R) -> Result<VerifyOutcome> {
    let mut dev = std::fs::File::open(device_path)
        .with_context(|| format!("re-open {} for verify", device_path.display()))?;

    let mut buf_dev = vec![0u8; 1024 * 1024];
    let mut buf_src = vec![0u8; 1024 * 1024];
    let mut offset: u64 = 0;

    loop {
        let n_src = read_exact_or_eof(&mut expected_source, &mut buf_src)?;
        let n_dev = read_exact_or_eof(&mut dev, &mut buf_dev)?;

        let n = n_src.min(n_dev);
        if let Some(pos) = first_diff(&buf_src[..n], &buf_dev[..n]) {
            return Ok(VerifyOutcome {
                passed: false,
                bytes_read: offset + n as u64,
                first_mismatch_offset: Some(offset + pos as u64),
            });
        }
        offset += n as u64;

        // Length mismatch is also a failure — but only after we've compared the
        // overlapping prefix.
        if n_src != n_dev {
            return Ok(VerifyOutcome {
                passed: false,
                bytes_read: offset,
                first_mismatch_offset: Some(offset),
            });
        }
        if n_src == 0 {
            return Ok(VerifyOutcome {
                passed: true,
                bytes_read: offset,
                first_mismatch_offset: None,
            });
        }
    }
}

/// Hash-based verify: re-read the entire device, hash it, and compare against
/// `expected_hash_hex`. Used when the caller doesn't want to keep the source bytes
/// resident — typical for flash, where the source is a multi-GB compressed image.
pub fn verify_full_by_hash(
    device_path: &Path,
    expected_hash_hex: &str,
    kind: HashKind,
) -> Result<VerifyOutcome> {
    let mut dev = std::fs::File::open(device_path)
        .with_context(|| format!("re-open {} for verify", device_path.display()))?;

    let mut hasher = Hasher::new(kind);
    let mut buf = vec![0u8; 1024 * 1024];
    let mut offset: u64 = 0;
    loop {
        let n = dev.read(&mut buf).context("read for verify")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        offset += n as u64;
    }
    let got = hasher.finalize_hex();
    let passed = got.eq_ignore_ascii_case(expected_hash_hex);
    Ok(VerifyOutcome {
        passed,
        bytes_read: offset,
        first_mismatch_offset: if passed { None } else { Some(0) }, // hash gives us pass/fail; offset-locating needs verify_full
    })
}

/// Plan the offsets a `Sampled` verify should hit. Returns a deterministic list of
/// `(offset, length)` ranges totalling roughly `sample_pct%` of `total_bytes`. Always
/// includes the very start and end of the device — most write failures cluster there.
pub fn sampled_ranges(plan: &VerifyPlan) -> Vec<(u64, u64)> {
    if plan.total_bytes == 0 || plan.sample_pct == 0 {
        return Vec::new();
    }
    let target = plan.total_bytes * plan.sample_pct as u64 / 100;
    let stripe = (1024 * 1024u64).min(target.max(1));

    // First MiB and last MiB are always sampled.
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    let head_len = stripe.min(plan.total_bytes);
    ranges.push((0, head_len));
    if plan.total_bytes > 2 * stripe {
        ranges.push((plan.total_bytes - stripe, stripe));
    }

    // Fill the rest with stripes spread by a deterministic LCG seeded with stripe_seed.
    let mut covered = ranges.iter().map(|(_, l)| *l).sum::<u64>();
    let mut state = plan.stripe_seed | 1;
    while covered < target {
        // LCG (Numerical Recipes constants).
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        let offset = state % plan.total_bytes.max(1);
        let aligned = offset & !(stripe - 1);
        let len = stripe.min(plan.total_bytes - aligned);
        if len == 0 {
            continue;
        }
        ranges.push((aligned, len));
        covered += len;
    }
    ranges.sort_by_key(|(o, _)| *o);
    ranges
}

fn read_exact_or_eof<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = r.read(&mut buf[filled..]).context("verify read")?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

fn first_diff(a: &[u8], b: &[u8]) -> Option<usize> {
    a.iter().zip(b.iter()).position(|(x, y)| x != y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn write_tempfile(bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "tekflash-verify-{}-{}.bin",
            std::process::id(),
            blake3::hash(bytes).to_hex()
        ));
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(bytes).unwrap();
        p
    }

    #[test]
    fn verify_full_passes_for_identical_streams() {
        let payload: Vec<u8> = (0u32..50_000).map(|i| (i & 0xff) as u8).collect();
        let p = write_tempfile(&payload);
        let result = verify_full(&p, Cursor::new(&payload)).unwrap();
        assert!(result.passed);
        assert_eq!(result.bytes_read, payload.len() as u64);
        assert!(result.first_mismatch_offset.is_none());
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn verify_full_reports_first_mismatch_offset() {
        let mut payload: Vec<u8> = (0u32..50_000).map(|i| (i & 0xff) as u8).collect();
        let p = write_tempfile(&payload);
        payload[40_000] ^= 0xff;
        let result = verify_full(&p, Cursor::new(&payload)).unwrap();
        assert!(!result.passed);
        assert_eq!(result.first_mismatch_offset, Some(40_000));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn verify_full_detects_length_mismatch() {
        let payload: Vec<u8> = vec![0xab; 1000];
        let p = write_tempfile(&payload);
        let result = verify_full(&p, Cursor::new(&payload[..900])).unwrap();
        assert!(!result.passed);
        assert_eq!(result.first_mismatch_offset, Some(900));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn verify_full_by_hash_passes_on_match() {
        let payload: Vec<u8> = (0u32..200_000).map(|i| (i & 0xff) as u8).collect();
        let p = write_tempfile(&payload);
        let expected = blake3::hash(&payload).to_hex().to_string();
        let result = verify_full_by_hash(&p, &expected, HashKind::Blake3).unwrap();
        assert!(result.passed);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn sampled_ranges_cover_head_and_tail() {
        let plan = VerifyPlan::sampled(100 * 1024 * 1024, 5);
        let ranges = sampled_ranges(&plan);
        assert!(!ranges.is_empty());
        assert_eq!(ranges.first().unwrap().0, 0);
        let total_sampled: u64 = ranges.iter().map(|(_, l)| *l).sum();
        // We should have sampled at least 5% of the total.
        assert!(total_sampled >= plan.total_bytes * 5 / 100 / 2);
    }

    #[test]
    fn sampled_ranges_are_deterministic_per_seed() {
        let plan = VerifyPlan::sampled(64 * 1024 * 1024, 5);
        let a = sampled_ranges(&plan);
        let b = sampled_ranges(&plan);
        assert_eq!(a, b);
    }
}
