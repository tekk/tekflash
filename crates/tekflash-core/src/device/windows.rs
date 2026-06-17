//! Windows device enumeration via WMI (skeleton).
//!
//! The real implementation shells out to `powershell -NoProfile -Command "Get-Disk |
//! ConvertTo-Json"` (always available, no extra deps, returns rich data) and joins with
//! `Get-Partition` / `Get-Volume` for mountpoints. Raw I/O later opens
//! `\\.\PhysicalDriveN` with `FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH` and locks
//! child volumes via `FSCTL_LOCK_VOLUME` + `FSCTL_DISMOUNT_VOLUME` before write.

use super::{BlockDevice, Transport};
use color_eyre::eyre::{eyre, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PsDisk {
    number: u32,
    #[serde(default)]
    friendly_name: Option<String>,
    #[serde(default)]
    manufacturer: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    serial_number: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    bus_type: Option<String>,
    #[serde(default)]
    is_boot: Option<bool>,
    #[serde(default)]
    is_system: Option<bool>,
    #[serde(default)]
    is_read_only: Option<bool>,
}

pub fn enumerate_raw() -> Result<Vec<BlockDevice>> {
    let out = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-Disk | Select-Object Number,FriendlyName,Manufacturer,Model,SerialNumber,Size,BusType,IsBoot,IsSystem,IsReadOnly | ConvertTo-Json -Depth 2 -Compress",
        ])
        .output()
        .context("failed to invoke PowerShell Get-Disk")?;
    if !out.status.success() {
        return Err(eyre!(
            "Get-Disk failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    // PowerShell emits either an array or a single object depending on count.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stdout = stdout.trim();
    if stdout.is_empty() {
        return Ok(Vec::new());
    }
    let disks: Vec<PsDisk> = if stdout.starts_with('[') {
        serde_json::from_str(stdout).context("parsing Get-Disk JSON array")?
    } else {
        let one: PsDisk = serde_json::from_str(stdout).context("parsing Get-Disk JSON object")?;
        vec![one]
    };

    Ok(disks.into_iter().map(to_block).collect())
}

fn to_block(d: PsDisk) -> BlockDevice {
    let path = PathBuf::from(format!(r"\\.\PhysicalDrive{}", d.number));
    let transport = match d.bus_type.as_deref().unwrap_or("") {
        "USB" => Transport::Usb,
        "SD" => Transport::Sd,
        "MMC" => Transport::Mmc,
        "SATA" => Transport::Sata,
        "NVMe" => Transport::Nvme,
        "SCSI" => Transport::Scsi,
        "Thunderbolt" => Transport::Thunderbolt,
        _ => Transport::Unknown,
    };
    let removable = matches!(
        transport,
        Transport::Usb | Transport::Sd | Transport::Mmc | Transport::Thunderbolt
    );
    let is_system = d.is_boot.unwrap_or(false) || d.is_system.unwrap_or(false);
    BlockDevice {
        path,
        vendor: d.manufacturer,
        model: d.model.or(d.friendly_name),
        serial: d.serial_number,
        size_bytes: d.size.unwrap_or(0),
        block_size: 512,
        transport,
        removable,
        is_system,
        read_only: d.is_read_only.unwrap_or(false),
        mountpoints: Vec::new(),
    }
}

/// Windows opens `\\.\PhysicalDriveN` directly; no separate raw vs cooked node.
pub fn raw_path(path: &std::path::Path) -> std::path::PathBuf {
    path.to_path_buf()
}
