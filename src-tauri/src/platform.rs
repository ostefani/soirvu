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

// WER (Windows Error Reporting) exclusion: tells Windows
// never to write a crash dump for this executable (§9).
#[cfg(target_os = "windows")]
#[link(name = "wer")]
unsafe extern "C" {
    fn WerAddExcludedApplication(exe_name: *const u16, all_users: i32) -> i32;
}

// libc does not export SetErrorMode for Windows; declared
// directly (kernel32), with its two mode constants (§9).
#[cfg(target_os = "windows")]
unsafe extern "C" {
    fn SetErrorMode(mode: u32) -> u32;
}

#[cfg(target_os = "windows")]
const SEM_FAILCRITICALERRORS: u32 = 0x0001;
#[cfg(target_os = "windows")]
const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;

/// Rename, retrying briefly (mirrors the core helper): on
/// Windows a just-closed file or folder can stay
/// unrenamable while an antivirus or the indexer holds it
/// (§12). Used for the backup and rotation swaps.
pub fn rename_retry(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    let mut result = std::fs::rename(from, to);
    if cfg!(windows) {
        for _ in 0..4 {
            if result.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
            result = std::fs::rename(from, to);
        }
    }
    result
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
    #[cfg(all(target_os = "windows", not(debug_assertions)))]
    unsafe {
        // §9 on Windows: no error dialog pausing a crashed
        // process with its memory on screen, and WER must
        // not write a dump of this process anywhere.
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
        if let Some(name) = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        {
            let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            WerAddExcludedApplication(wide.as_ptr(), 0);
        }
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
    // OneDrive's sync root is a path component of its own
    // ("OneDrive" or "OneDrive - <org>") wherever it lives —
    // on Windows it sits under the profile, which $HOME does
    // not name there. A folder with that name anywhere in
    // the path is the sync tree or inside it.
    if path
        .components()
        .any(|c| {
            let name = c.as_os_str().to_string_lossy();
            name == "OneDrive" || name.starts_with("OneDrive - ")
        })
    {
        return true;
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = home.to_string_lossy().into_owned();
        for name in ["Dropbox", "Google Drive", "OneDrive", "Nextcloud"] {
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

#[cfg(target_os = "linux")]
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
    is_network_magic(stat.f_type)
}

/// Linux statfs f_type magics for filesystems a vault should
/// not live on (§3): NFS, SMB/CIFS (both generations), FUSE
/// (sshfs, rclone and WebDAV mounts are FUSE — a purely local
/// FUSE vault earns a warning it can ignore, which is the
/// right trade for a warn-never-block check), 9p and
/// VirtualBox shared folders (host shares from a VM).
#[cfg(target_os = "linux")]
fn is_network_magic(f_type: i64) -> bool {
    matches!(
        f_type,
        0x6969          // NFS
        | 0x517B        // SMB (first generation)
        | 0xFE534D42    // SMB2
        | 0x65735546    // FUSE
        | 0x0102_1997   // 9p
        | 0x786F4256 // VirtualBox shared folder
    )
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
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
    #[cfg(target_os = "windows")]
    {
        Box::new(crate::windows_lock::WindowsLockEvents::default())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Box::new(NoLockEvents)
    }
}

/// Platforms without a backend yet: no OS signals.
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
struct NoLockEvents;

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
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

    #[cfg(target_os = "linux")]
    #[test]
    fn network_magics_warn() {
        for magic in [0x6969, 0x517B, 0xFE534D42, 0x65735546, 0x0102_1997, 0x786F4256] {
            assert!(is_network_magic(magic), "{magic:#x}");
        }
        for magic in [0xEF53, 0x794C7630, 0x0102_1994, 0x9123683E] {
            assert!(!is_network_magic(magic), "{magic:#x}");
        }
        // The root filesystem of a normal machine or CI
        // runner (ext4, overlayfs, btrfs, xfs) never warns.
        assert_eq!(fs_warning(std::path::Path::new("/")), None);
    }

    #[test]
    fn onedrive_component_warns() {
        assert_eq!(
            fs_warning(std::path::Path::new("C:/Users/x/OneDrive/Diary")),
            Some("cloud_sync")
        );
        assert_eq!(
            fs_warning(std::path::Path::new("/mnt/c/Users/x/OneDrive - Acme/Diary")),
            Some("cloud_sync")
        );
        assert_eq!(fs_warning(std::path::Path::new("/Users/x/OneDriveStuff/Diary")), None);
    }

    #[test]
    fn plain_paths_do_not_warn() {
        assert_eq!(fs_warning(std::path::Path::new("/Users/x/Documents/Diary")), None);
        assert_eq!(fs_warning(std::path::Path::new("/Volumes/Disk Image/Diary")), None);
    }
}
