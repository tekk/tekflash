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

    // `diskutil info` reports MountPoint only for whole-disk filesystems (e.g. HFS+ on
    // unpartitioned media). For partitioned disks the mount lives on a child slice like
    // `/dev/disk5s2`, so we also scan `mount` output to pick up partition mounts.
    let mut mountpoints: Vec<PathBuf> = info
        .get("MountPoint")
        .and_then(Value::as_string)
        .filter(|s| !s.is_empty())
        .map(|s| vec![PathBuf::from(s)])
        .unwrap_or_default();
    let disk_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    for mp in mountpoints_for_disk(disk_name) {
        if !mountpoints.contains(&mp) {
            mountpoints.push(mp);
        }
    }

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

/// Parse `mount(8)` output to find every mountpoint backed by `/dev/<disk>` or one of
/// its partitions (e.g. `/dev/disk5`, `/dev/disk5s2`). Returns an empty vec on any error
/// or when nothing matches — the caller treats that as "nothing mounted".
fn mountpoints_for_disk(disk: &str) -> Vec<PathBuf> {
    if disk.is_empty() {
        return Vec::new();
    }
    let Ok(out) = Command::new("mount").output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let dev_prefix = format!("/dev/{disk}");
    let mut result = Vec::new();
    for line in text.lines() {
        if !line.starts_with(&dev_prefix) {
            continue;
        }
        // After the prefix, accept either end-of-token (whole disk) or "sN..." (slice).
        let after = &line[dev_prefix.len()..];
        let next_char = after.chars().next();
        let is_match = match next_char {
            None | Some(' ') | Some('\t') => true,
            Some('s') => after.chars().nth(1).is_some_and(|c| c.is_ascii_digit()),
            _ => false,
        };
        if !is_match {
            continue;
        }
        // Format: "/dev/diskNs[N] on /path/to/mount (type, ...)"
        if let Some(on_idx) = line.find(" on ") {
            let after_on = &line[on_idx + 4..];
            if let Some(paren_idx) = after_on.find(" (") {
                let path = after_on[..paren_idx].trim();
                if !path.is_empty() {
                    result.push(PathBuf::from(path));
                }
            }
        }
    }
    result
}

/// Best-effort mount of a /dev/diskN by shelling out to `diskutil mountDisk`. Returns
/// the first mountpoint that comes up afterwards, or an error if mounting fails or no
/// mountpoint appears within a short wait.
pub fn try_mount(device_path: &std::path::Path) -> color_eyre::Result<PathBuf> {
    use color_eyre::eyre::eyre;

    let disk_name = device_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| eyre!("not a /dev/disk* path: {}", device_path.display()))?
        .trim_start_matches('r')
        .to_string();
    let target = format!("/dev/{disk_name}");
    let out = Command::new("diskutil")
        .args(["mountDisk", &target])
        .output()
        .map_err(|e| eyre!("diskutil mountDisk {target}: {e}"))?;
    if !out.status.success() {
        return Err(eyre!(
            "diskutil mountDisk {target} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    // Give the kernel a beat to publish the new mount, then look it up via mount(8).
    for _ in 0..10 {
        let mps = mountpoints_for_disk(&disk_name);
        if let Some(mp) = mps.into_iter().next() {
            return Ok(mp);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(eyre!(
        "mounted {target} but no mountpoint became visible — is the volume usable?"
    ))
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
