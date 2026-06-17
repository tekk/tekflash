//! macOS device enumeration via `diskutil list -plist` + per-disk `diskutil info -plist`.
//!
//! We shell out instead of binding DiskArbitration directly because `diskutil` is the
//! canonical OS-blessed source of truth, ships on every macOS, and its plist output is
//! stable across releases. Parsing it is much cheaper than the IOKit / DA dance.

use super::{BlockDevice, Transport};
use color_eyre::eyre::{eyre, Context, Result};
use plist::Value;
use std::path::PathBuf;
use std::process::Command;

pub fn enumerate_raw() -> Result<Vec<BlockDevice>> {
    let list_out = Command::new("diskutil")
        .args(["list", "-plist", "physical"])
        .output()
        .context("failed to invoke `diskutil list -plist physical`")?;
    if !list_out.status.success() {
        return Err(eyre!(
            "diskutil list failed: {}",
            String::from_utf8_lossy(&list_out.stderr)
        ));
    }

    let root: Value = plist::from_bytes(&list_out.stdout).context("parsing diskutil list plist")?;
    let dict = root
        .as_dictionary()
        .ok_or_else(|| eyre!("diskutil list root is not a dictionary"))?;
    let whole_disks = dict
        .get("WholeDisks")
        .and_then(Value::as_array)
        .ok_or_else(|| eyre!("diskutil list is missing WholeDisks"))?;

    let mut devices = Vec::with_capacity(whole_disks.len());
    for d in whole_disks {
        let Some(name) = d.as_string() else { continue };
        match info_for(name) {
            Ok(dev) => devices.push(dev),
            Err(e) => tracing::warn!(disk = name, error = %e, "skipping disk"),
        }
    }
    Ok(devices)
}

fn info_for(disk: &str) -> Result<BlockDevice> {
    let out = Command::new("diskutil")
        .args(["info", "-plist", disk])
        .output()
        .with_context(|| format!("diskutil info -plist {disk}"))?;
    if !out.status.success() {
        return Err(eyre!(
            "diskutil info {disk} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let v: Value = plist::from_bytes(&out.stdout)?;
    let info = v
        .as_dictionary()
        .ok_or_else(|| eyre!("diskutil info root is not a dictionary"))?;

    let get_string = |k: &str| -> Option<String> {
        info.get(k)
            .and_then(Value::as_string)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let get_bool = |k: &str| -> bool { info.get(k).and_then(Value::as_boolean).unwrap_or(false) };
    let get_u64 = |k: &str| -> Option<u64> { info.get(k).and_then(Value::as_unsigned_integer) };

    let path = PathBuf::from(format!("/dev/{disk}"));
    let vendor = get_string("MediaName")
        .or_else(|| get_string("IORegistryEntryName"))
        .or_else(|| get_string("DeviceVendor"));
    let model = get_string("DeviceModel").or_else(|| get_string("MediaName"));
    let serial = get_string("DeviceSerial").or_else(|| get_string("IORegistryEntrySerialNumber"));
    let size_bytes = get_u64("TotalSize")
        .or_else(|| get_u64("Size"))
        .unwrap_or(0);
    let block_size = get_u64("DeviceBlockSize").unwrap_or(512) as u32;
    let removable_media = get_bool("RemovableMedia") || get_bool("Removable");
    let bus_protocol = get_string("BusProtocol").unwrap_or_default();
    let transport = match bus_protocol.as_str() {
        "USB" => Transport::Usb,
        "SD" => Transport::Sd,
        "Secure Digital" => Transport::Sd,
        "MMC" => Transport::Mmc,
        "SATA" => Transport::Sata,
        "PCI-Express" => Transport::Nvme,
        "PCI" => Transport::Pcie,
        "Thunderbolt" => Transport::Thunderbolt,
        "SCSI" => Transport::Scsi,
        _ => Transport::Unknown,
    };
    let is_system = disk == "disk0" || get_bool("Internal");
    let read_only = info
        .get("Writable")
        .and_then(Value::as_boolean)
        .map(|w| !w)
        .unwrap_or(false);

    let mountpoints = info
        .get("MountPoint")
        .and_then(Value::as_string)
        .filter(|s| !s.is_empty())
        .map(|s| vec![PathBuf::from(s)])
        .unwrap_or_default();

    Ok(BlockDevice {
        path,
        vendor,
        model,
        serial,
        size_bytes,
        block_size,
        transport,
        removable: removable_media
            || matches!(
                transport,
                Transport::Usb | Transport::Sd | Transport::Mmc | Transport::Thunderbolt
            ),
        is_system,
        read_only,
        mountpoints,
    })
}

/// Translate a user-supplied path like `/dev/disk5` to its raw equivalent `/dev/rdisk5`,
/// which is dramatically faster (unbuffered). Falls back to the original path if the raw
/// node doesn't exist (network disks etc.).
pub fn raw_path(path: &std::path::Path) -> std::path::PathBuf {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return path.to_path_buf();
    };
    if let Some(rest) = name.strip_prefix("disk") {
        let raw = std::path::PathBuf::from(format!("/dev/rdisk{rest}"));
        if raw.exists() {
            return raw;
        }
    }
    path.to_path_buf()
}
