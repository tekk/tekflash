//! Safety filter: hides internal/boot disks by default.

use super::BlockDevice;

/// Apply the show-all toggle. When `show_all` is false, drop devices marked `is_system`.
/// When true, keep them (the UI will paint them red and require extra confirmation).
pub fn filter(devices: Vec<BlockDevice>, show_all: bool) -> Vec<BlockDevice> {
    if show_all {
        devices
    } else {
        devices.into_iter().filter(|d| !d.is_system).collect()
    }
}
