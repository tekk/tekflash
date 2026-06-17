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
