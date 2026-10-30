// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! A vault folder on disk: creating it, and opening it again.
//!
//!   MyVault/
//!   ├── vault.header.json   key slots (no secrets); written last when creating
//!   ├── soirvu.db           encrypted SQLite database (plus -wal / -shm while open)
//!   └── attachments/ab/<id> encrypted attachment blobs, sharded by id prefix

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::crypto::{DeviceSecret, KdfParams, Key, MasterKey, RecoveryKey};
use crate::error::{Error, Result};
use crate::storage::Database;
use crate::vault::VaultHeader;

const HEADER_FILE: &str = "vault.header.json";
const DB_FILE: &str = "soirvu.db";
/// A fork interrupted between the database write and the header swap
/// leaves this file holding the new header, so opening can finish
/// the job instead of bricking the copy.
const FORK_MARKER: &str = ".fork-in-progress";
const BACKUPS_DIR: &str = "backups";

/// An UNLOCKED vault: the master key is in memory and the database is open.
///
/// Locking is just dropping the value: the database connection closes and the
/// master key is overwritten with zeros. There is deliberately no way to use a
/// vault after `lock()`, because `lock()` consumes it.
pub struct Vault {
    dir: PathBuf,
    header: VaultHeader,
    master: MasterKey,
    // The two below are `pub(crate)` so that entries.rs can add methods to Vault.
    pub(crate) db: Database,
    pub(crate) device_id: String,
}

impl Vault {
    /// Creates a new vault in `dir` (which must be missing or empty).
    /// Returns the open vault and the recovery key to show to the user once.
    pub fn create(dir: &Path, passphrase: &str, kdf: KdfParams) -> Result<(Vault, RecoveryKey)> {
        prepare_folder(dir)?;
        match create_files(dir, passphrase, kdf, None) {
            Ok(created) => Ok(created),
            Err(error) => {
                // The folder was empty before, so everything in it is ours.
                remove_our_files(dir);
                Err(error)
            }
        }
    }

    /// Creates a new vault whose device identity is
    /// ASSIGNED by the caller — the app, which keeps
    /// the assignment in its own settings outside the
    /// vault folder, so copying the folder does not
    /// copy the identity.
    pub fn create_with_device(
        dir: &Path,
        passphrase: &str,
        kdf: KdfParams,
        device_id: &str,
    ) -> Result<(Vault, RecoveryKey)> {
        prepare_folder(dir)?;
        match create_files(dir, passphrase, kdf, Some(device_id)) {
            Ok(created) => Ok(created),
            Err(error) => {
                remove_our_files(dir);
                Err(error)
            }
        }
    }

    pub fn open_with_passphrase(dir: &Path, passphrase: &str) -> Result<Vault> {
        let header = read_header(dir)?;
        let master = header.unlock_with_passphrase(passphrase)?;
        finish_open(dir, header, master, None)
    }

    /// Opens the vault AS the given device (the
    /// caller-assigned identity; see `create_with_device`).
    pub fn open_with_passphrase_as(
        dir: &Path,
        passphrase: &str,
        device_id: &str,
    ) -> Result<Vault> {
        let header = read_header(dir)?;
        let master = header.unlock_with_passphrase(passphrase)?;
        finish_open(dir, header, master, Some(device_id))
    }

    pub fn open_with_recovery(dir: &Path, recovery: &RecoveryKey) -> Result<Vault> {
        let header = read_header(dir)?;
        let master = header.unlock_with_recovery(recovery)?;
        finish_open(dir, header, master, None)
    }

    pub fn open_with_recovery_as(
        dir: &Path,
        recovery: &RecoveryKey,
        device_id: &str,
    ) -> Result<Vault> {
        let header = read_header(dir)?;
        let master = header.unlock_with_recovery(recovery)?;
        finish_open(dir, header, master, Some(device_id))
    }

    /// Opens the vault with the device secret (Phase 8). The
    /// caller fetched the secret from the OS keychain, which
    /// is where the biometrics were checked — the core only
    /// sees bytes, and a wrong or orphaned secret is
    /// Error::Unlock, like any other bad credential.
    pub fn open_with_device(dir: &Path, secret: &DeviceSecret) -> Result<Vault> {
        let header = read_header(dir)?;
        let master = header.unlock_with_device(secret)?;
        finish_open(dir, header, master, None)
    }

    pub fn open_with_device_as(
        dir: &Path,
        secret: &DeviceSecret,
        device_id: &str,
    ) -> Result<Vault> {
        let header = read_header(dir)?;
        let master = header.unlock_with_device(secret)?;
        finish_open(dir, header, master, Some(device_id))
    }

    /// Locks the vault: closes the database and wipes the keys from memory.
    pub fn lock(self) {
        drop(self);
    }

    /// Flushes the WAL into the main database file, so the
    /// vault folder can be copied file by file (backups, §13).
    /// The copy is as new as this checkpoint; the live database
    /// keeps working afterwards.
    pub fn checkpoint(&self) -> Result<()> {
        self.db
            .connection()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        Ok(())
    }

    /// Rotates the master key by migration (§9): builds a
    /// complete copy of this vault in `dest_dir` under a FRESH
    /// master key — same vault id, same passphrase, a NEW
    /// recovery key (returned, to be shown once) — and verifies
    /// the copy opens and holds the same entries and
    /// attachments before returning. This vault is not
    /// modified; the caller swaps the folders and deletes
    /// this one. The passphrase is verified first: it is the
    /// re-auth proof, and the copy's passphrase slot is
    /// wrapped from it.
    pub fn rotate_master_key(
        &self,
        dest_dir: &Path,
        passphrase: &str,
        kdf: KdfParams,
    ) -> Result<RecoveryKey> {
        self.header.unlock_with_passphrase(passphrase)?;
        let (header, recovery, master) =
            VaultHeader::create_with_id(&self.header.vault_id, passphrase, kdf)?;
        prepare_folder(dest_dir)?;
        self.db.export_encrypted(&dest_dir.join(DB_FILE), &master.db_key())?;
        write_atomic(&dest_dir.join(HEADER_FILE), &header.to_json()?)?;
        // The blobs are files, not database pages: re-encrypt
        // every live one under the new master, keeping its id
        // (entries reference attachments by id, and the
        // metadata rows just crossed over in the export).
        let ids: Vec<String> = {
            let conn = self.db.connection();
            let mut stmt = conn.prepare("SELECT id FROM attachment WHERE deleted_at IS NULL")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in &ids {
            let size = self.get_attachment(id)?.size as u64;
            let mut reader = AttachmentReader { vault: self, id, pos: 0, len: size };
            let key = crate::attachments::blob_key(&master.attachment_key(), id);
            crate::attachments::write_blob(dest_dir, id, &key, &mut reader)?;
        }
        let copy = Vault::open_with_passphrase(dest_dir, passphrase)?;
        let rows = |db: &Database, table: &str| -> Result<i64> {
            Ok(db
                .connection()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))?)
        };
        if rows(&self.db, "entry")? != rows(&copy.db, "entry")?
            || rows(&self.db, "attachment")? != rows(&copy.db, "attachment")?
        {
            return Err(Error::Database(rusqlite::Error::InvalidQuery));
        }
        copy.lock();
        Ok(recovery)
    }

    /// Proves the caller holds this vault's passphrase
    /// without changing anything — the re-authentication §8
    /// asks for before plaintext export.
    pub fn verify_passphrase(&self, passphrase: &str) -> Result<()> {
        self.header.unlock_with_passphrase(passphrase)?;
        Ok(())
    }

    /// Changes the passphrase. The current passphrase is
    /// verified against the slot first (a wrong one fails with
    /// Error::Unlock, like a bad unlock); then only the header
    /// is rewritten — the master key does not change, so no
    /// entry, attachment or database page is re-encrypted,
    /// and this open session simply continues.
    ///
    /// The vault keeps its stored Argon2id settings: the new
    /// slot re-wraps the same master key under the new
    /// passphrase with the existing KDF parameters. A change
    /// never recalibrates — calibrating here could silently
    /// weaken the vault on a throttled machine, strand it on
    /// weaker devices after tuning up elsewhere, and move
    /// unlock cost as a side effect of an unrelated action.
    pub fn change_passphrase(
        &mut self,
        current_passphrase: &str,
        new_passphrase: &str,
    ) -> Result<()> {
        self.header.unlock_with_passphrase(current_passphrase)?;
        let kdf = self.header.passphrase_kdf()?;
        let header = self
            .header
            .with_new_passphrase(new_passphrase, &self.master, kdf)?;
        write_atomic(&self.dir.join(HEADER_FILE), &header.to_json()?)?;
        self.header = header;
        Ok(())
    }

    /// Enrolls (or re-enrolls) the device slot (Phase 8): a
    /// fresh device secret is generated, the header gains the
    /// slot, and the secret is returned for the caller to
    /// store in the OS keychain. If the keychain store fails,
    /// the caller removes the slot again with remove_device —
    /// a slot whose secret was never stored opens nothing.
    pub fn enroll_device(&mut self) -> Result<DeviceSecret> {
        let (header, secret) = self.header.with_device_slot(&self.master)?;
        write_atomic(&self.dir.join(HEADER_FILE), &header.to_json()?)?;
        self.header = header;
        Ok(secret)
    }

    /// Removes the device slot, if any. The keychain item is
    /// the caller's to delete; a leftover item points at a
    /// slot that no longer exists and opens nothing.
    pub fn remove_device(&mut self) -> Result<()> {
        if !self.header.has_device_slot() {
            return Ok(());
        }
        let header = self.header.without_device_slot();
        write_atomic(&self.dir.join(HEADER_FILE), &header.to_json()?)?;
        self.header = header;
        Ok(())
    }

    pub fn has_device_slot(&self) -> bool {
        self.header.has_device_slot()
    }

    /// Replaces the recovery key with a fresh one, shown once
    /// by the caller. The current passphrase is the re-auth
    /// proof, as for a passphrase change.
    pub fn regenerate_recovery(&mut self, current_passphrase: &str) -> Result<RecoveryKey> {
        self.header.unlock_with_passphrase(current_passphrase)?;
        let (header, recovery) = self.header.with_new_recovery(&self.master)?;
        write_atomic(&self.dir.join(HEADER_FILE), &header.to_json()?)?;
        self.header = header;
        Ok(recovery)
    }

    /// Gives this vault a fresh identity (see
    /// VaultHeader::fork): the database's stored vault_id and
    /// the header are rewritten to the new id, and the new
    /// recovery key is returned — it is shown once, by the UI.
    /// The vault must already be open (the caller proved the
    /// passphrase by opening it). Operates on an imported copy
    /// before it is registered, so a failure leaves the copy
    /// unregistered and the original vault untouched.
    /// Crash-safe: the new header is staged in a marker file
    /// before the database moves, so an interruption is finished
    /// on the next open instead of bricking the copy.
    /// Caveat: a fork completed this way was never shown its new
    /// recovery key, and the old one no longer works — unlock with
    /// the passphrase and regenerate the recovery key afterwards.
    pub fn fork_identity(&mut self, passphrase: &str) -> Result<RecoveryKey> {
        let (header, recovery) = self.header.fork(passphrase, &self.master)?;
        write_atomic(&self.dir.join(FORK_MARKER), &header.to_json()?)?;
        self.db.connection().execute(
            "UPDATE meta SET value = ?1 WHERE key = 'vault_id'",
            rusqlite::params![header.vault_id],
        )?;
        write_atomic(&self.dir.join(HEADER_FILE), &header.to_json()?)?;
        let _ = fs::remove_file(self.dir.join(FORK_MARKER));
        self.header = header;
        Ok(recovery)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn header(&self) -> &VaultHeader {
        &self.header
    }

    /// The device identity this vault was opened (or
    /// created) AS — the caller-assigned id, or the
    /// stored one for settings-less callers.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn database(&self) -> &Database {
        &self.db
    }

    /// Root key for attachment files (used in a later step).
    pub fn attachment_key(&self) -> Key {
        self.master.attachment_key()
    }

    /// Key for sync bundles (Phase 10).
    pub fn sync_key(&self) -> Key {
        self.master.sync_key()
    }
}

/// Makes sure `dir` exists and is empty. A `.DS_Store` file, which Finder adds
/// to folders it has displayed, does not count.
fn prepare_folder(dir: &Path) -> Result<()> {
    if dir.exists() {
        let has_content = fs::read_dir(dir)?
            .filter_map(|entry| entry.ok())
            .any(|entry| entry.file_name() != ".DS_Store");
        if has_content {
            return Err(Error::FolderNotEmpty);
        }
    } else {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

fn create_files(
    dir: &Path,
    passphrase: &str,
    kdf: KdfParams,
    device: Option<&str>,
) -> Result<(Vault, RecoveryKey)> {
    let (header, recovery, master) = VaultHeader::create(passphrase, kdf)?;
    let mut db = Database::create(&dir.join(DB_FILE), &master.db_key(), &header.vault_id)?;
    let device_id = match device {
        Some(id) => {
            check_device_id(id)?;
            db.assign_device(id)?
        }
        None => db.ensure_device()?,
    };
    // The header goes last. A folder only counts as a vault once this file
    // exists, so a crash half-way leaves no vault that looks usable.
    write_atomic(&dir.join(HEADER_FILE), &header.to_json()?)?;
    let vault = Vault {
        dir: dir.to_path_buf(),
        header,
        master,
        db,
        device_id,
    };
    Ok((vault, recovery))
}

/// Copies a vault folder's content — the database, the header and the
/// encrypted attachment blobs — into `to` (created when missing).
/// Backups and restores both go through here, so a copy is always whole:
/// entries without their blobs are worse than no copy at all.
/// A missing `attachments/` tree copies as nothing (old copies predate it);
/// unfinished `.tmp` uploads and dotfiles are skipped, like the sweep does.
pub fn copy_vault_files(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    fs::copy(from.join(DB_FILE), to.join(DB_FILE))?;
    fs::copy(from.join(HEADER_FILE), to.join(HEADER_FILE))?;
    copy_attachments(from, to)?;
    Ok(())
}

/// Copies finished attachment blobs, preserving the id sharding.
fn copy_attachments(from: &Path, to: &Path) -> Result<()> {
    let src = from.join("attachments");
    if !src.exists() {
        return Ok(());
    }
    for shard in fs::read_dir(&src)? {
        let shard = shard?;
        if !shard.file_type()?.is_dir() {
            continue;
        }
        for file in fs::read_dir(shard.path())? {
            let file = file?;
            if !file.file_type()?.is_file() {
                continue;
            }
            let name = file.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            let dest_dir = to.join("attachments").join(shard.file_name());
            fs::create_dir_all(&dest_dir)?;
            fs::copy(file.path(), dest_dir.join(name.as_ref()))?;
        }
    }
    Ok(())
}

fn remove_our_files(dir: &Path) {
    for name in [
        HEADER_FILE,
        "vault.header.json.tmp",
        DB_FILE,
        "soirvu.db-wal",
        "soirvu.db-shm",
    ] {
        let _ = fs::remove_file(dir.join(name)); // best effort: ignore errors
    }
}

fn read_header(dir: &Path) -> Result<VaultHeader> {
    let bytes = fs::read(dir.join(HEADER_FILE))?;
    VaultHeader::from_json(&bytes)
}

/// Finishes a fork that died between the database write and the
/// header swap: the marker holds the new header and the database
/// already names it, so installing the header is exactly the
/// missing step. Anything else — no marker, an unreadable one,
/// or one naming a different id — is a genuinely mixed folder
/// and stays a mismatch, with the evidence renamed aside rather
/// than destroyed: failed opens never delete.
/// Moves a disproven marker out of the way for forensics. First
/// evidence wins: if a previous rejection is already there, the
/// marker stays put rather than destroying either copy.
fn set_marker_aside(dir: &Path, marker: &Path) {
    let rejected = dir.join(format!("{FORK_MARKER}.rejected"));
    if !rejected.exists() {
        let _ = fs::rename(marker, rejected);
    }
}

fn complete_interrupted_fork(dir: &Path, db: &Database) -> Result<Option<VaultHeader>> {
    let marker = dir.join(FORK_MARKER);
    let bytes = match fs::read(&marker) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let header = match VaultHeader::from_json(&bytes) {
        Ok(header) => header,
        Err(_) => {
            set_marker_aside(dir, &marker);
            return Ok(None);
        }
    };
    if db.meta("vault_id")?.as_deref() != Some(header.vault_id.as_str()) {
        set_marker_aside(dir, &marker);
        return Ok(None);
    }
    write_atomic(&dir.join(HEADER_FILE), &bytes)?;
    let _ = fs::remove_file(&marker);
    Ok(Some(header))
}

fn finish_open(
    dir: &Path,
    mut header: VaultHeader,
    master: MasterKey,
    device: Option<&str>,
) -> Result<Vault> {
    let mut db = Database::open(&dir.join(DB_FILE), &master.db_key(), &dir.join(BACKUPS_DIR))?;
    // Belt and braces: the database must say it belongs to this header.
    if db.meta("vault_id")?.as_deref() != Some(header.vault_id.as_str()) {
        match complete_interrupted_fork(dir, &db)? {
            Some(completed) => header = completed,
            None => return Err(Error::VaultMismatch),
        }
    }
    // A leftover marker with no mismatch behind it is just litter.
    let _ = fs::remove_file(dir.join(FORK_MARKER));
    let device_id = match device {
        Some(id) => {
            check_device_id(id)?;
            db.assign_device(id)?
        }
        None => db.ensure_device()?,
    };
    Ok(Vault {
        dir: dir.to_path_buf(),
        header,
        master,
        db,
        device_id,
    })
}

/// Writes a file so that a crash leaves either the old complete file or the
/// new complete file, never a half-written one: write to a temporary name,
/// force it to disk, then rename it over the target.
/// Plaintext of one attachment as a Read stream, in ranged
/// reads — rotation feeds it to the blob writer under the
/// new master key without holding the file in memory.
struct AttachmentReader<'a> {
    vault: &'a Vault,
    id: &'a str,
    pos: u64,
    len: u64,
}

impl Read for AttachmentReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.len || out.is_empty() {
            return Ok(0);
        }
        let want = (self.len - self.pos).min(out.len() as u64);
        let data = self
            .vault
            .read_attachment_range(self.id, self.pos..self.pos + want)
            .map_err(std::io::Error::other)?;
        out[..data.len()].copy_from_slice(&data);
        self.pos += data.len() as u64;
        Ok(data.len())
    }
}

/// Rename, retrying briefly. On Windows a just-closed file
/// can stay unrenamable for a moment while an antivirus or
/// the indexer holds it open (§12); five tries over half a
/// second cover the realistic holds. Elsewhere the first
/// result stands.
pub(crate) fn rename_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut result = fs::rename(from, to);
    if cfg!(windows) {
        for _ in 0..4 {
            if result.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
            result = fs::rename(from, to);
        }
    }
    result
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let mut file = File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    rename_retry(&tmp, path)?;
    // Also force the rename itself to disk (directories can be opened on Unix only).
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const PASSPHRASE: &str = "correct horse battery staple";
    const CANARY: &str = "секретная заметка Ж7Qx";

    fn fast() -> KdfParams {
        KdfParams {
            m_kib: 8,
            t: 1,
            p: 1,
        }
    }

    /// Creates a vault in a fresh temporary folder, writes one note, returns both.
    fn new_vault_with_note() -> (TempDir, Vault, RecoveryKey) {
        let tmp = TempDir::new().unwrap();
        let (vault, recovery) = Vault::create(&tmp.path().join("v"), PASSPHRASE, fast()).unwrap();
        let conn = vault.database().connection();
        conn.execute_batch("CREATE TABLE note (id INTEGER PRIMARY KEY, body TEXT NOT NULL)")
            .unwrap();
        conn.execute("INSERT INTO note (body) VALUES (?1)", [CANARY])
            .unwrap();
        (tmp, vault, recovery)
    }

    fn read_note(vault: &Vault) -> String {
        vault
            .database()
            .connection()
            .query_row("SELECT body FROM note", [], |row| row.get(0))
            .unwrap()
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    #[test]
    fn master_key_rotation_migrates_everything() {
        let (tmp, mut vault, old_recovery) = new_vault_with_note();
        let vault_id = vault.header().vault_id.clone();
        let saved = vault
            .create_entry(&crate::entries::EntryDraft {
                entry_date: "2026-10-15".into(),
                title: Some("rotation day".into()),
                body_md: "rotate me".into(),
                mood: None,
                tz: None,
                tags: vec![],
            })
            .unwrap();
        let att = vault
            .add_attachment(&saved.id, "note.txt", "text/plain", b"hello rotation")
            .unwrap();

        let dest = tmp.path().join("rotated");
        let new_recovery = vault.rotate_master_key(&dest, PASSPHRASE, fast()).unwrap();
        vault.lock();

        // The copy: same diary, same passphrase, everything in it.
        let copy = Vault::open_with_passphrase(&dest, PASSPHRASE).unwrap();
        assert_eq!(copy.header().vault_id, vault_id);
        assert_eq!(read_note(&copy), CANARY);
        assert_eq!(copy.read_attachment(&att.id).unwrap(), b"hello rotation");
        copy.lock();
        // ...under a NEW master: the old recovery key is dead,
        // the new one works.
        assert!(Vault::open_with_recovery(&dest, &old_recovery).is_err());
        assert!(Vault::open_with_recovery(&dest, &new_recovery).is_ok());
        // The original is untouched and still opens the old way.
        assert!(Vault::open_with_recovery(&tmp.path().join("v"), &old_recovery).is_ok());
    }

    #[test]
    fn passphrase_change_and_recovery_regeneration() {
        let (tmp, mut vault, recovery) = new_vault_with_note();
        vault
            .change_passphrase("not the passphrase", "second passphrase")
            .expect_err("the current passphrase is the proof");
        vault.change_passphrase(PASSPHRASE, "second passphrase").unwrap();

        let new_recovery = vault.regenerate_recovery("second passphrase").unwrap();
        vault.lock();

        let dir = tmp.path().join("v");
        assert!(Vault::open_with_passphrase(&dir, PASSPHRASE).is_err()); // old passphrase dead
        assert!(Vault::open_with_recovery(&dir, &recovery).is_err()); // old recovery dead
        let by_new_pass = Vault::open_with_passphrase(&dir, "second passphrase").unwrap();
        assert_eq!(read_note(&by_new_pass), CANARY); // data untouched
        by_new_pass.lock();
        assert!(Vault::open_with_recovery(&dir, &new_recovery).is_ok());
    }

    #[test]
    fn passphrase_change_keeps_the_vaults_kdf_params() {
        // A change re-wraps the master key under the new
        // passphrase with the STORED Argon2id settings — it
        // never recalibrates, so unlock cost and portability
        // survive the change byte for byte.
        let distinctive = KdfParams { m_kib: 32, t: 2, p: 1 };
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("v");
        let (mut vault, _recovery) = Vault::create(&dir, PASSPHRASE, distinctive).unwrap();
        vault.change_passphrase(PASSPHRASE, "second passphrase").unwrap();
        vault.lock();

        assert!(Vault::open_with_passphrase(&dir, PASSPHRASE).is_err());
        let reopened = Vault::open_with_passphrase(&dir, "second passphrase").unwrap();
        assert_eq!(reopened.header().passphrase_kdf().unwrap(), distinctive);
        reopened.lock();
    }


    #[test]
    fn data_survives_lock_and_reopen_with_passphrase() {
        let (tmp, vault, _recovery) = new_vault_with_note();
        let vault_id = vault.header().vault_id.clone();
        vault.lock();

        let reopened = Vault::open_with_passphrase(&tmp.path().join("v"), PASSPHRASE).unwrap();
        assert_eq!(reopened.header().vault_id, vault_id);
        assert_eq!(read_note(&reopened), CANARY);
    }

    #[test]
    fn recovery_key_reopens_the_vault() {
        let (tmp, vault, recovery) = new_vault_with_note();
        vault.lock();
        let typed = RecoveryKey::from_text(&recovery.to_text()).unwrap();
        let reopened = Vault::open_with_recovery(&tmp.path().join("v"), &typed).unwrap();
        assert_eq!(read_note(&reopened), CANARY);
    }

    #[test]
    fn wrong_passphrase_is_refused() {
        let (tmp, vault, _recovery) = new_vault_with_note();
        vault.lock();
        let result = Vault::open_with_passphrase(&tmp.path().join("v"), "wrong");
        assert!(matches!(result, Err(Error::Unlock)));
    }

    #[test]
    fn nothing_readable_is_written_to_disk() {
        let (tmp, vault, _recovery) = new_vault_with_note();
        let dir = tmp.path().join("v");

        // Check while the vault is OPEN: recent writes sit in the -wal file.
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.contains(&"soirvu.db-wal".to_string()),
            "files: {names:?}"
        );

        let scan = |label: &str| {
            for entry in fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let bytes = fs::read(entry.path()).unwrap();
                let name = entry.file_name().to_string_lossy().into_owned();
                assert!(
                    !contains(&bytes, CANARY.as_bytes()),
                    "{label}: {name} has the note"
                );
                assert!(
                    !contains(&bytes, "секретная".as_bytes()),
                    "{label}: {name} has a word"
                );
                if name.starts_with("soirvu.db") {
                    assert!(
                        !bytes.starts_with(b"SQLite format 3"),
                        "{label}: {name} is plain SQLite"
                    );
                }
            }
        };
        scan("open");
        vault.lock();
        scan("locked");
    }

    #[test]
    fn database_is_in_wal_mode() {
        let (_tmp, vault, _recovery) = new_vault_with_note();
        let mode: String = vault
            .database()
            .connection()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn create_refuses_a_folder_that_is_not_empty() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("holiday.jpg"), b"precious").unwrap();
        let result = Vault::create(tmp.path(), PASSPHRASE, fast());
        assert!(matches!(result, Err(Error::FolderNotEmpty)));
        // The existing file is untouched and nothing was added.
        assert_eq!(
            fs::read(tmp.path().join("holiday.jpg")).unwrap(),
            b"precious"
        );
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[test]
    fn create_accepts_an_empty_folder_with_ds_store() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".DS_Store"), b"finder").unwrap();
        assert!(Vault::create(tmp.path(), PASSPHRASE, fast()).is_ok());
    }

    #[test]
    fn a_second_vault_cannot_be_created_on_top_of_the_first() {
        let (tmp, vault, _recovery) = new_vault_with_note();
        vault.lock();
        let result = Vault::create(&tmp.path().join("v"), PASSPHRASE, fast());
        assert!(matches!(result, Err(Error::FolderNotEmpty)));
    }

    #[test]
    fn a_missing_database_file_is_an_error_and_is_not_recreated() {
        let (tmp, vault, _recovery) = new_vault_with_note();
        vault.lock();
        let dir = tmp.path().join("v");
        fs::remove_file(dir.join(DB_FILE)).unwrap();
        assert!(Vault::open_with_passphrase(&dir, PASSPHRASE).is_err());
        assert!(!dir.join(DB_FILE).exists());
    }

    #[test]
    fn a_damaged_header_file_on_disk_is_refused() {
        let (tmp, vault, _recovery) = new_vault_with_note();
        vault.lock();
        let dir = tmp.path().join("v");
        let mut bytes = fs::read(dir.join(HEADER_FILE)).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let at = text.find("\"vault_id\":\"").unwrap() + "\"vault_id\":\"".len();
        bytes[at] = if bytes[at] == b'0' { b'1' } else { b'0' };
        fs::write(dir.join(HEADER_FILE), bytes).unwrap();
        assert!(matches!(
            Vault::open_with_passphrase(&dir, PASSPHRASE),
            Err(Error::Unlock)
        ));
    }

    #[test]
    fn a_header_from_another_vault_does_not_open_this_database() {
        let tmp = TempDir::new().unwrap();
        let (a, _) = Vault::create(&tmp.path().join("a"), PASSPHRASE, fast()).unwrap();
        let (b, _) = Vault::create(&tmp.path().join("b"), PASSPHRASE, fast()).unwrap();
        a.lock();
        b.lock();
        // Same passphrase, so B's header unlocks fine, but yields B's keys.
        fs::copy(
            tmp.path().join("b").join(HEADER_FILE),
            tmp.path().join("a").join(HEADER_FILE),
        )
        .unwrap();
        let result = Vault::open_with_passphrase(&tmp.path().join("a"), PASSPHRASE);
        assert!(matches!(result, Err(Error::Database(_))));
    }

    #[test]
    fn a_database_that_names_a_different_vault_is_refused() {
        let (tmp, vault, _recovery) = new_vault_with_note();
        let key = vault.master.db_key();
        vault.lock();
        let dir = tmp.path().join("v");

        let db = Database::open(&dir.join(DB_FILE), &key, &dir.join(BACKUPS_DIR)).unwrap();
        db.connection()
            .execute(
                "UPDATE meta SET value = 'someone-else' WHERE key = 'vault_id'",
                [],
            )
            .unwrap();
        drop(db);

        let result = Vault::open_with_passphrase(&dir, PASSPHRASE);
        assert!(matches!(result, Err(Error::VaultMismatch)));
    }

    #[test]
    fn interrupted_fork_completes_on_open() {
        let (tmp, vault, _recovery) = new_vault_with_note();
        let dir = tmp.path().join("v");
        let key = vault.master.db_key();
        let (new_header, _) = vault.header.fork(PASSPHRASE, &vault.master).unwrap();
        let new_id = new_header.vault_id.clone();
        vault.lock();

        // The crash: the database moved to the new id, but the
        // header swap never landed. The staged marker remains.
        let db = Database::open(&dir.join(DB_FILE), &key, &dir.join(BACKUPS_DIR)).unwrap();
        db.connection()
            .execute(
                "UPDATE meta SET value = ?1 WHERE key = 'vault_id'",
                [&new_id],
            )
            .unwrap();
        drop(db);
        fs::write(dir.join(FORK_MARKER), new_header.to_json().unwrap()).unwrap();

        let copy = Vault::open_with_passphrase(&dir, PASSPHRASE).unwrap();
        assert_eq!(copy.header().vault_id, new_id);
        assert!(!dir.join(FORK_MARKER).exists());
        copy.lock();
    }

    #[test]
    fn garbage_marker_still_refuses() {
        let (tmp, vault, _recovery) = new_vault_with_note();
        let dir = tmp.path().join("v");
        let key = vault.master.db_key();
        vault.lock();

        let db = Database::open(&dir.join(DB_FILE), &key, &dir.join(BACKUPS_DIR)).unwrap();
        db.connection()
            .execute(
                "UPDATE meta SET value = 'someone-else' WHERE key = 'vault_id'",
                [],
            )
            .unwrap();
        drop(db);
        fs::write(dir.join(FORK_MARKER), b"not a header").unwrap();

        let result = Vault::open_with_passphrase(&dir, PASSPHRASE);
        assert!(matches!(result, Err(Error::VaultMismatch)));
        // Failed opens never delete: the evidence is renamed aside.
        assert!(!dir.join(FORK_MARKER).exists());
        assert_eq!(
            fs::read(dir.join(format!("{FORK_MARKER}.rejected"))).unwrap(),
            b"not a header"
        );
        // And the plain mismatch without any marker still refuses.
        let result = Vault::open_with_passphrase(&dir, PASSPHRASE);
        assert!(matches!(result, Err(Error::VaultMismatch)));
    }

    #[test]
    fn valid_marker_for_another_id_still_refuses() {
        let (tmp, vault, _recovery) = new_vault_with_note();
        let dir = tmp.path().join("v");
        let key = vault.master.db_key();
        let (other, _) = Vault::create(&tmp.path().join("other"), "other pass", fast()).unwrap();
        let other_header = fs::read(tmp.path().join("other").join(HEADER_FILE)).unwrap();
        other.lock();
        vault.lock();

        // The database names a third vault; the staged header belongs
        // to the second. This is not a half fork — refuse, keep evidence.
        let db = Database::open(&dir.join(DB_FILE), &key, &dir.join(BACKUPS_DIR)).unwrap();
        db.connection()
            .execute(
                "UPDATE meta SET value = 'someone-else' WHERE key = 'vault_id'",
                [],
            )
            .unwrap();
        drop(db);
        fs::write(dir.join(FORK_MARKER), &other_header).unwrap();

        let result = Vault::open_with_passphrase(&dir, PASSPHRASE);
        assert!(matches!(result, Err(Error::VaultMismatch)));
        assert!(!dir.join(FORK_MARKER).exists());
        assert_eq!(
            fs::read(dir.join(format!("{FORK_MARKER}.rejected"))).unwrap(),
            other_header
        );
    }

    #[test]
    fn litter_marker_removed_on_clean_open() {
        let (tmp, vault, _recovery) = new_vault_with_note();
        let dir = tmp.path().join("v");
        vault.lock();

        // No mismatch behind it: the marker is just litter.
        fs::write(dir.join(FORK_MARKER), b"stale").unwrap();
        let copy = Vault::open_with_passphrase(&dir, PASSPHRASE).unwrap();
        assert!(!dir.join(FORK_MARKER).exists());
        assert!(!dir.join(format!("{FORK_MARKER}.rejected")).exists());
        copy.lock();
    }

    #[test]
    fn fork_leaves_no_marker() {
        let (tmp, mut vault, _recovery) = new_vault_with_note();
        let dir = tmp.path().join("v");
        let before = vault.header().vault_id.clone();
        vault.fork_identity(PASSPHRASE).unwrap();
        let after = vault.header().vault_id.clone();
        assert_ne!(after, before);
        assert!(!dir.join(FORK_MARKER).exists());
        vault.lock();

        let copy = Vault::open_with_passphrase(&dir, PASSPHRASE).unwrap();
        assert_eq!(copy.header().vault_id, after);
        copy.lock();
    }

    #[test]
    fn copied_vault_keeps_attachments() {
        let (tmp, mut vault, _recovery) = new_vault_with_note();
        let entry = vault
            .create_entry(&crate::entries::EntryDraft {
                entry_date: "2026-10-15".into(),
                title: Some("with file".into()),
                body_md: "see attachment".into(),
                ..Default::default()
            })
            .unwrap();
        let att = vault
            .add_attachment(&entry.id, "note.txt", "text/plain", b"attachment bytes")
            .unwrap();
        vault.checkpoint().unwrap();
        vault.lock();

        let dest = tmp.path().join("copy");
        copy_vault_files(&tmp.path().join("v"), &dest).unwrap();

        let copy = Vault::open_with_passphrase(&dest, PASSPHRASE).unwrap();
        assert_eq!(copy.read_attachment(&att.id).unwrap(), b"attachment bytes");
        assert!(copy.verify().unwrap().problems.is_empty());
        copy.lock();
    }
}

/// Mints a fresh device id (UUIDv7, canonical form).
/// The caller keeps it — in the app's settings, not
/// in the vault.
pub fn new_device_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

fn check_device_id(id: &str) -> Result<()> {
    if crate::storage::is_canonical_uuid(id) {
        Ok(())
    } else {
        Err(Error::InvalidInput(
            "device id is not a canonical UUID".into(),
        ))
    }
}
