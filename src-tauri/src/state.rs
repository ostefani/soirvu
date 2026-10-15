//! App state and the lock state machine (DESIGN §6.2), plus the
//! app.json registry and settings (§3).
//!
//! The state machine lives here and only here:
//!
//!   Unavailable ⇄ Locked ──unlock──► Unlocking ──ok──► Unlocked
//!        ▲                            └─fail (backoff)─► Locked
//!        └──────── Locking ◄── (idle | manual | volume loss) ┘
//!
//! Every command that touches the vault goes through `with_vault`,
//! the one guard: unless the state is Unlocked it returns the
//! `locked` error code. Commands never check the state themselves.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use soirvu_core::Vault;

use crate::error::{AppError, AppResult};

/// Lock status, sent to the UI as part of every vault's info.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// No vault is selected, or the selected one is not there.
    Unavailable,
    Locked,
    Unlocking,
    Unlocked,
    Locking,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultRef {
    pub id: String,
    pub name: String,
    /// The path stays in Rust (§6.1): this struct is persisted to
    /// the app's own app.json and never crosses the IPC boundary —
    /// `VaultInfo` below is what the UI gets.
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Caps {
    pub image_mb: u64,
    pub pdf_mb: u64,
    pub audio_mb: u64,
    pub video_mb: u64,
}

impl Default for Caps {
    fn default() -> Self {
        // §6.4 initial defaults.
        Caps { image_mb: 25, pdf_mb: 100, audio_mb: 200, video_mb: 1024 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub lock_timeout_secs: u64,
    pub strip_exif: bool,
    pub caps: Caps,
    /// Display name of the backup folder (never a path — paths
    /// stay in Rust). Derived from AppConfig::backup_dir whenever
    /// settings are read; the value sent back by settings_set is
    /// ignored. Null until a folder is picked.
    #[serde(default)]
    pub backup_location: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { lock_timeout_secs: 300, strip_exif: true, caps: Caps::default(), backup_location: None }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub vaults: Vec<VaultRef>,
    pub last_opened: Option<String>,
    #[serde(default)]
    pub settings: Settings,
    /// The backup folder the user chose (Phase 3 setting; the
    /// scheduled backups that use it arrive in Phase 6). A path,
    /// so it lives here and never crosses the IPC boundary.
    #[serde(default)]
    pub backup_dir: Option<PathBuf>,
}

/// What the UI is allowed to know about a vault: an id, a name, and
/// a state. Never a path.
#[derive(Debug, Clone, Serialize)]
pub struct VaultInfo {
    pub id: String,
    pub name: String,
    /// "available" | "volume_missing" | "not_found" | "unlocked"
    pub state: String,
}

struct Inner {
    status: Status,
    vault: Option<Vault>,
    current: Option<String>,
    config: AppConfig,
    last_activity: Instant,
    unlock_failures: u32,
    backoff_until: Option<Instant>,
    /// Folder picked for a create that has not happened yet.
    pending_create: Option<PathBuf>,
    /// In-flight attachment uploads (§6.1): chunks accumulate here
    /// until attachment_finish commits them. Plaintext lives in
    /// process memory only — never on disk — and is dropped on lock.
    uploads: std::collections::HashMap<String, Upload>,
    upload_seq: u64,
}

/// One in-flight attachment upload.
pub struct Upload {
    pub entry_id: String,
    pub name: String,
    pub bytes: Vec<u8>,
}

pub struct AppState {
    inner: Mutex<Inner>,
    config_file: PathBuf,
}

impl AppState {
    /// Loads (or starts) app.json in the app config folder (§3).
    pub fn load(app_config_dir: PathBuf) -> AppState {
        let config_file = app_config_dir.join("app.json");
        let config = fs::read_to_string(&config_file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let state = AppState {
            inner: Mutex::new(Inner {
                status: Status::Unavailable,
                vault: None,
                current: None,
                config,
                last_activity: Instant::now(),
                unlock_failures: 0,
                backoff_until: None,
                pending_create: None,
                uploads: std::collections::HashMap::new(),
                upload_seq: 0,
            }),
            config_file,
        };
        // Copies left by a crashed run must not outlive it.
        state.clear_opened();
        state
    }

    fn save_config(&self, inner: &Inner) -> AppResult<()> {
        if let Some(parent) = self.config_file.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&inner.config)?;
        fs::write(&self.config_file, text)?;
        Ok(())
    }

    pub fn status(&self) -> Status {
        self.inner.lock().unwrap().status
    }

    pub fn current_id(&self) -> Option<String> {
        self.inner.lock().unwrap().current.clone()
    }

    pub fn settings(&self) -> Settings {
        let inner = self.inner.lock().unwrap();
        let mut settings = inner.config.settings.clone();
        settings.backup_location = inner
            .config
            .backup_dir
            .as_deref()
            .map(folder_display_name);
        settings
    }

    pub fn set_settings(&self, settings: Settings) -> AppResult<()> {
        let mut inner = self.inner.lock().unwrap();
        // backup_location in the DTO is read-only (see Settings):
        // the stored path never comes from the webview.
        inner.config.settings = settings;
        self.save_config(&inner)
    }

    /// Stores (or clears) the backup folder chosen through the
    /// native picker. Only Rust ever sees the path.
    pub fn set_backup_dir(&self, dir: Option<PathBuf>) -> AppResult<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.config.backup_dir = dir;
        self.save_config(&inner)
    }

    /// The vault list for the UI, with a state per vault.
    pub fn vault_list(&self) -> Vec<VaultInfo> {
        let inner = self.inner.lock().unwrap();
        inner
            .config
            .vaults
            .iter()
            .map(|v| VaultInfo {
                id: v.id.clone(),
                name: v.name.clone(),
                state: if Some(&v.id) == inner.current.as_ref()
                    && inner.status == Status::Unlocked
                {
                    "unlocked".into()
                } else {
                    availability_of(&v.path).into()
                },
            })
            .collect()
    }

    /// Registers a vault folder (after its header was read, or right
    /// after creation). Returns the registry entry.
    pub fn register(&self, id: String, name: String, path: PathBuf) -> AppResult<VaultRef> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(existing) = inner.config.vaults.iter_mut().find(|v| v.id == id) {
            existing.path = path;
            existing.name = name;
            let entry = existing.clone();
            self.save_config(&inner)?;
            return Ok(entry);
        }
        let entry = VaultRef { id, name, path };
        inner.config.vaults.push(entry.clone());
        self.save_config(&inner)?;
        Ok(entry)
    }

    pub fn forget(&self, vault_id: &str) -> AppResult<()> {
        let mut inner = self.inner.lock().unwrap();
        if inner.current.as_deref() == Some(vault_id) && inner.vault.is_some() {
            return Err(AppError::Other("vault is open — lock it first".into()));
        }
        inner.config.vaults.retain(|v| v.id != vault_id);
        if inner.current.as_deref() == Some(vault_id) {
            inner.current = None;
            inner.status = Status::Unavailable;
        }
        self.save_config(&inner)
    }

    pub fn vault_path(&self, vault_id: &str) -> AppResult<PathBuf> {
        let inner = self.inner.lock().unwrap();
        inner
            .config
            .vaults
            .iter()
            .find(|v| v.id == vault_id)
            .map(|v| v.path.clone())
            .ok_or_else(|| AppError::Code("vault_not_found".into()))
    }

    pub fn set_pending_create(&self, path: PathBuf) {
        self.inner.lock().unwrap().pending_create = Some(path);
    }

    pub fn take_pending_create(&self) -> AppResult<PathBuf> {
        self.inner
            .lock()
            .unwrap()
            .pending_create
            .take()
            .ok_or_else(|| AppError::Code("no_folder_picked".into()))
    }

    /// Locked ──► Unlocking. Enforces the failure backoff (§6.2):
    /// after each failed unlock the wait doubles, capped at 30 s.
    pub fn begin_unlock(&self, vault_id: &str) -> AppResult<()> {
        let mut inner = self.inner.lock().unwrap();
        if let Some(until) = inner.backoff_until {
            if Instant::now() < until {
                let secs = (until - Instant::now()).as_secs() + 1;
                return Err(AppError::Backoff(secs));
            }
        }
        inner.current = Some(vault_id.to_string());
        inner.status = Status::Unlocking;
        Ok(())
    }

    pub fn unlock_ok(&self, vault: Vault) {
        let mut inner = self.inner.lock().unwrap();
        inner.vault = Some(vault);
        inner.status = Status::Unlocked;
        inner.last_activity = Instant::now();
        inner.unlock_failures = 0;
        inner.backoff_until = None;
        if let Some(id) = &inner.current {
            inner.config.last_opened = Some(id.clone());
        }
        let _ = self.save_config(&inner);
    }

    /// A vault created in this session starts Unlocked: its keys
    /// were just derived here, there is nothing to re-ask.
    pub fn adopt_created(&self, vault: Vault, vault_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.current = Some(vault_id.to_string());
        inner.vault = Some(vault);
        inner.status = Status::Unlocked;
        inner.last_activity = Instant::now();
        inner.config.last_opened = Some(vault_id.to_string());
        let _ = self.save_config(&inner);
    }

    pub fn unlock_failed(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.status = Status::Locked;
        inner.unlock_failures += 1;
        let wait = Duration::from_secs((1u64 << inner.unlock_failures.min(5)).min(30));
        inner.backoff_until = Some(Instant::now() + wait);
    }

    /// Starts an attachment upload (§6.1). The vault must be
    /// unlocked; the entry is checked by the core at finish.
    pub fn begin_upload(
        &self,
        entry_id: String,
        name: String,
        // Advisory only (§6.1): the type is sniffed at finish,
        // never taken from the hint, so it is not stored.
        _mime_hint: Option<String>,
    ) -> AppResult<String> {
        let mut inner = self.inner.lock().unwrap();
        if inner.vault.is_none() {
            return Err(AppError::Code("locked".into()));
        }
        inner.upload_seq += 1;
        let id = format!("upload-{}", inner.upload_seq);
        inner.uploads.insert(
            id.clone(),
            Upload { entry_id, name, bytes: Vec::new() },
        );
        Ok(id)
    }

    pub fn push_chunk(&self, upload_id: &str, bytes: &[u8]) -> AppResult<()> {
        let mut inner = self.inner.lock().unwrap();
        let upload = inner
            .uploads
            .get_mut(upload_id)
            .ok_or_else(|| AppError::Code("invalid_input".into()))?;
        if upload.bytes.len() + bytes.len() > crate::commands::attachments::MAX_UPLOAD_HARD_CAP {
            return Err(AppError::Code("too_large".into()));
        }
        upload.bytes.extend_from_slice(bytes);
        Ok(())
    }

    pub fn take_upload(&self, upload_id: &str) -> AppResult<Upload> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .uploads
            .remove(upload_id)
            .ok_or_else(|| AppError::Code("invalid_input".into()))
    }

    /// The folder decrypted attachment copies land in when the
    /// user opens one (§6.4 rework): opened/<id>/<name> under the
    /// app config folder. Managed by the app, never by the user;
    /// wiped on lock and at startup, so plaintext copies exist
    /// only while the diary is unlocked.
    pub fn opened_dir(&self) -> PathBuf {
        self.config_file
            .parent()
            .map(|p| p.join("opened"))
            .unwrap_or_else(|| PathBuf::from("opened"))
    }

    pub fn clear_opened(&self) {
        let _ = fs::remove_dir_all(self.opened_dir());
    }

    pub fn drop_upload(&self, upload_id: &str) {
        self.inner.lock().unwrap().uploads.remove(upload_id);
    }

    /// Unlocked ──► Locking ──► Locked (or Unavailable if the vault
    /// folder is gone). Zeroizes the keys via the core's lock().
    pub fn lock(&self) {
        self.clear_opened();
        let mut inner = self.inner.lock().unwrap();
        inner.uploads.clear();
        if inner.vault.is_none() {
            return;
        }
        inner.status = Status::Locking;
        if let Some(vault) = inner.vault.take() {
            vault.lock();
        }
        inner.status = match &inner.current {
            Some(id) => {
                let there = inner
                    .config
                    .vaults
                    .iter()
                    .find(|v| &v.id == id)
                    .map(|v| v.path.join("vault.header.json").exists())
                    .unwrap_or(false);
                if there {
                    Status::Locked
                } else {
                    Status::Unavailable
                }
            }
            None => Status::Unavailable,
        };
        inner.last_activity = Instant::now();
    }

    /// True when the idle timeout has passed; the watcher in lib.rs
    /// calls lock() and tells the UI.
    pub fn idle_expired(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.status == Status::Unlocked
            && inner.last_activity.elapsed()
                >= Duration::from_secs(inner.config.settings.lock_timeout_secs)
    }

    /// THE guard (§6.2). Every vault-touching command runs its work
    /// through here: not Unlocked → the `locked` error code.
    /// Locks the vault if its folder has gone (volume ejected
    /// mid-session, §3): keys are dropped now, not at the next
    /// I/O error. Returns true when it locked. The watcher calls
    /// this every few seconds; with_vault checks too.
    pub fn lock_if_current_missing(&self) -> bool {
        let gone = {
            let inner = self.inner.lock().unwrap();
            if inner.status != Status::Unlocked {
                return false;
            }
            match &inner.current {
                Some(id) => inner
                    .config
                    .vaults
                    .iter()
                    .find(|v| &v.id == id)
                    .map(|v| availability_of(&v.path) != "available")
                    .unwrap_or(false),
                None => false,
            }
        };
        if gone {
            self.lock();
        }
        gone
    }

    pub fn with_vault<T>(
        &self,
        work: impl FnOnce(&mut Vault) -> AppResult<T>,
    ) -> AppResult<T> {
        // A vanished volume (§3) locks the vault before any work
        // touches the dead database handle.
        if self.lock_if_current_missing() {
            return Err(AppError::Code("vault_unavailable".into()));
        }
        let mut inner = self.inner.lock().unwrap();
        if inner.status != Status::Unlocked {
            return Err(AppError::Code("locked".into()));
        }
        inner.last_activity = Instant::now();
        let vault = inner.vault.as_mut().expect("Unlocked implies a vault");
        work(vault)
    }
}

/// Where a known vault actually stands (§3, §6.1): the header
/// file is there ("available"), the folder is gone from a volume
/// that is itself gone ("volume_missing" — the mountpoint under
/// /Volumes is missing), or the volume is there but the folder
/// is not ("not_found": moved, renamed, deleted).
pub fn availability_of(path: &std::path::Path) -> &'static str {
    if path.join("vault.header.json").exists() {
        return "available";
    }
    let mut cur = path;
    loop {
        if cur.exists() {
            return "not_found";
        }
        match cur.parent() {
            Some(parent) => {
                if parent == std::path::Path::new("/Volumes") {
                    return "volume_missing";
                }
                cur = parent;
            }
            None => return "not_found",
        }
    }
}

fn folder_display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Folder".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use soirvu_core::crypto::KdfParams;

    /// Production-floor KDF: test-only low floors do not apply
    /// outside soirvu-core's own test builds.
    fn fast_legal_kdf() -> KdfParams {
        KdfParams { m_kib: 64 * 1024, t: 3, p: 1 }
    }

    fn state_with_vault() -> (tempfile::TempDir, AppState, String) {
        let dir = tempfile::TempDir::new().unwrap();
        let vault_dir = dir.path().join("vault");
        let (vault, _recovery) =
            Vault::create(&vault_dir, "pass", fast_legal_kdf()).unwrap();
        let vault_id = vault.header().vault_id.clone();
        let state = AppState::load(dir.path().join("config"));
        state.register(vault_id.clone(), "Test".into(), vault_dir).unwrap();
        state.adopt_created(vault, &vault_id);
        (dir, state, vault_id)
    }

    #[test]
    fn the_guard_refuses_until_unlocked_and_after_lock() {
        let (_dir, state, _id) = state_with_vault();
        // adopt_created leaves it Unlocked: the guard passes.
        assert!(state.with_vault(|_| Ok(())).is_ok());
        state.lock();
        assert_eq!(state.status(), Status::Locked);
        match state.with_vault(|_| Ok(())) {
            Err(AppError::Code(code)) => assert_eq!(code, "locked"),
            _ => panic!("locked state must refuse with the locked code"),
        }
    }

    #[test]
    fn a_failed_unlock_starts_the_backoff() {
        let (_dir, state, id) = state_with_vault();
        state.lock();
        state.begin_unlock(&id).unwrap();
        state.unlock_failed();
        match state.begin_unlock(&id) {
            Err(AppError::Backoff(secs)) => assert!((1..=30).contains(&secs)),
            _ => panic!("a failed unlock must back off"),
        }
    }

    #[test]
    fn idle_expiry_uses_the_settings_timeout() {
        let (_dir, state, _id) = state_with_vault();
        assert!(!state.idle_expired());
        let mut settings = state.settings();
        settings.lock_timeout_secs = 0;
        state.set_settings(settings).unwrap();
        assert!(state.idle_expired());
        state.lock();
        assert!(!state.idle_expired(), "a locked vault is never idle-expired");
    }

    #[test]
    fn the_registry_survives_a_reload_and_forget_removes() {
        let (dir, state, id) = state_with_vault();
        state.lock();
        let reloaded = AppState::load(dir.path().join("config"));
        let list = reloaded.vault_list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].state, "available");
        reloaded.forget(&id).unwrap();
        assert!(reloaded.vault_list().is_empty());
    }
}

#[cfg(test)]
mod availability_tests {
    use super::availability_of;
    use std::io::Write;

    #[test]
    fn present_header_is_available() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("MyVault");
        std::fs::create_dir(&vault).unwrap();
        std::fs::File::create(vault.join("vault.header.json"))
            .unwrap()
            .write_all(b"{}")
            .unwrap();
        assert_eq!(availability_of(&vault), "available");
    }

    #[test]
    fn deleted_folder_on_a_live_volume_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("Moved");
        // The parent (the tempdir, our "volume") exists; the
        // vault folder does not.
        assert_eq!(availability_of(&vault), "not_found");
    }

    #[test]
    fn folder_without_header_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("NotAVault");
        std::fs::create_dir(&vault).unwrap();
        assert_eq!(availability_of(&vault), "not_found");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn missing_volume_is_volume_missing() {
        let vault = std::path::Path::new("/Volumes/soirvu-no-such-volume-9f3/Diary");
        assert_eq!(availability_of(vault), "volume_missing");
    }
}
