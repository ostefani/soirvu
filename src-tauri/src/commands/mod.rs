#[cfg(test)]
mod contract;

pub mod attachments;
pub mod entries;
pub mod misc;
pub mod vault;

use tauri::{AppHandle, Emitter};

use crate::state::AppState;

/// Tells the UI the lock status changed (unlock, lock, auto-lock).
/// The UI never polls; it listens for this.
/// §9: on lock the webview document is reloaded onto the lock
/// screen, discarding the editor instance, the undo history and
/// the plaintext strings in the JS heap (best-effort: JS strings
/// cannot be zeroized).
pub fn reload_main_window(app: &AppHandle) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        // Reload ONLY the app document, never the whole window.
        // Under the isolation pattern the window's top document
        // is the isolation driver and the app lives in its
        // iframe; window.reload() restarts the driver too, and
        // after such a restart the app's invokes die in the web
        // view ("Fetch API cannot load ipc://localhost/… due to
        // access control checks"). Reloading the iframe's
        // document discards the editor, undo history and JS
        // heap exactly as §9 asks, and leaves the ipc plumbing
        // as it was at startup. Without an iframe (no driver),
        // the top document IS the app and reloads itself.
        // Reloading is done from the DRIVER side — reassigning
        // the iframe's src — because the app frame is sandboxed
        // cross-origin: touching its contentWindow.location
        // throws a SecurityError and nothing reloads at all.
        let _ = window.eval(
            "const f = document.querySelector('iframe'); \
             if (f) { f.setAttribute('src', f.getAttribute('src')); } \
             else { window.location.reload(); }",
        );
    }
}

pub fn emit_status(app: &AppHandle, state: &AppState) {
    let payload = serde_json::json!({
        "status": state.status(),
        "vault_id": state.current_id(),
    });
    let _ = app.emit("vault-status", payload);
}
