#[cfg(test)]
mod contract;

pub mod entries;
pub mod misc;
pub mod vault;

use tauri::{AppHandle, Emitter};

use crate::state::AppState;

/// Tells the UI the lock status changed (unlock, lock, auto-lock).
/// The UI never polls; it listens for this.
pub fn emit_status(app: &AppHandle, state: &AppState) {
    let payload = serde_json::json!({
        "status": state.status(),
        "vault_id": state.current_id(),
    });
    let _ = app.emit("vault-status", payload);
}
