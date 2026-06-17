//! Fail-fast root/Administrator check.
//!
//! Tekflash needs raw block-device access on every supported platform, which means it
//! needs to start elevated. We want to detect that *before* the user picks a device and
//! kicks off a long-running pipeline — surfacing the right platform-specific advice.

use std::io::Write;

/// Result of an elevation check.
#[derive(Debug, Clone)]
pub struct ElevationStatus {
    pub elevated: bool,
    pub advice: &'static str,
}

/// Detect whether the current process is running with the privileges needed for raw
/// block-device access.
pub fn check() -> ElevationStatus {
    #[cfg(unix)]
    {
        let euid = unsafe { libc::geteuid() };
        ElevationStatus {
            elevated: euid == 0,
            advice: unix_advice(),
        }
    }
    #[cfg(windows)]
    {
        ElevationStatus {
            elevated: windows_is_elevated(),
            advice: WINDOWS_ADVICE,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        ElevationStatus {
            elevated: false,
            advice: "Unsupported platform.",
        }
    }
}

/// Print advice and exit(1) when not elevated. Call this once at startup, before any
/// device enumeration.
pub fn require_elevation() {
    let status = check();
    if !status.elevated {
        let mut err = std::io::stderr().lock();
        let _ = writeln!(
            err,
            "tekflash needs elevated privileges to access raw block devices."
        );
        let _ = writeln!(err, "{}", status.advice);
        std::process::exit(1);
    }
}

#[cfg(unix)]
const fn unix_advice() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Try:  sudo tekflash\n\
         On macOS you may also need to grant your terminal 'Full Disk Access' in\n\
         System Settings → Privacy & Security if reads from /dev/rdiskN fail with EPERM."
    }
    #[cfg(target_os = "linux")]
    {
        "Try:  sudo tekflash"
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "Re-run as root."
    }
}

#[cfg(windows)]
const WINDOWS_ADVICE: &str = "Right-click tekflash.exe and choose 'Run as administrator',\n\
                              or from an elevated PowerShell:  Start-Process -Verb RunAs tekflash";

#[cfg(windows)]
fn windows_is_elevated() -> bool {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation: TOKEN_ELEVATION = zeroed();
        let mut size: u32 = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        );
        let _ = CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}
