//! Vault lifecycle commands (§6.1). Paths never leave Rust: the UI
//! picks folders through the native dialog and afterwards works
//! only with vault ids.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use soirvu_core::crypto::{KdfParams, RecoveryKey};
use soirvu_core::vault::VaultHeader;
use soirvu_core::Vault;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;
use zeroize::Zeroize;

use crate::error::{ipc, AppError};
use crate::state::{AppState, VaultInfo};

#[derive(Serialize)]
pub struct Picked {
    /// Set for mode "open" (an existing vault was registered).
    /// Null for mode "create": the id exists only after vault_create.
    pub vault_id: Option<String>,
    pub name: String,
    /// The §3 filesystem warning for this folder, when the
    /// location looks unsuitable for SQLite ("cloud_sync" |
    /// "network_fs"). A warning only — never a refusal.
    pub warning: Option<String>,
}

#[derive(Serialize)]
pub struct Created {
    pub vault_id: String,
    /// Shown once, for the user to write down. Never stored here.
    pub recovery_key: String,
}

pub(crate) async fn pick_folder(app: &AppHandle) -> Option<PathBuf> {
    let (tx, rx) = oneshot::channel();
    app.dialog().file().pick_folder(move |result| {
        let _ = tx.send(result);
    });
    match rx.await {
        Ok(Some(tauri_plugin_dialog::FilePath::Path(path))) => Some(path),
        _ => None,
    }
}

/// The vault's id, read from its header file. The folder is not
/// opened and nothing is decrypted. The header is small (about a
/// kilobyte); the size cap and the regular-file check keep a picked
/// folder that points at something else (a device, a huge file)
/// from hanging or exhausting memory. A folder without a readable
/// header is not a vault.
fn read_vault_id(path: &std::path::Path) -> Result<String, AppError> {
    const MAX_HEADER: u64 = 64 * 1024;
    let not_a_vault = || AppError::Code("not_a_vault".into());
    let header_path = path.join("vault.header.json");
    let meta = std::fs::metadata(&header_path).map_err(|_| not_a_vault())?;
    if !meta.is_file() || meta.len() > MAX_HEADER {
        return Err(not_a_vault());
    }
    let bytes = std::fs::read(&header_path).map_err(|_| not_a_vault())?;
    let header = VaultHeader::from_json(&bytes)?;
    Ok(header.vault_id)
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
            let vault_id = read_vault_id(&path).map_err(|e| e.to_ipc())?;
            // The header is unauthenticated, so a copied folder claims
            // the same id. If that id is already registered at another
            // folder that still exists, refuse to repoint it silently;
            // moving a vault is what vault_relocate is for.
            if let Ok(old_path) = state.vault_path(&vault_id) {
                if old_path != path && old_path.join("vault.header.json").exists() {
                    return Err("vault_mismatch".into());
                }
            }
            let name = folder_name(&path);
            let warning = crate::platform::fs_warning(&path).map(str::to_string);
            state
                .register(vault_id.clone(), name.clone(), path)
                .map_err(|e| e.to_ipc())?;
            Ok(Some(Picked {
                vault_id: Some(vault_id),
                name,
                warning,
            }))
        }
        "create" => {
            let name = folder_name(&path);
            let warning = crate::platform::fs_warning(&path).map(str::to_string);
            state.set_pending_create(path);
            Ok(Some(Picked {
                vault_id: None,
                name,
                warning,
            }))
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
    let path = match state.take_pending_create() {
        Ok(path) => path,
        Err(e) => {
            passphrase.zeroize();
            return Err(e.to_ipc());
        }
    };
    let name = folder_name(&path);
    // Calibration plus creation is seconds of CPU (Argon2id): keep
    // it off the async runtime's threads.
    let task_path = path.clone();
    let mut task_passphrase = passphrase.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            let kdf = KdfParams::calibrate(Duration::from_millis(750))?;
            let pair = Vault::create(&task_path, &task_passphrase, kdf)?;
            Ok::<_, AppError>(pair)
        })();
        task_passphrase.zeroize();
        result
    })
    .await;
    passphrase.zeroize();
    let (vault, recovery) = match joined {
        Ok(Ok(pair)) => pair,
        Ok(Err(e)) => return Err(e.to_ipc()),
        Err(_) => return Err("internal".into()),
    };
    let vault_id = vault.header().vault_id.clone();
    ipc(state.register(vault_id.clone(), name, path))?;
    state.adopt_created(vault, &vault_id);
    let created = Created {
        vault_id,
        recovery_key: recovery.to_text(),
    };
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
    if kind != "passphrase" && kind != "recovery" {
        secret.zeroize();
        return Err("invalid_input".into());
    }
    let path = match state.vault_path(&vault_id) {
        Ok(path) => path,
        Err(e) => {
            secret.zeroize();
            return Err(e.to_ipc());
        }
    };
    if let Err(e) = state.begin_unlock(&vault_id) {
        secret.zeroize();
        return Err(e.to_ipc());
    }
    // Opening runs Argon2id (~750 ms): keep it off the async
    // runtime's threads.
    let mut task_secret = secret.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let opened = match kind.as_str() {
            "passphrase" => Vault::open_with_passphrase(&path, &task_secret),
            _ => RecoveryKey::from_text(&task_secret)
                .and_then(|key| Vault::open_with_recovery(&path, &key)),
        };
        task_secret.zeroize();
        opened
    })
    .await;
    secret.zeroize();
    match joined {
        Ok(Ok(vault)) => {
            state.unlock_ok(vault);
            super::emit_status(&app, &state);
            Ok(())
        }
        Ok(Err(e)) => {
            state.unlock_failed();
            Err(AppError::from(e).to_ipc())
        }
        Err(_) => {
            state.unlock_failed();
            Err("internal".into())
        }
    }
}

#[tauri::command]
pub fn vault_lock(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.lock();
    super::emit_status(&app, &state);
    // §9: reload the document onto the lock screen. Deferred to
    // the main thread's next turn so this command's reply is not
    // lost with the document it destroys.
    let reload_app = app.clone();
    let _ = app.run_on_main_thread(move || super::reload_main_window(&reload_app));
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
    let found_id = read_vault_id(&path).map_err(|e| e.to_ipc())?;
    if found_id != vault_id {
        return Err("vault_mismatch".into());
    }
    let name = folder_name(&path);
    state
        .register(vault_id, name, path)
        .map_err(|e| e.to_ipc())?;
    Ok(true)
}
