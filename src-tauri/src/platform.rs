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
