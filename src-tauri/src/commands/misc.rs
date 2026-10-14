//! open_link and settings (§6.1).

use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::sync::oneshot;

use crate::error::ipc;
use crate::platform;
use crate::state::{AppState, Settings};

/// Only https and mailto may leave the app (§6.1).
fn scheme_allowed(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("mailto:")
}

#[tauri::command]
pub async fn open_link(app: AppHandle, url: String) -> Result<bool, String> {
    if !scheme_allowed(&url) {
        return Err("invalid_input".into());
    }
    // A native confirmation, from Rust: the webview cannot skip it.
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .message(format!("Open this link in your browser?\n\n{url}"))
        .title("Soirvu")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Open".into(),
            "Cancel".into(),
        ))
        .show(move |confirmed| {
            let _ = tx.send(confirmed);
        });
    if !rx.await.unwrap_or(false) {
        return Ok(false);
    }
    ipc(platform::open_external(&url).map_err(Into::into))?;
    Ok(true)
}

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn settings_set(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    ipc(state.set_settings(settings))
}
