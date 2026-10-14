//! Vault lifecycle commands (§6.1). Paths never leave Rust: the UI
//! picks folders through the native dialog and afterwards works
//! only with vault ids.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use soirvu_core::crypto::{calibrate_kdf, RecoveryKey};
use soirvu_core::Vault;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;
use zeroize::Zeroize;

use crate::error::ipc;
use crate::state::{AppState, VaultInfo};

#[derive(Serialize)]
pub struct Picked {
    /// Set for mode "open" (an existing vault was registered).
    /// Null for mode "create": the id exists only after vault_create.
    pub vault_id: Option<String>,
    pub name: String,
}

#[derive(Serialize)]
pub struct Created {
    pub vault_id: String,
    /// Shown once, for the user to write down. Never stored here.
    pub recovery_key: String,
}

async fn pick_folder(app: &AppHandle) -> Option<PathBuf> {
    let (tx, rx) = oneshot::channel();
    app.dialog().file().pick_folder(move |result| {
        let _ = tx.send(result);
    });
    match rx.await {
        Ok(Some(tauri_plugin_dialog::FilePath::Path(path))) => Some(path),
        _ => None,
    }
}

fn folder_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Vault".into())
}

#[tauri::command]
pub fn vault_list(state: State<'_, AppState>) -> Vec<VaultInfo> {
    state.vault_list()
}

#[tauri::command]
pub async fn vault_pick_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: String,
) -> Result<Option<Picked>, String> {
    let Some(path) = pick_folder(&app).await else {
        return Ok(None);
    };
    match mode.as_str() {
        "open" => {
            let info = ipc(Vault::header_info(&path).map_err(Into::into))?;
            let name = folder_name(&path);
            state
                .register(info.vault_id.clone(), name.clone(), path)
                .map_err(|e| e.to_ipc())?;
            Ok(Some(Picked { vault_id: Some(info.vault_id), name }))
        }
        "create" => {
            let name = folder_name(&path);
            state.set_pending_create(path);
            Ok(Some(Picked { vault_id: None, name }))
        }
        _ => Err("invalid_input".into()),
    }
}

#[tauri::command]
pub async fn vault_create(
    app: AppHandle,
    state: State<'_, AppState>,
    mut passphrase: String,
) -> Result<Created, String> {
    let result = (|| {
        let path = state.take_pending_create()?;
        let kdf = calibrate_kdf(Duration::from_millis(750));
        let (vault, recovery) = Vault::create(&path, &passphrase, kdf)?;
        let vault_id = vault.header().vault_id.clone();
        let name = folder_name(&path);
        state.register(vault_id.clone(), name, path)?;
        state.adopt_created(vault, &vault_id);
        Ok::<_, crate::error::AppError>(Created {
            vault_id,
            recovery_key: recovery.to_text(),
        })
    })();
    passphrase.zeroize();
    let created = ipc(result)?;
    super::emit_status(&app, &state);
    Ok(created)
}

#[tauri::command]
pub async fn vault_unlock(
    app: AppHandle,
    state: State<'_, AppState>,
    vault_id: String,
    kind: String,
    mut secret: String,
) -> Result<(), String> {
    let result = (|| {
        let path = state.vault_path(&vault_id)?;
        state.begin_unlock(&vault_id)?;
        let opened = match kind.as_str() {
            "passphrase" => Vault::open_with_passphrase(&path, &secret),
            "recovery" => {
                let key = RecoveryKey::from_text(&secret)?;
                Vault::open_with_recovery(&path, &key)
            }
            _ => return Err(crate::error::AppError::Code("invalid_input".into())),
        };
        match opened {
            Ok(vault) => {
                state.unlock_ok(vault);
                Ok(())
            }
            Err(e) => {
                state.unlock_failed();
                Err(e.into())
            }
        }
    })();
    secret.zeroize();
    ipc(result)?;
    super::emit_status(&app, &state);
    Ok(())
}

#[tauri::command]
pub fn vault_lock(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.lock();
    super::emit_status(&app, &state);
    Ok(())
}

#[tauri::command]
pub fn vault_forget(state: State<'_, AppState>, vault_id: String) -> Result<(), String> {
    ipc(state.forget(&vault_id))
}

#[tauri::command]
pub async fn vault_relocate(
    state: State<'_, AppState>,
    app: AppHandle,
    vault_id: String,
) -> Result<bool, String> {
    let Some(path) = pick_folder(&app).await else {
        return Ok(false);
    };
    let info = ipc(Vault::header_info(&path).map_err(Into::into))?;
    if info.vault_id != vault_id {
        return Err("vault_mismatch".into());
    }
    let name = folder_name(&path);
    state.register(vault_id, name, path).map_err(|e| e.to_ipc())?;
    Ok(true)
}
