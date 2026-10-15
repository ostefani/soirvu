//! A vault folder on disk: creating it, and opening it again.
//!
//!   MyVault/
//!   ├── vault.header.json   key slots (no secrets); written last when creating
//!   └── soirvu.db           encrypted SQLite database (plus -wal / -shm while open)

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::crypto::{KdfParams, Key, MasterKey, RecoveryKey};
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
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let mut file = File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
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
