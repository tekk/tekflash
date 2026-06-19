//! Linux device enumeration via `lsblk -J -O -b`.
//!
//! `lsblk` is on every distro (it's part of util-linux), its JSON output is stable, and
//! it already knows about transport, removable, mountpoints, model, vendor, and serial.

use super::{BlockDevice, Transport};
use color_eyre::eyre::{eyre, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Deserialize)]
struct LsblkOut {
    blockdevices: Vec<LsblkDev>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // serde fields read at parse time; many are reserved for richer UI later
struct LsblkDev {
    name: String,
    #[serde(default)]
    kname: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(rename = "phy-sec", default)]
    phy_sec: Option<u32>,
    #[serde(default)]
    rota: Option<bool>,
    #[serde(default)]
    rm: Option<bool>,
    #[serde(default)]
    ro: Option<bool>,
    #[serde(default)]
    tran: Option<String>,
    #[serde(default)]
    vendor: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    serial: Option<String>,
    #[serde(default)]
    mountpoint: Option<String>,
    #[serde(default)]
    mountpoints: Option<Vec<Option<String>>>,
    #[serde(default)]
    children: Vec<LsblkDev>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

pub fn enumerate_raw() -> Result<Vec<BlockDevice>> {
    let out = Command::new("lsblk")
        .args([
            "-J", "-b", "-O",
            "-o",
            "NAME,KNAME,SIZE,PHY-SEC,ROTA,RM,RO,TRAN,VENDOR,MODEL,SERIAL,MOUNTPOINT,MOUNTPOINTS,TYPE",
        ])
        .output()
        .context("failed to invoke `lsblk`")?;
    if !out.status.success() {
        return Err(eyre!(
            "lsblk failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let parsed: LsblkOut = serde_json::from_slice(&out.stdout).context("parsing lsblk JSON")?;

    let root_device_name = root_device_name();
    let mut out_devs = Vec::with_capacity(parsed.blockdevices.len());
    for d in parsed.blockdevices {
        if !matches!(d.kind.as_deref(), Some("disk") | Some("rom") | None) {
            continue;
        }
        out_devs.push(to_block(&d, root_device_name.as_deref()));
    }
    Ok(out_devs)
}

fn to_block(d: &LsblkDev, root_dev: Option<&str>) -> BlockDevice {
    let kname = d.kname.clone().unwrap_or_else(|| d.name.clone());
    let path = PathBuf::from(format!("/dev/{kname}"));

    let mut mountpoints: Vec<PathBuf> = d
        .mountpoint
        .iter()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    if let Some(list) = &d.mountpoints {
        for m in list.iter().flatten() {
            if !m.is_empty() {
                mountpoints.push(PathBuf::from(m));
            }
        }
    }
    // Children (partitions) carry their own mountpoints — propagate to the parent disk.
    for c in &d.children {
        if let Some(m) = &c.mountpoint {
            if !m.is_empty() {
                mountpoints.push(PathBuf::from(m));
            }
        }
        if let Some(list) = &c.mountpoints {
            for m in list.iter().flatten() {
                if !m.is_empty() {
                    mountpoints.push(PathBuf::from(m));
                }
            }
        }
    }
    mountpoints.sort();
    mountpoints.dedup();

    let transport = match d.tran.as_deref() {
        Some("usb") => Transport::Usb,
        Some("sd") | Some("mmc") => Transport::Mmc,
        Some("sata") => Transport::Sata,
        Some("nvme") => Transport::Nvme,
        Some("pcie") => Transport::Pcie,
        Some("scsi") => Transport::Scsi,
        Some("ata") => Transport::Sata,
        _ => Transport::Unknown,
    };

    let removable = d.rm.unwrap_or(false)
        || matches!(transport, Transport::Usb | Transport::Sd | Transport::Mmc);
    let is_system = root_dev.map(|r| r == kname).unwrap_or(false);

    BlockDevice {
        path,
        vendor: d
            .vendor
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        model: d
            .model
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        serial: d
            .serial
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        size_bytes: d.size.unwrap_or(0),
        block_size: d.phy_sec.unwrap_or(512),
        transport,
        removable,
        is_system,
        read_only: d.ro.unwrap_or(false),
        mountpoints,
    }
}

/// Find the kname of the disk that hosts the running root filesystem, so we can mark it
/// as `is_system` and refuse to write it by default.
fn root_device_name() -> Option<String> {
    let out = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", "/"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let source = String::from_utf8(out.stdout).ok()?;
    let source = source.trim();
    // Walk pkname chain (LVM/dm/luks -> underlying disk).
    let pk = Command::new("lsblk")
        .args(["-n", "-o", "PKNAME", source])
        .output()
        .ok()?;
    if pk.status.success() {
        let name = String::from_utf8_lossy(&pk.stdout).trim().to_string();
        if !name.is_empty() {
            return Some(name.lines().next().unwrap_or("").to_string());
        }
    }
    // Fall back to the source basename (e.g. /dev/sda1 -> sda1 -> strip trailing digits).
    let base = std::path::Path::new(source)
        .file_name()
        .and_then(|s| s.to_str())?
        .to_string();
    Some(base.trim_end_matches(char::is_numeric).to_string())
}

/// On Linux, the path the user picked is the path we open. There's no rdisk equivalent.
pub fn raw_path(path: &std::path::Path) -> std::path::PathBuf {
    path.to_path_buf()
}
