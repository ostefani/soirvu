//! Small per-OS helpers (§2 layout: platform/). v1 targets macOS;
//! the other branches exist so the crate builds everywhere.

use std::process::Command;

/// Opens a URL in the user's default browser/mail client. Used only
/// by the open_link command, after its scheme check and the user's
/// confirmation — the webview itself has no opener (§6.3).
pub fn open_external(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "linux")]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    cmd.spawn().map(|_| ())
}

/// Release-build process hardening (§9): no debugger may
/// attach, and no core dump may carry the diary's memory to
/// disk. Called first in run(), before anything opens. Dev
/// builds keep their debugger and dumps.
pub fn harden_process() {
    #[cfg(all(target_os = "macos", not(debug_assertions)))]
    unsafe {
        libc::ptrace(libc::PT_DENY_ATTACH, 0, std::ptr::null_mut(), 0);
    }
    #[cfg(all(target_os = "linux", not(debug_assertions)))]
    unsafe {
        // The Linux deny-attach (§9): a non-dumpable process
        // cannot be ptraced or have its memory read by a
        // non-root peer, and produces no core dump either.
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
    #[cfg(all(unix, not(debug_assertions)))]
    unsafe {
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        libc::setrlimit(libc::RLIMIT_CORE, &limit);
    }
}

/// The clipboard's change counter (§9): it rises on every copy
/// made by any app, so it identifies "the current clipboard"
/// without ever reading its content. None where unsupported.
pub fn clipboard_change_count() -> Option<i64> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSPasteboard;
        let board = NSPasteboard::generalPasteboard();
        Some(board.changeCount() as i64)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Clears the clipboard, but only if it still holds exactly the
/// copy counted earlier — the user may have copied something
/// else in the meantime, and that is never ours to erase.
pub fn clipboard_clear_if_unchanged(count: i64) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSPasteboard;
        let board = NSPasteboard::generalPasteboard();
        if board.changeCount() as i64 == count {
            board.clearContents();
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = count;
    }
}

/// The §3 filesystem warning for a vault location: WAL files
/// and sync clients do not mix, and SQLite over a network share
/// is unreliable. Warn, never block. Returns "cloud_sync" or
/// "network_fs".
pub fn fs_warning(path: &std::path::Path) -> Option<&'static str> {
    if is_cloud_path(path) {
        return Some("cloud_sync");
    }
    if is_network_fs(path) {
        return Some("network_fs");
    }
    None
}

fn is_cloud_path(path: &std::path::Path) -> bool {
    // Well-known cloud-sync locations (macOS first): iCloud Drive
    // lives under Mobile Documents, the desktop sync apps under
    // CloudStorage or a folder in the home directory.
    const MARKERS: [&str; 2] = ["/Library/Mobile Documents/", "/Library/CloudStorage/"];
    let text = path.to_string_lossy();
    if MARKERS.iter().any(|m| text.contains(m)) {
        return true;
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = home.to_string_lossy().into_owned();
        for name in ["Dropbox", "Google Drive", "OneDrive"] {
            if text.starts_with(&format!("{home}/{name}/")) || text == format!("{home}/{name}") {
                return true;
            }
        }
    }
    false
}

#[cfg(target_os = "macos")]
fn is_network_fs(path: &std::path::Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let Ok(cpath) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(cpath.as_ptr(), &mut stat) } != 0 {
        return false;
    }
    let ftype: Vec<u8> = stat
        .f_fstypename
        .iter()
        .map(|&c| c as u8)
        .take_while(|&c| c != 0)
        .collect();
    matches!(ftype.as_slice(), b"nfs" | b"smbfs" | b"afpfs" | b"webdav")
}

#[cfg(not(target_os = "macos"))]
fn is_network_fs(_path: &std::path::Path) -> bool {
    false
}

/// Why the OS says the diary must lock (§9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockReason {
    /// System sleep, display sleep, or lid close (which sleeps).
    Sleep,
    /// The screen was locked.
    ScreenLocked,
    /// The screensaver started.
    Screensaver,
}

/// OS lock signals (§9). Idle expiry and volume loss are timers
/// in lib.rs; sleep, screen lock and screensaver arrive through
/// this trait, so the rest of the bridge never names a platform
/// type. The macOS backend is `crate::macos_lock`, the Linux
/// one `crate::linux_lock`. Backends may call from a worker
/// thread; the callback must stay cheap and thread-safe.
pub trait LockEvents: Send {
    /// Calls `on_lock` when the OS says the session is no
    /// longer private.
    fn start(&mut self, on_lock: Box<dyn Fn(LockReason) + Send + Sync>) -> Result<(), String>;
}

/// The platform's lock-signal source.
pub fn lock_events() -> Box<dyn LockEvents> {
    #[cfg(target_os = "macos")]
    {
        Box::new(crate::macos_lock::MacLockEvents::default())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(crate::linux_lock::LinuxLockEvents::default())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Box::new(NoLockEvents)
    }
}

/// Platforms without a backend yet: no OS signals.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
struct NoLockEvents;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
impl LockEvents for NoLockEvents {
    fn start(&mut self, _on_lock: Box<dyn Fn(LockReason) + Send + Sync>) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_paths_warn() {
        assert_eq!(
            fs_warning(std::path::Path::new("/Users/x/Library/Mobile Documents/com~apple~CloudDocs/Diary")),
            Some("cloud_sync")
        );
        assert_eq!(
            fs_warning(std::path::Path::new("/Users/x/Library/CloudStorage/Dropbox-Photos/Diary")),
            Some("cloud_sync")
        );
    }

    #[test]
    fn plain_paths_do_not_warn() {
        assert_eq!(fs_warning(std::path::Path::new("/Users/x/Documents/Diary")), None);
        assert_eq!(fs_warning(std::path::Path::new("/Volumes/Disk Image/Diary")), None);
    }
}
