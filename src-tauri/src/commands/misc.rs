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
        .title("SoirVu")
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
pub async fn settings_get(app: AppHandle) -> Result<Settings, String> {
    crate::commands::run_blocking_state(app, |state| Ok(state.settings())).await
}

#[tauri::command]
pub async fn settings_set(app: AppHandle, settings: Settings) -> Result<(), String> {
    crate::commands::run_blocking_state(app, move |state| ipc(state.set_settings(settings))).await
}

/// Picks the backup folder with the native dialog and stores
/// the PATH in Rust (app.json). The UI gets back the updated
/// settings, whose backup_location is a display name only —
/// the one settings value that cannot travel through
/// settings_set, because paths never cross the IPC boundary.
#[tauri::command]
pub async fn settings_pick_backup_dir(app: AppHandle) -> Result<Option<Settings>, String> {
    let Some(path) = crate::commands::vault::pick_folder(&app).await else {
        return Ok(None);
    };
    let settings = crate::commands::run_blocking_state(app, move |state| {
        ipc(state.set_backup_dir(Some(path)))?;
        Ok(state.settings())
    })
    .await?;
    Ok(Some(settings))
}

#[tauri::command]
pub async fn settings_clear_backup_dir(app: AppHandle) -> Result<Settings, String> {
    crate::commands::run_blocking_state(app, |state| {
        ipc(state.set_backup_dir(None))?;
        Ok(state.settings())
    })
    .await
}

/// §9 (optional, off by default): the UI reports every copy made
/// in the diary; 30 s later the clipboard is cleared, but only
/// if nothing else has been copied since. The clipboard's
/// content is never read — only its change counter.
#[tauri::command]
pub fn clipboard_note_copy(state: State<'_, AppState>) -> Result<(), String> {
    if !state.clipboard_clear() {
        return Ok(());
    }
    let Some(count) = platform::clipboard_change_count() else {
        return Ok(());
    };
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(30));
        platform::clipboard_clear_if_unchanged(count);
    });
    Ok(())
}

/// §13 Phase 6: copy the open vault to the backup location
/// now, whatever the schedule says. Returns the new folder's
/// display name; the path never crosses the boundary.
#[tauri::command]
pub async fn backup_now(app: AppHandle) -> Result<String, String> {
    crate::commands::run_blocking_state(app, |state| ipc(crate::backup::make_backup(state))).await
}
/// What an export wrote (§8): counts only, plus a display
/// name for the destination — paths never cross the IPC
/// boundary.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOutcome {
    pub entries: usize,
    pub attachments: usize,
    pub target: String,
}

/// The §8 warning, native so the webview cannot skip it.
/// True = the user chose to export plaintext anyway.
async fn confirm_export(app: &AppHandle) -> bool {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .message(
            "Export writes your diary as PLAIN files that anyone can read — \
             no passphrase protects them.\n\n\
             Keep the export somewhere private, and delete it when you no longer need it."
                .to_string(),
        )
        .title("SoirVu")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Export plaintext".into(),
            "Cancel".into(),
        ))
        .show(move |confirmed| {
            let _ = tx.send(confirmed);
        });
    rx.await.unwrap_or(false)
}

/// Shared export flow: warning, folder pick, then the work on
/// a blocking thread against a standalone handle — opening it
/// with the passphrase IS the §8 re-authentication.
async fn run_export(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: String,
    json: bool,
) -> Result<Option<ExportOutcome>, String> {
    use soirvu_core::Vault;
    use zeroize::Zeroizing;

    let Some(id) = state.current_id() else {
        return Err("locked".into());
    };
    let src = ipc(state.vault_path(&id))?;
    if !confirm_export(&app).await {
        return Ok(None);
    }
    let Some(dir) = crate::commands::vault::pick_folder(&app).await else {
        return Ok(None);
    };
    let pass = Zeroizing::new(passphrase);
    let joined =
        tauri::async_runtime::spawn_blocking(move || -> crate::error::AppResult<ExportOutcome> {
            let vault = Vault::open_with_passphrase(&src, &pass)?;
            if json {
                let file = dir.join("soirvu-export.json");
                if file.exists() {
                    return Err(crate::error::AppError::Code("export_target_exists".into()));
                }
                let report = vault.export_json(&file)?;
                let folder = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Ok(ExportOutcome {
                    entries: report.entries,
                    attachments: report.attachments,
                    target: format!("{folder}/soirvu-export.json"),
                })
            } else {
                let report = vault.export_markdown(&dir)?;
                Ok(ExportOutcome {
                    entries: report.entries,
                    attachments: report.attachments,
                    target: dir
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                })
            }
        })
        .await;
    let result = match joined {
        Ok(inner) => inner,
        Err(_) => Err(crate::error::AppError::Code("storage".into())),
    };
    Ok(Some(ipc(result)?))
}

/// §8: the whole diary as Markdown files with front matter,
/// attachments loose in `assets/`. Plaintext — see the
/// warning in `confirm_export`.
#[tauri::command]
pub async fn export_markdown(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: String,
) -> Result<Option<ExportOutcome>, String> {
    run_export(app, state, passphrase, false).await
}

/// §8: the whole diary as one lossless JSON file,
/// tombstones and ids included, made for re-import.
#[tauri::command]
pub async fn export_json(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: String,
) -> Result<Option<ExportOutcome>, String> {
    run_export(app, state, passphrase, true).await
}

/// What an import did (§8): counts only — paths never cross
/// the boundary here either.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub entries: usize,
    pub attachments: usize,
    pub skipped_entries: usize,
    pub skipped_attachments: usize,
}

impl From<soirvu_core::import::ImportReport> for ImportOutcome {
    fn from(report: soirvu_core::import::ImportReport) -> Self {
        ImportOutcome {
            entries: report.entries,
            attachments: report.attachments,
            skipped_entries: report.skipped_entries,
            skipped_attachments: report.skipped_attachments,
        }
    }
}

/// §8: import a folder of Markdown files (front matter
/// optional) into the open diary. Only adds — see the core
/// import module for the id rules.
#[tauri::command]
pub async fn import_markdown(app: AppHandle) -> Result<Option<ImportOutcome>, String> {
    let Some(dir) = crate::commands::vault::pick_folder(&app).await else {
        return Ok(None);
    };
    let report = crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| Ok(vault.import_markdown(&dir)?)))
    })
    .await?;
    Ok(Some(report.into()))
}

/// §8: import a SoirVu JSON export into the open diary.
/// Entries and attachments keep their ids; ids already taken
/// are skipped, never overwritten.
#[tauri::command]
pub async fn import_json(app: AppHandle) -> Result<Option<ImportOutcome>, String> {
    let Some(file) = crate::commands::vault::pick_file(&app).await else {
        return Ok(None);
    };
    let report = crate::commands::run_blocking_state(app, move |state| {
        ipc(state.with_vault(|vault| Ok(vault.import_json(&file)?)))
    })
    .await?;
    Ok(Some(report.into()))
}
