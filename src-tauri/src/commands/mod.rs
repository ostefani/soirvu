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
        let _ = window.reload();
    }
}

pub fn emit_status(app: &AppHandle, state: &AppState) {
    let payload = serde_json::json!({
        "status": state.status(),
        "vault_id": state.current_id(),
    });
    let _ = app.emit("vault-status", payload);
}
