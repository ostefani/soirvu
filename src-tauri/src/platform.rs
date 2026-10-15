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
