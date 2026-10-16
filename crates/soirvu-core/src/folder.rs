//! A vault folder on disk: creating it, and opening it again.
//!
//!   MyVault/
//!   ├── vault.header.json   key slots (no secrets); written last when creating
//!   └── soirvu.db           encrypted SQLite database (plus -wal / -shm while open)

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::crypto::{DeviceSecret, KdfParams, Key, MasterKey, RecoveryKey};
use crate::error::{Error, Result};
use crate::storage::Database;
use crate::vault::VaultHeader;

const HEADER_FILE: &str = "vault.header.json";
const DB_FILE: &str = "soirvu.db";
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
        match create_files(dir, passphrase, kdf) {
            Ok(created) => Ok(created),
            Err(error) => {
                // The folder was empty before, so everything in it is ours.
                remove_our_files(dir);
                Err(error)
            }
        }
    }

    pub fn open_with_passphrase(dir: &Path, passphrase: &str) -> Result<Vault> {
        let header = read_header(dir)?;
        let master = header.unlock_with_passphrase(passphrase)?;
        finish_open(dir, header, master)
    }

    pub fn open_with_recovery(dir: &Path, recovery: &RecoveryKey) -> Result<Vault> {
        let header = read_header(dir)?;
        let master = header.unlock_with_recovery(recovery)?;
        finish_open(dir, header, master)
    }

    /// Opens the vault with the device secret (Phase 8). The
    /// caller fetched the secret from the OS keychain, which
    /// is where the biometrics were checked — the core only
    /// sees bytes, and a wrong or orphaned secret is
    /// Error::Unlock, like any other bad credential.
    pub fn open_with_device(dir: &Path, secret: &DeviceSecret) -> Result<Vault> {
        let header = read_header(dir)?;
        let master = header.unlock_with_device(secret)?;
        finish_open(dir, header, master)
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
    pub fn change_passphrase(
        &mut self,
        current_passphrase: &str,
        new_passphrase: &str,
        kdf: KdfParams,
    ) -> Result<()> {
        self.header.unlock_with_passphrase(current_passphrase)?;
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
    pub fn fork_identity(&mut self, passphrase: &str) -> Result<RecoveryKey> {
        let (header, recovery) = self.header.fork(passphrase, &self.master)?;
        self.db.connection().execute(
            "UPDATE meta SET value = ?1 WHERE key = 'vault_id'",
            rusqlite::params![header.vault_id],
        )?;
        write_atomic(&self.dir.join(HEADER_FILE), &header.to_json()?)?;
        self.header = header;
        Ok(recovery)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn header(&self) -> &VaultHeader {
        &self.header
    }

    pub fn database(&self) -> &Database {
        &self.db
    }

    /// Root key for attachment files (used in a later step).
    pub fn attachment_key(&self) -> Key {
        self.master.attachment_key()
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

fn create_files(dir: &Path, passphrase: &str, kdf: KdfParams) -> Result<(Vault, RecoveryKey)> {
    let (header, recovery, master) = VaultHeader::create(passphrase, kdf)?;
    let mut db = Database::create(&dir.join(DB_FILE), &master.db_key(), &header.vault_id)?;
    let device_id = db.ensure_device()?;
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

fn finish_open(dir: &Path, header: VaultHeader, master: MasterKey) -> Result<Vault> {
    let mut db = Database::open(&dir.join(DB_FILE), &master.db_key(), &dir.join(BACKUPS_DIR))?;
    // Belt and braces: the database must say it belongs to this header.
    if db.meta("vault_id")?.as_deref() != Some(header.vault_id.as_str()) {
        return Err(Error::VaultMismatch);
    }
    let device_id = db.ensure_device()?;
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
            .change_passphrase("not the passphrase", "second passphrase", fast())
            .expect_err("the current passphrase is the proof");
        vault.change_passphrase(PASSPHRASE, "second passphrase", fast()).unwrap();

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
}
