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
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use crate::lock_handshake::LockHandshake;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use soirvu_core::Vault;

use crate::error::{AppError, AppResult};
use zeroize::Zeroize;

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
    /// The folder this vault syncs through (Phase 10):
    /// bundles and acks live there; another tool carries
    /// the folder between devices. A path, so like `path`
    /// it never crosses the IPC boundary — Settings sees
    /// only its display name (Settings::sync_location).
    #[serde(default)]
    pub sync_dir: Option<PathBuf>,
    /// The device identity assigned to this vault
    /// (Phase 10b step 3): minted here, in the app's
    /// own settings, and handed to the core at open —
    /// a copied vault folder arrives with no entry in
    /// this app's registry and gets a fresh identity.
    #[serde(default)]
    pub device_id: Option<String>,
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
        Caps {
            image_mb: 25,
            pdf_mb: 100,
            audio_mb: 200,
            video_mb: 1024,
        }
    }
}

/// The scheduled-backup rule (§13 Phase 6): the open vault is
/// copied to the backup location at most this often.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackupSchedule {
    #[default]
    Off,
    Daily,
    Weekly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub lock_timeout_secs: u64,
    pub strip_exif: bool,
    /// §9: clear the clipboard 30 s after a copy from the diary
    /// (only if nothing else has been copied since).
    #[serde(default)]
    pub clipboard_clear: bool,
    /// How often the open vault backs itself up (§13 Phase 6).
    #[serde(default)]
    pub backup_schedule: BackupSchedule,
    pub caps: Caps,
    /// Display name of the backup folder (never a path — paths
    /// stay in Rust). Derived from AppConfig::backup_dir whenever
    /// settings are read; the value sent back by settings_set is
    /// ignored. Null until a folder is picked.
    #[serde(default)]
    pub backup_location: Option<String>,
    /// Where the open diary lives, as a full path (derived like
    /// backup_location; the value sent back by settings_set is
    /// ignored). The diary's own home is shown in full, on
    /// purpose: the user should always know where it is.
    #[serde(default)]
    pub vault_location: Option<String>,
    /// The OS this build runs on ("macos", "linux",
    /// "windows" — std::env::consts::OS), derived on read
    /// like the two locations above and ignored on set. The
    /// UI uses it to offer only what the platform supports
    /// (the clipboard clear is macOS-only: it needs the
    /// NSPasteboard change counter).
    #[serde(default)]
    pub platform: String,
    /// Display name of the OPEN vault's sync folder (never
    /// a path). Derived from the vault's registry entry
    /// whenever settings are read, like backup_location;
    /// the value sent back by settings_set is ignored.
    /// Null until a sync folder is picked for this vault.
    #[serde(default)]
    pub sync_location: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lock_timeout_secs: 300,
            strip_exif: true,
            clipboard_clear: false,
            backup_schedule: BackupSchedule::Off,
            platform: String::new(),
            caps: Caps::default(),
            backup_location: None,
            vault_location: None,
            sync_location: None,
        }
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
    pending_fork: Option<PathBuf>,
    /// In-flight attachment uploads (§6.1): chunks accumulate here
    /// until attachment_finish commits them. Plaintext lives in
    /// process memory only — never on disk — and is dropped on lock.
    uploads: std::collections::HashMap<String, Upload>,
    upload_bytes: usize,
    upload_seq: u64,
}

/// Payload of the `lock-requested` event (§6.2): the generation
/// the frontend must echo in `vault_lock_ready`.
#[derive(Serialize, Clone)]
pub struct LockRequested {
    pub generation: u64,
}

/// One in-flight attachment upload.
pub struct Upload {
    pub entry_id: String,
    pub name: String,
    pub bytes: Vec<u8>,
}

impl Drop for Upload {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

pub struct AppState {
    inner: Mutex<Inner>,
    // Serializes complete vault operations, including multi-step file
    // workflows. The inner mutex protects state; this gate keeps a lock,
    // backup snapshot, or plaintext materialization from interleaving with
    // another vault operation between individual database calls.
    vault_operations: Mutex<()>,
    /// Lock handshake (§6.2). Status stays Unlocked while one is
    /// pending, so the editor's flush still passes the guard.
    pub handshake: LockHandshake,
    /// Serializes forced locks so two triggers cannot both run the
    /// lock/emit/reload sequence.
    lock_serial: Mutex<()>,
    config_file: PathBuf,
}

/// Temporarily owns the open vault while a command uses it without holding
/// the state mutex. If the command unwinds, the vault is returned to state so
/// `Unlocked` cannot be left without its corresponding vault.
struct VaultLease<'a> {
    state: &'a AppState,
    vault: Option<Vault>,
}

impl Deref for VaultLease<'_> {
    type Target = Vault;

    fn deref(&self) -> &Self::Target {
        self.vault.as_ref().expect("vault lease is active")
    }
}

impl DerefMut for VaultLease<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.vault.as_mut().expect("vault lease is active")
    }
}

impl VaultLease<'_> {
    fn lock(mut self) {
        // Mark the state as transitioning before consuming the vault. If the
        // vault lock operation unwinds, Drop must not restore a locked vault
        // under an `Unlocked` status.
        {
            let mut inner = self
                .state
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner.status = Status::Locking;
        }
        self.vault.take().expect("vault lease is active").lock();
    }
}

impl Drop for VaultLease<'_> {
    fn drop(&mut self) {
        let Some(vault) = self.vault.take() else {
            return;
        };
        let mut inner = self
            .state
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A completed lock transition intentionally discards the lease.
        // For an ordinary operation (including panic unwinding), restore it.
        if inner.status == Status::Unlocked {
            inner.vault = Some(vault);
        }
    }
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
                pending_fork: None,
                uploads: std::collections::HashMap::new(),
                upload_bytes: 0,
                upload_seq: 0,
            }),
            vault_operations: Mutex::new(()),
            handshake: LockHandshake::default(),
            lock_serial: Mutex::new(()),
            config_file,
        };
        // Copies left by a crashed run must not outlive it.
        let _ = state.clear_opened();
        state
    }

    fn save_config(&self, inner: &Inner) -> AppResult<()> {
        if let Some(parent) = self.config_file.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&inner.config)?;
        let tmp = self
            .config_file
            .with_extension(format!("json.tmp.{}", std::process::id()));
        let mut file = fs::File::create(&tmp)?;
        use std::io::Write;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, &self.config_file)?;
        #[cfg(unix)]
        if let Some(parent) = self.config_file.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }

    /// The operation mutex only represents exclusive access; recovering its
    /// guard after unwinding is safe because it contains no mutable data.
    fn lock_vault_operations(&self) -> MutexGuard<'_, ()> {
        self.vault_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
        settings.backup_location = inner.config.backup_dir.as_deref().map(folder_display_name);
        settings.platform = std::env::consts::OS.to_string();
        settings.vault_location = inner.current.as_ref().and_then(|id| {
            inner
                .config
                .vaults
                .iter()
                .find(|v| &v.id == id)
                .map(|v| v.path.to_string_lossy().into_owned())
        });
        settings.sync_location = inner.current.as_ref().and_then(|id| {
            inner
                .config
                .vaults
                .iter()
                .find(|v| &v.id == id)
                .and_then(|v| v.sync_dir.as_deref())
                .map(folder_display_name)
        });
        settings
    }

    /// Whether the clipboard should be cleared after a copy (§9).
    pub fn clipboard_clear(&self) -> bool {
        self.inner.lock().unwrap().config.settings.clipboard_clear
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
    /// The backup folder the user picked (a path: Rust only).
    pub fn backup_dir(&self) -> Option<PathBuf> {
        self.inner.lock().unwrap().config.backup_dir.clone()
    }

    /// The scheduled-backup rule from the settings.
    pub fn backup_schedule(&self) -> BackupSchedule {
        self.inner.lock().unwrap().config.settings.backup_schedule
    }

    pub fn set_backup_dir(&self, dir: Option<PathBuf>) -> AppResult<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.config.backup_dir = dir;
        self.save_config(&inner)
    }

    /// The open vault's sync folder, if one is set.
    pub fn current_sync_dir(&self) -> Option<PathBuf> {
        let inner = self.inner.lock().unwrap();
        let current = inner.current.as_ref()?;
        inner
            .config
            .vaults
            .iter()
            .find(|v| &v.id == current)
            .and_then(|v| v.sync_dir.clone())
    }

    /// Stores (or clears) a vault's sync folder, chosen
    /// through the native picker. Only Rust sees the path.
    pub fn set_sync_dir(&self, vault_id: &str, dir: Option<PathBuf>) -> AppResult<()> {
        let _operation = self.lock_vault_operations();
        let mut inner = self.inner.lock().unwrap();
        let entry = inner
            .config
            .vaults
            .iter_mut()
            .find(|v| v.id == vault_id)
            .ok_or_else(|| AppError::Code("vault_not_found".into()))?;
        entry.sync_dir = dir;
        self.save_config(&inner)
    }

    /// The device identity assigned to a vault, if
    /// one has been assigned yet.
    ///
    /// Entries registered before identities lived
    /// here (Phase 10b step 3) have none — their first
    /// unlock ADOPTS the id the vault already syncs
    /// under, so an existing diary never changes
    /// identity by upgrading. Entries registered
    /// since then were minted an identity at
    /// registration (see `register`): a folder that
    /// arrives at this app is a copy until it proves
    /// otherwise.
    pub fn assignment(&self, vault_id: &str) -> AppResult<Option<String>> {
        let inner = self.inner.lock().unwrap();
        inner
            .config
            .vaults
            .iter()
            .find(|v| v.id == vault_id)
            .map(|v| v.device_id.clone())
            .ok_or_else(|| AppError::Code("vault_not_found".into()))
    }

    /// Replaces a vault's assigned identity (restamp:
    /// the caller persists the new id here BEFORE the
    /// live vault switches to it, so an interrupted
    /// restamp still converges at the next open).
    pub fn set_device_id(&self, vault_id: &str, device_id: String) -> AppResult<()> {
        let _operation = self.lock_vault_operations();
        let mut inner = self.inner.lock().unwrap();
        let previous = inner.config.clone();
        let entry = inner
            .config
            .vaults
            .iter_mut()
            .find(|v| v.id == vault_id)
            .ok_or_else(|| AppError::Code("vault_not_found".into()))?;
        entry.device_id = Some(device_id);
        if let Err(error) = self.save_config(&inner) {
            inner.config = previous;
            return Err(error);
        }
        Ok(())
    }

    /// Persists and applies an identity change as one serialized vault
    /// operation. Persistence precedes the core write so an interrupted
    /// update converges to the assigned identity on the next open.
    pub fn restamp_current_device(&self, device_id: String) -> AppResult<()> {
        let _operation = self.lock_vault_operations();
        let mut inner = self.inner.lock().unwrap();
        if inner.status != Status::Unlocked {
            return Err(AppError::Code("locked".into()));
        }
        let vault_id = inner
            .current
            .clone()
            .ok_or_else(|| AppError::Code("locked".into()))?;
        let previous = inner.config.clone();
        let entry = inner
            .config
            .vaults
            .iter_mut()
            .find(|entry| entry.id == vault_id)
            .ok_or_else(|| AppError::Code("vault_not_found".into()))?;
        entry.device_id = Some(device_id.clone());
        if let Err(error) = self.save_config(&inner) {
            inner.config = previous;
            return Err(error);
        }
        let vault = inner.vault.as_mut().expect("Unlocked implies a vault");
        vault.restamp_device_to(&device_id)?;
        Ok(())
    }

    /// The vault list for the UI, with a state per vault.
    pub fn vault_list(&self) -> Vec<VaultInfo> {
        // Snapshot registry state briefly. Availability checks touch the
        // filesystem, so do them after releasing the state mutex and without
        // waiting for long-running sync or backup operations.
        let (vaults, current, status) = {
            let inner = self.inner.lock().unwrap();
            (
                inner.config.vaults.clone(),
                inner.current.clone(),
                inner.status,
            )
        };
        vaults
            .into_iter()
            .map(|v| VaultInfo {
                id: v.id.clone(),
                name: v.name.clone(),
                state: if Some(&v.id) == current.as_ref() && status == Status::Unlocked {
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
        self.register_with_device(id, name, path, None)
    }

    /// Registers a vault with the identity already assigned by its
    /// creation or copy-opening operation. Existing entries keep their
    /// assignment when `device_id` is absent (ordinary relocation).
    pub fn register_with_device(
        &self,
        id: String,
        name: String,
        path: PathBuf,
        device_id: Option<String>,
    ) -> AppResult<VaultRef> {
        let _operation = self.lock_vault_operations();
        let mut inner = self.inner.lock().unwrap();
        if inner.current.as_deref() == Some(id.as_str())
            && inner.vault.is_some()
            && inner
                .config
                .vaults
                .iter()
                .find(|v| v.id == id)
                .is_some_and(|entry| entry.path != path)
        {
            return Err(AppError::Other(
                "lock the vault before changing its folder".into(),
            ));
        }
        let previous = inner.config.clone();
        if let Some(existing) = inner.config.vaults.iter_mut().find(|v| v.id == id) {
            existing.path = path;
            existing.name = name;
            if let Some(device_id) = device_id {
                existing.device_id = Some(device_id);
            }
            let entry = existing.clone();
            if let Err(error) = self.save_config(&inner) {
                inner.config = previous;
                return Err(error);
            }
            return Ok(entry);
        }
        let entry = VaultRef {
            id,
            name,
            path,
            sync_dir: None,
            device_id: Some(device_id.unwrap_or_else(soirvu_core::new_device_id)),
        };
        inner.config.vaults.push(entry.clone());
        if let Err(error) = self.save_config(&inner) {
            inner.config = previous;
            return Err(error);
        }
        Ok(entry)
    }

    pub fn forget(&self, vault_id: &str) -> AppResult<()> {
        let _operation = self.lock_vault_operations();
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

    /// An imported backup copy waiting for its own identity
    /// (Phase 6): the folder the copy was written to. The fork
    /// command peeks (a wrong passphrase must not lose it) and
    /// clears on success.
    pub fn set_pending_fork(&self, path: PathBuf) {
        self.inner.lock().unwrap().pending_fork = Some(path);
    }

    pub fn pending_fork(&self) -> Option<PathBuf> {
        self.inner.lock().unwrap().pending_fork.clone()
    }

    pub fn clear_pending_fork(&self) {
        self.inner.lock().unwrap().pending_fork = None;
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
        let _operation = self.lock_vault_operations();
        let mut inner = self.inner.lock().unwrap();
        inner.current = Some(vault_id.to_string());
        inner.vault = Some(vault);
        inner.status = Status::Unlocked;
        inner.last_activity = Instant::now();
        inner.config.last_opened = Some(vault_id.to_string());
        let _ = self.save_config(&inner);
    }

    /// Back to Locked WITHOUT counting a failure: the user
    /// cancelled (a dismissed Touch ID prompt is not a wrong
    /// guess, and the §6.2 backoff is for guesses).
    pub fn unlock_aborted(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.status = Status::Locked;
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
        let _operation = self.lock_vault_operations();
        let mut inner = self.inner.lock().unwrap();
        if inner.vault.is_none() {
            return Err(AppError::Code("locked".into()));
        }
        if inner.uploads.len() >= crate::commands::attachments::MAX_ACTIVE_UPLOADS {
            return Err(AppError::Code("too_many_uploads".into()));
        }
        inner.upload_seq += 1;
        let id = format!("upload-{}", inner.upload_seq);
        inner.uploads.insert(
            id.clone(),
            Upload {
                entry_id,
                name,
                bytes: Vec::new(),
            },
        );
        Ok(id)
    }

    pub fn push_chunk(&self, upload_id: &str, bytes: &[u8]) -> AppResult<()> {
        let mut inner = self.inner.lock().unwrap();
        let current_len = inner
            .uploads
            .get(upload_id)
            .ok_or_else(|| AppError::Code("invalid_input".into()))?
            .bytes
            .len();
        let upload_len = current_len
            .checked_add(bytes.len())
            .ok_or_else(|| AppError::Code("too_large".into()))?;
        let total = inner
            .upload_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| AppError::Code("too_large".into()))?;
        if upload_len > crate::commands::attachments::MAX_UPLOAD_HARD_CAP
            || total > crate::commands::attachments::MAX_UPLOAD_TOTAL
        {
            return Err(AppError::Code("too_large".into()));
        }
        inner
            .uploads
            .get_mut(upload_id)
            .expect("checked above")
            .bytes
            .extend_from_slice(bytes);
        inner.upload_bytes = total;
        Ok(())
    }

    pub fn take_upload(&self, upload_id: &str) -> AppResult<Upload> {
        let mut inner = self.inner.lock().unwrap();
        let upload = inner
            .uploads
            .remove(upload_id)
            .ok_or_else(|| AppError::Code("invalid_input".into()))?;
        inner.upload_bytes = inner.upload_bytes.saturating_sub(upload.bytes.len());
        Ok(upload)
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

    pub fn clear_opened(&self) -> AppResult<()> {
        let dir = self.opened_dir();
        match fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn drop_upload(&self, upload_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(upload) = inner.uploads.remove(upload_id) {
            inner.upload_bytes = inner.upload_bytes.saturating_sub(upload.bytes.len());
        }
    }

    /// Begin a lock handshake. None if not Unlocked or one is pending.
    pub fn begin_lock(&self) -> Option<u64> {
        if self.status() != Status::Unlocked {
            return None;
        }
        self.handshake.begin()
    }

    /// Lock only if still Unlocked. Returns true if this call locked.
    /// A trigger that joined someone else's wait uses this, so the
    /// window is reloaded once, not once per trigger.
    pub fn lock_once(&self) -> AppResult<bool> {
        let _serial = self
            .lock_serial
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.status() != Status::Unlocked {
            return Ok(false);
        }
        self.lock()?;
        Ok(true)
    }

    /// Unlocked ──► Locking ──► Locked (or Unavailable if the vault
    /// folder is gone). Zeroizes the keys via the core's lock().
    pub fn lock(&self) -> AppResult<()> {
        let _operation = self.lock_vault_operations();
        let mut inner = self.inner.lock().unwrap();
        // A direct lock supersedes any pending handshake: the keys
        // drop now, so a late ack must not end a later wait.
        self.handshake.cancel();
        inner.uploads.clear();
        inner.upload_bytes = 0;
        if inner.vault.is_none() {
            drop(inner);
            return self.clear_opened();
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
        drop(inner);
        // The operation gate ensures no attachment writer can recreate a
        // plaintext file after this wipe begins.
        self.clear_opened()
    }

    /// True when the idle timeout has passed; the watcher in lib.rs
    /// calls lock() and tells the UI.
    pub fn idle_expired(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.status == Status::Unlocked
            && inner.last_activity.elapsed()
                >= Duration::from_secs(inner.config.settings.lock_timeout_secs)
    }

    /// Record user activity: the idle watcher measures silence
    /// since this stamp, not since unlock.
    pub fn note_activity(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.last_activity = Instant::now();
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
            let _ = self.lock();
        }
        gone
    }

    pub fn with_vault<T>(&self, work: impl FnOnce(&mut Vault) -> AppResult<T>) -> AppResult<T> {
        // A vanished volume (§3) locks the vault before any work
        // touches the dead database handle.
        if self.lock_if_current_missing() {
            return Err(AppError::Code("vault_unavailable".into()));
        }
        let _operation = self.lock_vault_operations();
        self.with_vault_locked(work)
    }

    /// Runs a complete multi-step operation while excluding every other
    /// vault operation and lock transition.
    pub fn with_vault_exclusive<T>(
        &self,
        work: impl FnOnce(&mut Vault) -> AppResult<T>,
    ) -> AppResult<T> {
        if self.lock_if_current_missing() {
            return Err(AppError::Code("vault_unavailable".into()));
        }
        let _operation = self.lock_vault_operations();
        self.with_vault_locked(work)
    }

    /// Runs a final authenticated vault migration, locks the in-memory vault,
    /// wipes opened plaintext, and performs the filesystem swap before any
    /// competing vault operation can start.
    pub fn with_vault_exclusive_then_lock<T>(
        &self,
        expected_vault_path: &std::path::Path,
        work: impl FnOnce(&mut Vault) -> AppResult<T>,
        after_lock: impl FnOnce() -> AppResult<()>,
    ) -> AppResult<T> {
        if self.lock_if_current_missing() {
            return Err(AppError::Code("vault_unavailable".into()));
        }
        let _operation = self.lock_vault_operations();
        let mut vault = {
            let mut inner = self.inner.lock().unwrap();
            if inner.status != Status::Unlocked {
                return Err(AppError::Code("locked".into()));
            }
            let current_matches = inner
                .current
                .as_ref()
                .and_then(|id| inner.config.vaults.iter().find(|entry| &entry.id == id))
                .is_some_and(|entry| entry.path == expected_vault_path);
            if !current_matches {
                return Err(AppError::Code("vault_mismatch".into()));
            }
            VaultLease {
                state: self,
                vault: Some(inner.vault.take().expect("Unlocked implies a vault")),
            }
        };
        let result = work(&mut vault)?;
        vault.lock();
        {
            let mut inner = self.inner.lock().unwrap();
            inner.uploads.clear();
            inner.upload_bytes = 0;
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
        self.clear_opened()?;
        after_lock()?;
        Ok(result)
    }

    fn with_vault_locked<T>(&self, work: impl FnOnce(&mut Vault) -> AppResult<T>) -> AppResult<T> {
        let mut vault = {
            let mut inner = self.inner.lock().unwrap();
            if inner.status != Status::Unlocked {
                return Err(AppError::Code("locked".into()));
            }
            inner.last_activity = Instant::now();
            VaultLease {
                state: self,
                vault: Some(inner.vault.take().expect("Unlocked implies a vault")),
            }
        };
        work(&mut vault)
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
        KdfParams {
            m_kib: 64 * 1024,
            t: 3,
            p: 1,
        }
    }

    fn state_with_vault() -> (tempfile::TempDir, AppState, String) {
        let dir = tempfile::TempDir::new().unwrap();
        let vault_dir = dir.path().join("vault");
        let (vault, _recovery) = Vault::create(&vault_dir, "pass", fast_legal_kdf()).unwrap();
        let vault_id = vault.header().vault_id.clone();
        let state = AppState::load(dir.path().join("config"));
        state
            .register(vault_id.clone(), "Test".into(), vault_dir)
            .unwrap();
        state.adopt_created(vault, &vault_id);
        (dir, state, vault_id)
    }

    #[test]
    fn the_guard_refuses_until_unlocked_and_after_lock() {
        let (_dir, state, _id) = state_with_vault();
        // adopt_created leaves it Unlocked: the guard passes.
        assert!(state.with_vault(|_| Ok(())).is_ok());
        let _ = state.lock();
        assert_eq!(state.status(), Status::Locked);
        match state.with_vault(|_| Ok(())) {
            Err(AppError::Code(code)) => assert_eq!(code, "locked"),
            _ => panic!("locked state must refuse with the locked code"),
        }
    }

    #[test]
    fn lock_waits_for_plaintext_materialization_then_wipes_it() {
        let (_dir, state, _id) = state_with_vault();
        let opened = state.opened_dir();
        let marker = opened.join("attachment").join("plain.txt");
        let (start_lock_tx, start_lock_rx) = std::sync::mpsc::channel();
        let (attempted_tx, attempted_rx) = std::sync::mpsc::channel();

        std::thread::scope(|scope| {
            let state_ref = &state;
            scope.spawn(move || {
                start_lock_rx.recv().unwrap();
                attempted_tx.send(()).unwrap();
                state_ref.lock().unwrap();
            });
            state
                .with_vault_exclusive(|_| {
                    fs::create_dir_all(marker.parent().unwrap())?;
                    fs::write(&marker, b"plaintext")?;
                    start_lock_tx.send(()).unwrap();
                    attempted_rx.recv().unwrap();
                    Ok(())
                })
                .unwrap();
        });

        assert_eq!(state.status(), Status::Locked);
        assert!(
            !opened.exists(),
            "lock must remove plaintext written by the in-flight operation"
        );
    }

    #[test]
    fn writes_wait_until_an_exclusive_backup_snapshot_finishes() {
        let (_dir, state, _id) = state_with_vault();
        let (start_write_tx, start_write_rx) = std::sync::mpsc::channel();
        let (attempted_tx, attempted_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();

        std::thread::scope(|scope| {
            let state_ref = &state;
            scope.spawn(move || {
                start_write_rx.recv().unwrap();
                attempted_tx.send(()).unwrap();
                state_ref
                    .with_vault(|vault| {
                        vault.create_entry(&soirvu_core::entries::EntryDraft {
                            entry_date: "2026-10-19".into(),
                            title: Some("after snapshot".into()),
                            body_md: String::new(),
                            mood: None,
                            tz: None,
                            tags: Vec::new(),
                        })?;
                        Ok(())
                    })
                    .unwrap();
                finished_tx.send(()).unwrap();
            });
            state
                .with_vault_exclusive(|_| {
                    start_write_tx.send(()).unwrap();
                    attempted_rx.recv().unwrap();
                    assert!(finished_rx.try_recv().is_err());
                    Ok(())
                })
                .unwrap();
        });

        finished_rx.recv().unwrap();
    }

    #[test]
    fn uploads_obey_an_aggregate_budget_and_release_it_on_abort() {
        let (_dir, state, _id) = state_with_vault();
        let upload = state
            .begin_upload("entry".into(), "file".into(), None)
            .unwrap();
        state.inner.lock().unwrap().upload_bytes = crate::commands::attachments::MAX_UPLOAD_TOTAL;
        assert!(
            matches!(state.push_chunk(&upload, b"x"), Err(AppError::Code(code)) if code == "too_large")
        );

        state.inner.lock().unwrap().upload_bytes = 0;
        state.push_chunk(&upload, b"payload").unwrap();
        state.drop_upload(&upload);
        assert_eq!(state.inner.lock().unwrap().upload_bytes, 0);

        let uploads: Vec<_> = (0..crate::commands::attachments::MAX_ACTIVE_UPLOADS)
            .map(|i| {
                state
                    .begin_upload(format!("entry-{i}"), "file".into(), None)
                    .unwrap()
            })
            .collect();
        assert!(matches!(
            state.begin_upload("overflow".into(), "file".into(), None),
            Err(AppError::Code(code)) if code == "too_many_uploads"
        ));
        for upload in uploads {
            state.drop_upload(&upload);
        }
    }

    #[test]
    fn a_failed_unlock_starts_the_backoff() {
        let (_dir, state, id) = state_with_vault();
        let _ = state.lock();
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
        let _ = state.lock();
        assert!(
            !state.idle_expired(),
            "a locked vault is never idle-expired"
        );
    }

    #[test]
    fn note_activity_resets_idle_expiry() {
        let (_dir, state, _id) = state_with_vault();
        state.inner.lock().unwrap().last_activity = Instant::now() - Duration::from_secs(600);
        assert!(state.idle_expired());
        state.note_activity();
        assert!(!state.idle_expired());
    }

    #[test]
    fn a_vaults_sync_folder_is_per_vault_and_survives_reload() {
        // The read-back goes through the persisted
        // registry: surviving the reload is the point.
        fn stored_sync_dir(state: &AppState, id: &str) -> Option<PathBuf> {
            state
                .inner
                .lock()
                .unwrap()
                .config
                .vaults
                .iter()
                .find(|v| v.id == id)
                .and_then(|v| v.sync_dir.clone())
        }
        let (dir, state, id) = state_with_vault();
        assert_eq!(stored_sync_dir(&state, &id), None);
        assert_eq!(state.settings().sync_location, None);
        let sync = dir.path().join("sync-folder");
        state.set_sync_dir(&id, Some(sync.clone())).unwrap();
        assert_eq!(stored_sync_dir(&state, &id), Some(sync));
        assert_eq!(state.current_sync_dir(), stored_sync_dir(&state, &id));
        assert_eq!(
            state.settings().sync_location.as_deref(),
            Some("sync-folder")
        );
        let reloaded = AppState::load(dir.path().join("config"));
        assert_eq!(
            stored_sync_dir(&reloaded, &id),
            stored_sync_dir(&state, &id)
        );
        state.set_sync_dir(&id, None).unwrap();
        assert_eq!(stored_sync_dir(&state, &id), None);
        assert_eq!(state.settings().sync_location, None);
    }

    #[test]
    fn the_registry_survives_a_reload_and_forget_removes() {
        let (dir, state, id) = state_with_vault();
        let _ = state.lock();
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

    // platform-ok: the test asserts /Volumes mount semantics.
    #[cfg(target_os = "macos")]
    #[test]
    fn missing_volume_is_volume_missing() {
        let vault = std::path::Path::new("/Volumes/soirvu-no-such-volume-9f3/Diary");
        assert_eq!(availability_of(vault), "volume_missing");
    }
}
