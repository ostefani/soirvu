//! Vault lifecycle commands (§6.1). Paths never leave Rust: the UI
//! picks folders through the native dialog and afterwards works
//! only with vault ids.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use soirvu_core::Vault;
use soirvu_core::crypto::{DeviceSecret, KdfParams, RecoveryKey};
use soirvu_core::vault::VaultHeader;

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::sync::oneshot;
use zeroize::Zeroize;

use crate::error::{AppError, ipc};
use crate::lock_handshake::{FlushOutcome, LOCK_ACK_WAIT};
use crate::state::{AppState, LockRequested, VaultInfo};

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
    /// Mode "open", import case: the copy's vault is already
    /// registered, so the copy is not registered yet — it waits
    /// as a pending fork until vault_fork_import gives it its
    /// own identity (or the user drops it).
    pub fork_required: bool,
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

pub(crate) async fn pick_file(app: &AppHandle) -> Option<PathBuf> {
    let (tx, rx) = oneshot::channel();
    app.dialog().file().pick_file(move |result| {
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

/// Copies a vault's two files into an empty folder — the
/// restore-as-import step. The source (a backup) is only read.
fn import_copy(from: &std::path::Path, to: &std::path::Path) -> Result<(), AppError> {
    // A folder holding only .DS_Store counts as empty: Finder
    // writes that file into every folder it shows, so a folder
    // the user emptied in Finder still has it.
    for entry in std::fs::read_dir(to).map_err(|_| AppError::Code("storage".into()))? {
        let entry = entry.map_err(|_| AppError::Code("storage".into()))?;
        if entry.file_name() != ".DS_Store" {
            return Err(AppError::Code("destination_not_empty".into()));
        }
    }
    for file in ["soirvu.db", "vault.header.json"] {
        std::fs::copy(from.join(file), to.join(file))
            .map_err(|_| AppError::Code("storage".into()))?;
    }
    Ok(())
}

/// Native explanation before a backup restore: a backup is an
/// archive, not a home, so restoring imports it into a folder
/// the user chooses next.
async fn confirm_import(app: &AppHandle, name: &str) -> bool {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .message(format!(
            "This is a backup of \"{name}\"\n\nTo restore it, choose an empty folder where the diary will live. The backup itself stays untouched."
        ))
        .title("Soirvu")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Choose folder\u{2026}".into(),
            "Cancel".into(),
        ))
        .show(move |confirmed| {
            let _ = tx.send(confirmed);
        });
    rx.await.unwrap_or(false)
}

/// The folder that actually holds the vault: the picked folder
/// itself, or — when a backup's per-vault folder was picked —
/// its newest backup inside it. Backups nest one level (the
/// timestamp folder is the vault copy); a folder with no header
/// of its own and no backup inside is not a vault.
fn resolve_vault_folder(path: &std::path::Path) -> std::path::PathBuf {
    if path.join("vault.header.json").exists() {
        return path.to_path_buf();
    }
    let mut newest: Option<std::path::PathBuf> = None;
    if let Ok(read) = std::fs::read_dir(path) {
        for entry in read.flatten() {
            let sub = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if sub.is_dir()
                && crate::backup::is_stamp(&name)
                && sub.join("vault.header.json").exists()
                && newest.as_ref().is_none_or(|n| sub > *n)
            {
                newest = Some(sub);
            }
        }
    }
    newest.unwrap_or_else(|| path.to_path_buf())
}

/// Native confirmation for opening a copy of a registered
/// vault (a restored backup, most often): the registration is
/// repointed only if the user says so.
async fn confirm_open_copy(app: &AppHandle, name: &str) -> bool {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .message(format!(
            "This folder is a copy of \"{name}\"\n\nOpen the copy? Soirvu will use this folder for \"{name}\" from now on and assign it a new sync identity. The other folder is left untouched."
        ))
        .title("Soirvu")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Open the copy".into(),
            "Cancel".into(),
        ))
        .show(move |confirmed| {
            let _ = tx.send(confirmed);
        });
    rx.await.unwrap_or(false)
}

fn folder_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Vault".into())
}

#[tauri::command]
pub async fn vault_list(app: AppHandle) -> Result<Vec<VaultInfo>, String> {
    crate::commands::run_blocking_state(app, |state| Ok(state.vault_list())).await
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
            let picked_path = path.clone();
            let (resolved, vault_id) = crate::commands::run_blocking(move || {
                let resolved = resolve_vault_folder(&picked_path);
                let vault_id = read_vault_id(&resolved).map_err(|e| e.to_ipc())?;
                Ok((resolved, vault_id))
            })
            .await?;
            // A backup is an archive, not a home: restoring
            // imports it into a folder the user chooses, and
            // the backup folder itself is never registered.
            let in_backup_root = state
                .backup_dir()
                .is_some_and(|root| resolved.starts_with(&root));
            let is_backup = resolved != path
                || crate::backup::is_stamp(&folder_name(&resolved))
                || in_backup_root;
            if is_backup {
                let vaults =
                    crate::commands::run_blocking_state(
                        app.clone(),
                        |state| Ok(state.vault_list()),
                    )
                    .await?;
                let name = vaults
                    .into_iter()
                    .find(|v| v.id == vault_id)
                    .map(|v| v.name)
                    .unwrap_or_else(|| folder_name(&path));
                if !confirm_import(&app, &name).await {
                    return Ok(None);
                }
                let Some(dest) = pick_folder(&app).await else {
                    return Ok(None);
                };
                if dest == resolved {
                    return Err("invalid_input".into());
                }
                let copy_from = resolved.clone();
                let copy_to = dest.clone();
                crate::commands::run_blocking(move || {
                    import_copy(&copy_from, &copy_to).map_err(|e| e.to_ipc())
                })
                .await?;
                let name = folder_name(&dest);
                let warning = crate::platform::fs_warning(&dest).map(str::to_string);
                if state.vault_path(&vault_id).is_ok() {
                    // The original is registered: the copy must
                    // not take its place. It waits for a fork.
                    state.set_pending_fork(dest);
                    return Ok(Some(Picked {
                        vault_id: Some(vault_id),
                        name,
                        warning,
                        fork_required: true,
                    }));
                }
                let register_id = vault_id.clone();
                let register_name = name.clone();
                crate::commands::run_blocking_state(app.clone(), move |state| {
                    state
                        .register(register_id, register_name, dest)
                        .map(|_| ())
                        .map_err(|e| e.to_ipc())
                })
                .await?;
                return Ok(Some(Picked {
                    vault_id: Some(vault_id),
                    name,
                    warning,
                    fork_required: false,
                }));
            }
            // The header is unauthenticated, so a copied folder
            // claims the same id. If that id is already registered
            // at another folder that still exists, this is a copy
            // (a restored backup, most often): never repoint
            // silently — ask first, natively.
            let mut opening_copy = false;
            let vault_id_for_lookup = vault_id.clone();
            let selected_path = path.clone();
            let old_path = crate::commands::run_blocking_state(app.clone(), move |state| {
                let old_path = state.vault_path(&vault_id_for_lookup).ok();
                let is_existing_copy = old_path.as_ref().is_some_and(|old_path| {
                    old_path != &selected_path && old_path.join("vault.header.json").exists()
                });
                Ok((old_path, is_existing_copy))
            })
            .await?;
            if let (Some(old_path), true) = old_path {
                let vaults =
                    crate::commands::run_blocking_state(
                        app.clone(),
                        |state| Ok(state.vault_list()),
                    )
                    .await?;
                let name = vaults
                    .into_iter()
                    .find(|v| v.id == vault_id)
                    .map(|v| v.name)
                    .unwrap_or_else(|| folder_name(&old_path));
                if !confirm_open_copy(&app, &name).await {
                    return Ok(None);
                }
                opening_copy = true;
            }
            let name = folder_name(&path);
            let warning = crate::platform::fs_warning(&path).map(str::to_string);
            if opening_copy {
                // The selected folder is a second copy of the same
                // encrypted vault. Keep its vault id, but stop it
                // sharing the original copy's sync stream identity.
                let register_id = vault_id.clone();
                let register_name = name.clone();
                crate::commands::run_blocking_state(app.clone(), move |state| {
                    state
                        .register_with_device(
                            register_id,
                            register_name,
                            path,
                            Some(soirvu_core::new_device_id()),
                        )
                        .map(|_| ())
                        .map_err(|e| e.to_ipc())
                })
                .await?;
            } else {
                let register_id = vault_id.clone();
                let register_name = name.clone();
                crate::commands::run_blocking_state(app.clone(), move |state| {
                    state
                        .register(register_id, register_name, path)
                        .map(|_| ())
                        .map_err(|e| e.to_ipc())
                })
                .await?;
            }
            Ok(Some(Picked {
                vault_id: Some(vault_id),
                name,
                warning,
                fork_required: false,
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
                fork_required: false,
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
    // The device identity is assigned at birth: minted
    // here, kept in the registry (below), and handed
    // to the core — it never lives in the vault folder.
    let device_id = soirvu_core::new_device_id();
    // Calibration plus creation is seconds of CPU (Argon2id): keep
    // it off the async runtime's threads.
    let task_path = path.clone();
    let task_device = device_id.clone();
    let mut task_passphrase = passphrase.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            let kdf = KdfParams::calibrate(Duration::from_millis(750))?;
            let pair = Vault::create_with_device(&task_path, &task_passphrase, kdf, &task_device)?;
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
    ipc(state.register_with_device(vault_id.clone(), name, path, Some(device_id)))?;
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
    let assignment = match state.assignment(&vault_id) {
        Ok(a) => a,
        Err(e) => {
            secret.zeroize();
            return Err(e.to_ipc());
        }
    };
    // Opening runs Argon2id (~750 ms): keep it off the async
    // runtime's threads.
    let mut task_secret = secret.clone();
    let task_assignment = assignment.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let opened = match (kind.as_str(), task_assignment.as_deref()) {
            ("passphrase", Some(device)) => {
                Vault::open_with_passphrase_as(&path, &task_secret, device)
            }
            (_, Some(device)) => RecoveryKey::from_text(&task_secret)
                .and_then(|key| Vault::open_with_recovery_as(&path, &key, device)),
            ("passphrase", None) => Vault::open_with_passphrase(&path, &task_secret),
            (_, None) => RecoveryKey::from_text(&task_secret)
                .and_then(|key| Vault::open_with_recovery(&path, &key)),
        };
        task_secret.zeroize();
        opened
    })
    .await;
    secret.zeroize();
    match joined {
        Ok(Ok(vault)) => {
            if assignment.is_none() {
                // A pre-step-3 entry: adopt the identity
                // this vault already syncs under.
                // Best effort — the vault is open either
                // way, and a stored open yields the same
                // id next time, so a failed write just
                // retries then.
                let _ = state.set_device_id(&vault_id, vault.device_id().to_string());
            }
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

/// Ask the UI to flush and wait for its verdict (§6.2). Never
/// blocks an async runtime thread: call from a blocking thread.
pub fn begin_lock_handshake(app: &AppHandle, cap: Duration) -> FlushOutcome {
    let state = app.state::<AppState>();
    let generation = match state.begin_lock() {
        Some(generation) => {
            let _ = app.emit("lock-requested", LockRequested { generation });
            Some(generation)
        }
        None => state.handshake.pending(),
    };
    match generation {
        Some(generation) => state.handshake.wait(generation, cap),
        None => FlushOutcome::Ok,
    }
}

/// Idle and OS-signal locks: never vetoed. A failed or unanswered flush locks
/// anyway and leaves a content-free notice for the lock screen.
/// `cap = None` skips the wait. Returns true if this call locked.
pub fn lock_forced(app: &AppHandle, cap: Option<Duration>) -> bool {
    let outcome = cap.map_or(FlushOutcome::Ok, |cap| begin_lock_handshake(app, cap));
    let state = app.state::<AppState>();
    if matches!(outcome, FlushOutcome::Failed | FlushOutcome::TimedOut) {
        state.handshake.flag_notice();
    }
    if matches!(state.lock_once(), Ok(true)) {
        crate::commands::emit_status(app, &state);
        crate::commands::reload_main_window(app);
        true
    } else {
        false
    }
}

#[tauri::command]
pub async fn vault_lock(app: AppHandle) -> Result<(), String> {
    // §6.2 handshake first: the UI flushes while the keys live.
    // The wait runs on a blocking thread. The lock itself is
    // forced: a failed or unanswered flush is reported on the lock screen
    // instead of refusing the user's explicit lock.
    let handshake_app = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        begin_lock_handshake(&handshake_app, LOCK_ACK_WAIT)
    })
    .await
    .unwrap_or(FlushOutcome::Ok);
    if matches!(outcome, FlushOutcome::Failed | FlushOutcome::TimedOut) {
        app.state::<AppState>().handshake.flag_notice();
    }
    let lock_result =
        crate::commands::run_blocking_state(app.clone(), |state| ipc(state.lock_once())).await;
    let state = app.state::<AppState>();
    // Another trigger may have locked first; it already emitted
    // and reloaded, so only the winning call does it again.
    let already_locked = lock_result == Ok(false);
    if !already_locked {
        super::emit_status(&app, &state);
        // §9: reload the document onto the lock screen. Deferred to
        // the main thread's next turn so this command's reply is not
        // lost with the document it destroys.
        let reload_app = app.clone();
        let _ = app.run_on_main_thread(move || super::reload_main_window(&reload_app));
    }
    lock_result.map(|_| ())
}

/// Acknowledge the lock handshake (§6.2): the UI flushed (or
/// has nothing to flush) for `generation`. Returns true only
/// when the ack matches the pending handshake.
#[tauri::command]
pub fn vault_lock_ready(state: State<'_, AppState>, generation: u64, ok: bool) -> bool {
    state.handshake.acknowledge(generation, ok)
}

/// True once if the last forced lock dropped unsaved edits.
#[tauri::command]
pub fn lock_notice_take(state: State<'_, AppState>) -> bool {
    state.handshake.take_notice()
}

#[tauri::command]
pub async fn vault_forget(app: AppHandle, vault_id: String) -> Result<(), String> {
    crate::commands::run_blocking_state(app, move |state| ipc(state.forget(&vault_id))).await
}

#[tauri::command]
pub async fn vault_relocate(app: AppHandle, vault_id: String) -> Result<bool, String> {
    let Some(path) = pick_folder(&app).await else {
        return Ok(false);
    };
    let check_path = path.clone();
    let found_id = crate::commands::run_blocking_state(app.clone(), move |_| {
        read_vault_id(&check_path).map_err(|e| e.to_ipc())
    })
    .await?;
    if found_id != vault_id {
        return Err("vault_mismatch".into());
    }
    let name = folder_name(&path);
    crate::commands::run_blocking_state(app, move |state| {
        state
            .register(vault_id, name, path)
            .map(|_| ())
            .map_err(|e| e.to_ipc())
    })
    .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_in(dir: &std::path::Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("vault.header.json"), b"{}").unwrap();
    }

    #[test]
    fn import_copies_into_an_empty_folder_only() {
        let tmp = tempfile::TempDir::new().unwrap();
        let from = tmp.path().join("backup");
        let to = tmp.path().join("home");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(from.join("soirvu.db"), b"db").unwrap();
        std::fs::write(from.join("vault.header.json"), b"{}").unwrap();
        import_copy(&from, &to).unwrap();
        assert_eq!(std::fs::read(to.join("soirvu.db")).unwrap(), b"db");
        // And a non-empty destination is refused, untouched.
        let err = import_copy(&from, &to).unwrap_err();
        assert_eq!(err.to_ipc(), "destination_not_empty");

        // But a folder whose only content is Finder's .DS_Store
        // counts as empty.
        let to2 = tmp.path().join("home2");
        std::fs::create_dir_all(&to2).unwrap();
        std::fs::write(to2.join(".DS_Store"), b"finder").unwrap();
        import_copy(&from, &to2).unwrap();
        assert!(to2.join("soirvu.db").is_file());
    }

    #[test]
    fn resolve_prefers_the_folder_itself() {
        let tmp = tempfile::TempDir::new().unwrap();
        header_in(tmp.path());
        assert_eq!(resolve_vault_folder(tmp.path()), tmp.path());
    }

    #[test]
    fn resolve_enters_a_backup_folder_and_takes_the_newest() {
        let tmp = tempfile::TempDir::new().unwrap();
        header_in(&tmp.path().join("2026-10-14_020000"));
        header_in(&tmp.path().join("2026-10-15_032607"));
        std::fs::create_dir(tmp.path().join("2026-10-16_000000")).unwrap(); // no header: skip
        assert_eq!(
            resolve_vault_folder(tmp.path()),
            tmp.path().join("2026-10-15_032607")
        );
    }

    #[test]
    fn resolve_leaves_other_folders_alone() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("random")).unwrap();
        header_in(&tmp.path().join("random")); // not a backup stamp
        assert_eq!(resolve_vault_folder(tmp.path()), tmp.path());
    }
}

/// What a finished fork hands back: the copy's new identity
/// and its NEW recovery key (shown once, like at creation).
/// Touch ID at the lock screen and in Settings (Phase 8):
/// `built_in` says the binary was compiled with the
/// touch-id feature at all; `capable` adds this build's
/// signature (the keychain entitlement — spike 6);
/// `enrolled` is a property of the vault (its header
/// carries a device slot). The UI shows the Touch ID button
/// only when all three are true, and explains the specific
/// gap otherwise.
#[derive(Serialize)]
pub struct TouchIdStatus {
    pub built_in: bool,
    pub capable: bool,
    pub enrolled: bool,
}

#[tauri::command]
pub async fn touch_id_status(
    app: AppHandle,
    vault_id: Option<String>,
) -> Result<TouchIdStatus, String> {
    crate::commands::run_blocking_state(app, move |state| {
        let built_in = cfg!(feature = "touch-id");
        let capable = crate::keychain::capable();
        let enrolled = match &vault_id {
            // The lock screen asks about the selected vault: its
            // header file says whether a device slot exists.
            Some(id) => state
                .vault_path(id)
                .ok()
                .and_then(|path| std::fs::read(path.join("vault.header.json")).ok())
                .and_then(|bytes| VaultHeader::from_json(&bytes).ok())
                .map(|header| header.has_device_slot())
                .unwrap_or(false),
            // Settings asks about the open vault.
            None => state
                .with_vault(|vault| Ok(vault.has_device_slot()))
                .unwrap_or(false),
        };
        Ok(TouchIdStatus {
            built_in,
            capable,
            enrolled,
        })
    })
    .await
}

enum TouchUnlockError {
    Keychain(crate::keychain::KeychainError),
    Core(soirvu_core::Error),
}

/// Unlocks with Touch ID (Phase 8): the keychain read runs
/// on a blocking thread because the fingerprint prompt waits
/// there. A dismissed prompt aborts the unlock without
/// feeding the §6.2 backoff.
#[tauri::command]
pub async fn vault_unlock_touch_id(
    app: AppHandle,
    state: State<'_, AppState>,
    vault_id: String,
) -> Result<(), String> {
    let path = match state.vault_path(&vault_id) {
        Ok(path) => path,
        Err(e) => return Err(e.to_ipc()),
    };
    if let Err(e) = state.begin_unlock(&vault_id) {
        return Err(e.to_ipc());
    }
    let assignment = match state.assignment(&vault_id) {
        Ok(a) => a,
        Err(e) => return Err(e.to_ipc()),
    };
    let task_assignment = assignment.clone();
    let keychain_id = vault_id.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let bytes = crate::keychain::read(&keychain_id).map_err(TouchUnlockError::Keychain)?;
        let secret = DeviceSecret::from_bytes(&bytes).map_err(TouchUnlockError::Core)?;
        match task_assignment.as_deref() {
            Some(device) => {
                Vault::open_with_device_as(&path, &secret, device).map_err(TouchUnlockError::Core)
            }
            None => Vault::open_with_device(&path, &secret).map_err(TouchUnlockError::Core),
        }
    })
    .await;
    match joined {
        Ok(Ok(vault)) => {
            if assignment.is_none() {
                // Adopt, as in vault_unlock.
                let _ = state.set_device_id(&vault_id, vault.device_id().to_string());
            }
            state.unlock_ok(vault);
            super::emit_status(&app, &state);
            Ok(())
        }
        Ok(Err(TouchUnlockError::Keychain(crate::keychain::KeychainError::Cancelled))) => {
            state.unlock_aborted();
            Err("touch_id_cancelled".into())
        }
        Ok(Err(TouchUnlockError::Keychain(crate::keychain::KeychainError::NotFound))) => {
            // A missing item (never stored, or the keychain
            // was reset) is not a wrong guess either: no
            // backoff, and the passphrase path is right there.
            state.unlock_aborted();
            Err("touch_id_unavailable".into())
        }
        Ok(Err(TouchUnlockError::Keychain(e))) => {
            state.unlock_failed();
            Err(match e {
                crate::keychain::KeychainError::Unavailable => "touch_id_unavailable".into(),
                _ => "storage".into(),
            })
        }
        Ok(Err(TouchUnlockError::Core(e))) => {
            state.unlock_failed();
            Err(AppError::from(e).to_ipc())
        }
        Err(_) => {
            state.unlock_failed();
            Err("storage".into())
        }
    }
}

/// Enrolls the open vault for Touch ID: the core mints the
/// device secret and re-wraps the header, then the secret
/// goes into the keychain. If the keychain store fails, the
/// header slot is removed again — a slot without its secret
/// would only pretend to work.
#[tauri::command]
pub async fn touch_id_enroll(app: AppHandle) -> Result<(), String> {
    crate::commands::run_blocking_state(app, |state| {
        ipc(state.with_vault(|vault| {
            if !crate::keychain::capable() {
                return Err(AppError::Code("touch_id_unavailable".into()));
            }
            let vault_id = vault.header().vault_id.clone();
            let secret = vault.enroll_device()?;
            if crate::keychain::store(&vault_id, secret.as_bytes()).is_err() {
                let _ = vault.remove_device();
                return Err(AppError::Code("touch_id_unavailable".into()));
            }
            Ok(())
        }))
    })
    .await
}

/// Removes the open vault's Touch ID enrollment: the header
/// slot goes, and the keychain item with it.
#[tauri::command]
pub async fn touch_id_disable(app: AppHandle) -> Result<(), String> {
    crate::commands::run_blocking_state(app, |state| {
        ipc(state.with_vault(|vault| {
            let vault_id = vault.header().vault_id.clone();
            vault.remove_device()?;
            let _ = crate::keychain::delete(&vault_id);
            Ok(())
        }))
    })
    .await
}

#[derive(Serialize)]
pub struct Forked {
    pub vault_id: String,
    pub recovery_key: String,
}

/// Gives an imported backup copy its own identity (§13
/// Phase 6): opens the pending copy with the passphrase —
/// which is also the proof it is the diary's owner asking —
/// forks its identity in the core, and registers it under the
/// new id, next to the original. A wrong passphrase changes
/// nothing; the pending copy stays pending.
#[tauri::command]
pub async fn vault_fork_import(app: AppHandle, mut passphrase: String) -> Result<Forked, String> {
    crate::commands::run_blocking_state(app, move |state| {
        let result = (|| -> Result<Forked, AppError> {
            let dest = state
                .pending_fork()
                .ok_or_else(|| AppError::Code("invalid_input".into()))?;
            let mut vault = Vault::open_with_passphrase(&dest, &passphrase)?;
            let recovery = vault.fork_identity(&passphrase)?;
            let new_id = vault.header().vault_id.clone();
            drop(vault);
            state.clear_pending_fork();
            state.register(new_id.clone(), folder_name(&dest), dest)?;
            Ok(Forked {
                vault_id: new_id,
                recovery_key: recovery.to_text(),
            })
        })();
        passphrase.zeroize();
        ipc(result)
    })
    .await
}

/// Changes the open vault's passphrase (§13 Phase 6). The
/// core verifies the current one; the KDF for the new slot is
/// calibrated like at creation.
#[tauri::command]
pub async fn vault_change_passphrase(
    app: AppHandle,
    current_passphrase: String,
    new_passphrase: String,
) -> Result<(), String> {
    crate::commands::run_blocking_state(app, move |state| {
        let mut current = current_passphrase;
        let mut new = new_passphrase;
        let result = state.with_vault(|vault| {
            let kdf = KdfParams::calibrate(Duration::from_millis(750))?;
            vault.change_passphrase(&current, &new, kdf)?;
            Ok(())
        });
        current.zeroize();
        new.zeroize();
        ipc(result)
    })
    .await
}

/// Replaces the open vault's recovery key with a fresh one;
/// returns its text — the UI shows it exactly once.
#[tauri::command]
pub async fn vault_regenerate_recovery(
    app: AppHandle,
    current_passphrase: String,
) -> Result<String, String> {
    crate::commands::run_blocking_state(app, move |state| {
        let mut current = current_passphrase;
        let result = state.with_vault(|vault| {
            let recovery = vault.regenerate_recovery(&current)?;
            Ok(recovery.to_text())
        });
        current.zeroize();
        ipc(result)
    })
    .await
}

/// The rotation's folder arithmetic: (source, staging, old).
/// The staging and old folders are siblings — same volume,
/// so the swaps are renames, never copies.
fn rotation_paths(
    state: &AppState,
) -> Result<(std::path::PathBuf, std::path::PathBuf, std::path::PathBuf), AppError> {
    let id = state
        .current_id()
        .ok_or_else(|| AppError::Code("locked".into()))?;
    let src = state.vault_path(&id)?;
    let parent = src
        .parent()
        .ok_or_else(|| AppError::Code("storage".into()))?
        .to_path_buf();
    let folder = src
        .file_name()
        .ok_or_else(|| AppError::Code("storage".into()))?
        .to_string_lossy()
        .into_owned();
    Ok((
        src.clone(),
        parent.join(format!(".{folder}.rotating")),
        parent.join(format!(".{folder}.pre-rotation")),
    ))
}

/// Native confirmation before rotation: it replaces every
/// key this diary has, and old backups keep the old master —
/// the user hears both before anything moves.
async fn confirm_rotate(app: &AppHandle) -> bool {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .message(format!("Change the master key?\n\nSoirvu builds a fresh copy of this diary under a new master key, checks it, and replaces the old one. Your passphrase stays the same and you will get a NEW recovery key.\n\nBackups made earlier keep the old master key — delete them if the old key may have leaked."))
        .title("Soirvu")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Change master key".into(),
            "Cancel".into(),
        ))
        .show(move |confirmed| {
            let _ = tx.send(confirmed);
        });
    rx.await.unwrap_or(false)
}

/// Changes the open vault's master key by fresh-vault
/// migration (§9, §13 Phase 6). Returns the NEW recovery key
/// text (shown once), or None when the user cancels. The
/// vault ends LOCKED but silently — no status event, no
/// reload — so the Settings screen can show the key; its
/// Done button calls vault_lock, which locks "again" (a
/// no-op inside), emits and reloads as usual.
#[tauri::command]
pub async fn vault_rotate_master_key(
    app: AppHandle,
    current_passphrase: String,
) -> Result<Option<String>, String> {
    if !confirm_rotate(&app).await {
        return Ok(None);
    }
    // The decision is made and the work starts now — the UI
    // shows its progress indicator on this event, never
    // before (a spinner behind the confirmation dialog would
    // claim work that does not exist yet).
    use tauri::Emitter;
    let _ = app.emit("rotation-started", ());
    let passphrase = zeroize::Zeroizing::new(current_passphrase);
    let recovery_text = crate::commands::run_blocking_state(app, move |state| {
        let paths = rotation_paths(state).map_err(|e| e.to_ipc())?;
        if paths.1.exists() {
            let _ = std::fs::remove_dir_all(&paths.1);
        }
        let (src, staging, old) = paths;
        state
            .with_vault_exclusive_then_lock(
                &src,
                |vault| {
                    let kdf = KdfParams::calibrate(Duration::from_millis(750))?;
                    let recovery = vault.rotate_master_key(&staging, &passphrase, kdf)?;
                    Ok(recovery.to_text())
                },
                || {
                    if old.exists() {
                        let _ = std::fs::remove_dir_all(&old);
                    }
                    crate::platform::rename_retry(&src, &old)
                        .map_err(|_| AppError::Code("storage".into()))?;
                    if crate::platform::rename_retry(&staging, &src).is_err() {
                        let _ = crate::platform::rename_retry(&old, &src);
                        return Err(AppError::Code("storage".into()));
                    }
                    let _ = std::fs::remove_dir_all(&old);
                    Ok(())
                },
            )
            .map_err(|e| e.to_ipc())
    })
    .await?;
    Ok(Some(recovery_text))
}
