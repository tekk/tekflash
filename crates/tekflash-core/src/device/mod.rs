//! Block device enumeration + I/O entry points.
//!
//! Each OS has its own implementation module. The public surface is the same:
//! `enumerate()` returns a list of `BlockDevice` records that the safety filter has
//! already pruned (or annotated, for `--show-all`).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod safety;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos as backend;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub use linux as backend;

#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
pub use windows as backend;

/// Transport reported by the OS. Used for UI labelling and for filtering: removable
/// transports (USB, SD, MMC, Thunderbolt) are listed first; internal ones (SATA, NVMe,
/// PCIe) are hidden behind `--show-all`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Usb,
    Sd,
    Mmc,
    Sata,
    Nvme,
    Pcie,
    Thunderbolt,
    Scsi,
    Virtual,
    Unknown,
}

/// One physical block device the user might want to write to or read from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDevice {
    /// OS-specific node path. `/dev/diskN` on macOS, `/dev/sdX` on Linux, `\\.\PhysicalDriveN` on Windows.
    pub path: PathBuf,
    /// Vendor string from the device (e.g. "SanDisk").
    pub vendor: Option<String>,
    /// Model string from the device (e.g. "Ultra USB 3.0").
    pub model: Option<String>,
    /// Hardware serial, when the OS exposes it. Useful for distinguishing identical sticks.
    pub serial: Option<String>,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Sector size in bytes (typically 512 or 4096).
    pub block_size: u32,
    /// Bus / transport.
    pub transport: Transport,
    /// True for USB, SD, MMC, and Thunderbolt enclosures — i.e. things a user would deliberately plug in.
    pub removable: bool,
    /// True if the OS reports this disk contains the running root filesystem (or, on macOS, is disk0).
    pub is_system: bool,
    /// True if write-protect is asserted (e.g. SD card lock switch).
    pub read_only: bool,
    /// Mountpoints of any partitions on this disk. If non-empty, writing requires unmount.
    pub mountpoints: Vec<PathBuf>,
}

impl BlockDevice {
    /// Human-readable size like "31.9 GB".
    pub fn size_human(&self) -> String {
        format_bytes(self.size_bytes)
    }

    /// Combined "Vendor Model" string, or `path` if neither is known.
    pub fn name(&self) -> String {
        match (&self.vendor, &self.model) {
            (Some(v), Some(m)) => format!("{v} {m}"),
            (None, Some(m)) => m.clone(),
            (Some(v), None) => v.clone(),
            (None, None) => self.path.display().to_string(),
        }
    }
}

/// Enumerate all block devices known to the OS, applying the safety filter.
///
/// When `show_all` is true, internal/system disks are included (with `is_system = true`
/// set so the UI can mark them visually). Otherwise they are excluded.
pub fn enumerate(show_all: bool) -> color_eyre::Result<Vec<BlockDevice>> {
    let all = backend::enumerate_raw()?;
    Ok(safety::filter(all, show_all))
}

/// Translate a user-supplied device path into the per-OS *fastest* equivalent for raw
/// I/O. On macOS this rewrites `/dev/diskN` -> `/dev/rdiskN` (an order-of-magnitude
/// faster on USB / SD because it bypasses the kernel's buffered block-device layer).
/// On Linux and Windows the OS already has one canonical device node, so the original
/// path is returned unchanged.
pub fn resolve_fast_path(path: &std::path::Path) -> std::path::PathBuf {
    backend::raw_path(path)
}

/// Open `path` for reading with the per-OS fast-path hints applied. Pair with
/// [`resolve_fast_path`] when the caller wants the path rewrite too.
///
/// Hints applied:
///
/// - **macOS**: `F_NOCACHE` (don't pollute the page cache with a one-shot read of a
///   whole device) and `F_RDAHEAD` (request kernel read-ahead). The combination is
///   harmless on `/dev/rdiskN` (already unbuffered) and a significant win on regular
///   files.
/// - **Linux**: `posix_fadvise(SEQUENTIAL)` and `posix_fadvise(NOREUSE)` — tells the
///   kernel to be aggressive with read-ahead and to drop pages quickly after use.
///   `O_DIRECT` would bypass the cache entirely but requires sector-aligned buffers
///   throughout the pipeline; that's a follow-up.
/// - **Windows**: `FILE_FLAG_SEQUENTIAL_SCAN` at open time — the same intent.
pub fn open_fast_read(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    #[cfg(windows)]
    {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
        return OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
            .open(path);
    }
    #[cfg(not(windows))]
    {
        let f = std::fs::File::open(path)?;
        apply_unix_fast_read_hints(&f);
        Ok(f)
    }
}

#[cfg(target_os = "macos")]
fn apply_unix_fast_read_hints(f: &std::fs::File) {
    use std::os::fd::AsRawFd;
    // Both fcntls are best-effort — they return an error on file types that don't
    // support them (e.g. pipes) and that's fine.
    unsafe {
        let fd = f.as_raw_fd();
        let _ = libc::fcntl(fd, libc::F_NOCACHE, 1);
        let _ = libc::fcntl(fd, libc::F_RDAHEAD, 1);
    }
}

#[cfg(target_os = "linux")]
fn apply_unix_fast_read_hints(f: &std::fs::File) {
    use std::os::fd::AsRawFd;
    unsafe {
        let fd = f.as_raw_fd();
        let _ = libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_SEQUENTIAL);
        let _ = libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_NOREUSE);
    }
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn apply_unix_fast_read_hints(_f: &std::fs::File) {}

fn format_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = n as f64;
    let mut idx = 0;
    while v >= 1000.0 && idx + 1 < UNITS.len() {
        v /= 1000.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{} {}", n, UNITS[0])
    } else {
        format!("{:.1} {}", v, UNITS[idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn resolve_fast_path_returns_unrelated_paths_unchanged() {
        let p = PathBuf::from("/tmp/definitely-not-a-block-device-2026");
        assert_eq!(resolve_fast_path(&p), p);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn resolve_fast_path_rewrites_disk_to_rdisk_when_present() {
        // /dev/disk0 (boot disk) and /dev/rdisk0 always exist on macOS.
        let p = PathBuf::from("/dev/disk0");
        let r = resolve_fast_path(&p);
        assert_eq!(
            r,
            PathBuf::from("/dev/rdisk0"),
            "expected /dev/disk0 -> /dev/rdisk0, got {}",
            r.display()
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn resolve_fast_path_passes_already_raw_paths_through() {
        let p = PathBuf::from("/dev/rdisk0");
        let r = resolve_fast_path(&p);
        // The function doesn't recognize a "rdisk" prefix as needing further rewrite —
        // it just returns the original path. (And rdiskrdisk0 wouldn't exist anyway.)
        assert_eq!(r, p);
    }
}
