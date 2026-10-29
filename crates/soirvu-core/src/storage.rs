// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! The encrypted SQLite database (`soirvu.db`), using SQLCipher, and its schema.

use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use data_encoding::HEXLOWER;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::crypto::Key;
use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// Schema and migrations.
//
// The database stores its version in SQLite's `user_version`. MIGRATIONS is an
// ordered list: entry number N (counting from 1) upgrades a database from
// version N-1 to version N. NEVER edit a migration that has shipped; add a new
// one at the end instead. Each migration runs in its own transaction, so a
// failure leaves the database exactly as it was.
// ---------------------------------------------------------------------------

const MIGRATION_1_META: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

const MIGRATION_2_ENTRIES: &str = "
CREATE TABLE device (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE entry (
  rid        INTEGER PRIMARY KEY,
  id         TEXT NOT NULL UNIQUE,
  entry_date TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER,
  rev        INTEGER NOT NULL DEFAULT 1,
  device_id  TEXT NOT NULL REFERENCES device(id),
  lseq       INTEGER NOT NULL,
  title      TEXT,
  body_md    TEXT,
  mood       INTEGER,
  tz         TEXT
);
CREATE INDEX entry_date_idx ON entry(entry_date DESC, created_at DESC, id DESC)
  WHERE deleted_at IS NULL;
CREATE INDEX entry_lseq_idx ON entry(lseq);

CREATE TABLE tag (
  id       INTEGER PRIMARY KEY,
  name     TEXT NOT NULL,
  name_key TEXT NOT NULL UNIQUE
);
CREATE TABLE entry_tag (
  entry_rid INTEGER NOT NULL REFERENCES entry(rid) ON DELETE CASCADE,
  tag_id    INTEGER NOT NULL REFERENCES tag(id),
  PRIMARY KEY (entry_rid, tag_id)
);
CREATE INDEX entry_tag_tag_idx ON entry_tag(tag_id);

CREATE VIRTUAL TABLE entry_fts USING fts5(
  title, body_md,
  content='entry', content_rowid='rid',
  tokenize='trigram'
);
CREATE TRIGGER entry_fts_insert AFTER INSERT ON entry BEGIN
  INSERT INTO entry_fts(rowid, title, body_md) VALUES (new.rid, new.title, new.body_md);
END;
CREATE TRIGGER entry_fts_delete AFTER DELETE ON entry BEGIN
  INSERT INTO entry_fts(entry_fts, rowid, title, body_md)
    VALUES ('delete', old.rid, old.title, old.body_md);
END;
CREATE TRIGGER entry_fts_update AFTER UPDATE OF title, body_md ON entry BEGIN
  INSERT INTO entry_fts(entry_fts, rowid, title, body_md)
    VALUES ('delete', old.rid, old.title, old.body_md);
  INSERT INTO entry_fts(rowid, title, body_md) VALUES (new.rid, new.title, new.body_md);
END;

INSERT INTO meta (key, value) VALUES ('lseq', '0');
";

const MIGRATION_3_ATTACHMENTS: &str = "
CREATE TABLE attachment (
  rid        INTEGER PRIMARY KEY,
  id         TEXT NOT NULL UNIQUE,
  entry_id   TEXT NOT NULL REFERENCES entry(id),
  name       TEXT NOT NULL,
  mime       TEXT NOT NULL,
  size       INTEGER NOT NULL,
  sha256     BLOB NOT NULL,
  created_at INTEGER NOT NULL,
  deleted_at INTEGER,
  lseq       INTEGER NOT NULL
);
CREATE INDEX attachment_entry_idx ON attachment(entry_id) WHERE deleted_at IS NULL;
";

const MIGRATION_4_SYNC: &str = "
ALTER TABLE entry ADD COLUMN base_rev INTEGER NOT NULL DEFAULT 0;
ALTER TABLE entry ADD COLUMN base_device TEXT NOT NULL DEFAULT '';
";

const MIGRATIONS: &[&str] = &[
    MIGRATION_1_META,
    MIGRATION_2_ENTRIES,
    MIGRATION_3_ATTACHMENTS,
    MIGRATION_4_SYNC,
];

/// The schema version this build of the app understands.
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// Milliseconds since 1970-01-01 UTC.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Creates a new encrypted database file at the newest schema.
    /// Refuses to touch a file that already exists.
    pub fn create(path: &Path, key: &Key, vault_id: &str) -> Result<Database> {
        create_with(path, key, vault_id, MIGRATIONS)
    }

    /// Opens an existing database and upgrades its schema if it is older.
    /// Never creates a file: a missing file is an error.
    ///
    /// Before any upgrade the database file is copied into `backup_dir`. The copy
    /// is ciphertext under the same key, so it is as safe as the original.
    pub fn open(path: &Path, key: &Key, backup_dir: &Path) -> Result<Database> {
        open_with(path, key, backup_dir, MIGRATIONS)
    }

    /// Reads one value from the `meta` table.
    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// Returns this vault's device id, creating the device row the first time.
    ///
    /// The stored identity: the id this database was
    /// last opened (or created) with. It is a CACHE and
    /// a history — the authoritative assignment lives
    /// with the caller (the app keeps it in its own
    /// settings, outside the vault, so a copied vault
    /// folder does not carry the identity to another
    /// computer; see `assign_device`). Callers that
    /// have no settings of their own (the CLI, tests)
    /// get this stored id.
    pub fn ensure_device(&mut self) -> Result<String> {
        if let Some(id) = self.meta("device_id")? {
            return Ok(id);
        }
        let id = Uuid::now_v7().to_string();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO device (id, name, created_at) VALUES (?1, 'this device', ?2)",
            params![id, now_ms()],
        )?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('device_id', ?1)",
            [&id],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Opens this database AS the given device: the
    /// device row exists afterwards and the stored
    /// cache names it. The id itself is minted and kept
    /// by the caller, outside the vault folder.
    ///
    /// When the assigned id DIFFERS from the stored
    /// one, this database changes identity (a copied
    /// vault opened as a new device): the previous
    /// identity's device-local sync state is reset in
    /// the same transaction — the emitted watermark
    /// (the new identity's stream starts at 1, or a
    /// peer's contiguity check wedges its cursor at 0
    /// forever), the ack-ahead flag and the claimed
    /// evidence (a detector state of the old identity).
    /// The same set `restamp_device` clears. Peer
    /// cursors and seen evidence stay: they are facts
    /// about OTHER devices, and the records they count
    /// are in this database.
    pub fn assign_device(&mut self, id: &str) -> Result<String> {
        let tx = self.conn.transaction()?;
        let previous: Option<String> = tx
            .query_row("SELECT value FROM meta WHERE key = 'device_id'", [], |row| {
                row.get(0)
            })
            .optional()?;
        tx.execute(
            "INSERT OR IGNORE INTO device (id, name, created_at) VALUES (?1, 'this device', ?2)",
            params![id, now_ms()],
        )?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('device_id', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [&id],
        )?;
        if previous.is_some() && previous.as_deref() != Some(id) {
            tx.execute(
                "DELETE FROM meta WHERE key = ?1",
                [crate::sync::EMITTED_KEY],
            )?;
            tx.execute(
                "DELETE FROM meta WHERE key = ?1",
                [crate::sync::ACK_AHEAD_KEY],
            )?;
            tx.execute(
                &format!(
                    "DELETE FROM meta WHERE key LIKE '{}%'",
                    crate::sync::CLAIMED_PREFIX
                ),
                [],
            )?;
        }
        tx.commit()?;
        Ok(id.to_string())
    }

    /// The raw connection (read access).
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// The raw connection (write access; needed to start transactions).
    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

fn create_with(path: &Path, key: &Key, vault_id: &str, migrations: &[&str]) -> Result<Database> {
    if path.exists() {
        return Err(Error::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "database file already exists",
        )));
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    silence_sqlite_log();
    let mut conn = Connection::open_with_flags(path, flags)?;
    apply_key(&conn, key)?;
    configure(&conn)?;

    migrate(&mut conn, migrations, 0)?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('vault_id', ?1), ('created_by_app_version', ?2)",
        params![vault_id, env!("CARGO_PKG_VERSION")],
    )?;
    Ok(Database { conn })
}

fn open_with(path: &Path, key: &Key, backup_dir: &Path, migrations: &[&str]) -> Result<Database> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    silence_sqlite_log();
    let mut conn = Connection::open_with_flags(path, flags)?;
    apply_key(&conn, key)?;

    // SQLite does not check the key when it is set; the first real read does.
    // With a wrong key this fails with "file is not a database".
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })?;
    configure(&conn)?;

    let latest = migrations.len() as i64;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > latest {
        return Err(Error::UnsupportedFormat(format!(
            "database schema version {version} is newer than this app understands ({latest})"
        )));
    }
    if version == 0 {
        return Err(Error::UnsupportedFormat("database has no schema".into()));
    }
    if version < latest {
        backup_before_migration(&conn, path, backup_dir, version, latest)?;
        migrate(&mut conn, migrations, version)?;
    }
    Ok(Database { conn })
}

/// Runs the migrations after `from`, each in its own transaction.
fn migrate(conn: &mut Connection, migrations: &[&str], from: i64) -> Result<()> {
    for (index, sql) in migrations.iter().enumerate().skip(from as usize) {
        let version = index as i64 + 1;
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

/// Copies the database file to `backup_dir` before its schema is upgraded.
fn backup_before_migration(
    conn: &Connection,
    path: &Path,
    backup_dir: &Path,
    from: i64,
    to: i64,
) -> Result<()> {
    // Move everything still waiting in the -wal file into the main file,
    // so that copying that one file copies everything.
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    fs::create_dir_all(backup_dir)?;
    let target = backup_dir.join(format!("soirvu-v{from}-before-v{to}-{}.db", now_ms()));
    fs::copy(path, &target)?;
    // Opened writable on purpose: sync_all on a read-only
    // handle is FlushFileBuffers, which Windows refuses.
    fs::OpenOptions::new().write(true).open(&target)?.sync_all()?;
    Ok(())
}

/// SQLite reports its diagnostics through sqlite3_log, which
/// by default lands on stderr. §9: audit every such message and
/// silence the channel. The messages contain no diary content,
/// but silence is achievable, so the log callback installed
/// here discards them all. (SQLCipher's own logger does not
/// route through this callback — it is silenced separately, by
/// PRAGMA cipher_log_level in configure.) Installed once,
/// before the first connection does work.
fn silence_sqlite_log() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        unsafe extern "C" fn discard(
            _: *mut std::ffi::c_void,
            _: std::ffi::c_int,
            _: *const std::ffi::c_char,
        ) {
        }
        rusqlite::ffi::sqlite3_config(
            rusqlite::ffi::SQLITE_CONFIG_LOG,
            discard
                as unsafe extern "C" fn(
                    *mut std::ffi::c_void,
                    std::ffi::c_int,
                    *const std::ffi::c_char,
                ),
        );
    });
}

/// `PRAGMA key` must be the first statement on a connection.
/// The `x'...'` form hands SQLCipher the raw 32-byte key, so it does not run its
/// own (slower) password-to-key step; Argon2id already did the slow part.
impl Database {
    /// Copies this whole database into a new file encrypted
    /// under `key`, via SQLCipher's export: schema and data —
    /// entries, tags, attachment chunks, meta — re-encrypted
    /// page by page under the new key. The rotation engine.
    pub fn export_encrypted(&self, dest: &Path, key: &Key) -> Result<()> {
        // The caller's connection may have been opened without
        // the CREATE flag (an opened vault is read-write only),
        // and SQLite will not let such a connection create the
        // attached file. Materialize the destination first, with
        // its own short-lived connection under the new key.
        {
            let pre = Connection::open_with_flags(
                dest,
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            apply_key(&pre, key)?;
            pre.execute_batch(
                "CREATE TABLE IF NOT EXISTS _soirvu_export (x); DROP TABLE _soirvu_export;",
            )?;
        }
        let hex = Zeroizing::new(HEXLOWER.encode(&key[..]));
        let path = dest.to_string_lossy().replace('\'', "''");
        let sql = Zeroizing::new(format!(
            "ATTACH DATABASE '{path}' AS rotated KEY \"x'{}'\"; SELECT sqlcipher_export('rotated'); DETACH DATABASE rotated;",
            hex.as_str()
        ));
        // The export carries schema and data but not the
        // user_version stamp; copy it across or the result
        // reads as a database with no schema.
        let version: i64 =
            self.connection().query_row("PRAGMA user_version", [], |row| row.get(0))?;
        self.connection().execute_batch(&sql)?;
        let dest_conn = Connection::open_with_flags(
            dest,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        apply_key(&dest_conn, key)?;
        dest_conn.execute_batch(&format!("PRAGMA user_version = {version};"))?;
        Ok(())
    }
}

fn apply_key(conn: &Connection, key: &Key) -> Result<()> {
    let hex = Zeroizing::new(HEXLOWER.encode(&key[..]));
    let sql = Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", hex.as_str()));
    conn.execute_batch(&sql)?;
    Ok(())
}

fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA cipher_memory_security = ON;
         PRAGMA cipher_log_level = 'NONE';",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn key() -> Key {
        Zeroizing::new([7u8; 32])
    }

    fn user_version(db: &Database) -> i64 {
        db.connection()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }

    fn table_exists(db: &Database, name: &str) -> bool {
        db.connection()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = ?1",
                [name],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            == 1
    }

    fn backup_files(dir: &Path) -> Vec<std::path::PathBuf> {
        match fs::read_dir(dir) {
            Ok(entries) => entries.map(|e| e.unwrap().path()).collect(),
            Err(_) => vec![],
        }
    }

    #[test]
    fn a_new_database_has_the_newest_schema() {
        let tmp = TempDir::new().unwrap();
        let db = Database::create(&tmp.path().join("a.db"), &key(), "vid").unwrap();
        assert_eq!(user_version(&db), SCHEMA_VERSION);
        for table in [
            "meta",
            "device",
            "entry",
            "tag",
            "entry_tag",
            "entry_fts",
            "attachment",
        ] {
            assert!(table_exists(&db, table), "missing table {table}");
        }
        assert_eq!(db.meta("vault_id").unwrap().as_deref(), Some("vid"));
    }

    #[test]
    fn an_old_database_is_upgraded_after_a_backup() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.db");
        let backups = tmp.path().join("backups");

        // Build a database that stops at version 1, as an older app would have.
        let old = create_with(&path, &key(), "vid", &MIGRATIONS[..1]).unwrap();
        assert_eq!(user_version(&old), 1);
        assert!(!table_exists(&old, "entry"));
        drop(old);

        let db = Database::open(&path, &key(), &backups).unwrap();
        assert_eq!(user_version(&db), SCHEMA_VERSION);
        assert!(table_exists(&db, "entry"));
        assert_eq!(db.meta("vault_id").unwrap().as_deref(), Some("vid")); // old data kept
        assert_eq!(db.meta("lseq").unwrap().as_deref(), Some("0"));
        drop(db);

        // Exactly one backup, and it is encrypted.
        let files = backup_files(&backups);
        assert_eq!(files.len(), 1, "{files:?}");
        let bytes = fs::read(&files[0]).unwrap();
        assert!(!bytes.starts_with(b"SQLite format 3"));
    }

    #[test]
    fn the_backup_is_a_version_1_database_that_opens_with_the_same_key() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.db");
        let backups = tmp.path().join("backups");
        drop(create_with(&path, &key(), "vid", &MIGRATIONS[..1]).unwrap());
        drop(Database::open(&path, &key(), &backups).unwrap());

        let backup_path = backup_files(&backups).remove(0);
        let conn =
            Connection::open_with_flags(&backup_path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        apply_key(&conn, &key()).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn a_version_2_database_gains_the_attachment_table() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.db");
        let backups = tmp.path().join("backups");

        // Build a database that stops at version 2, before attachments existed.
        let old = create_with(&path, &key(), "vid", &MIGRATIONS[..2]).unwrap();
        assert_eq!(user_version(&old), 2);
        assert!(!table_exists(&old, "attachment"));
        drop(old);

        let db = Database::open(&path, &key(), &backups).unwrap();
        assert_eq!(user_version(&db), SCHEMA_VERSION);
        assert!(table_exists(&db, "attachment"));
        assert_eq!(backup_files(&backups).len(), 1); // pre-upgrade backup, as always
    }

    #[test]
    fn opening_an_up_to_date_database_makes_no_backup() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.db");
        let backups = tmp.path().join("backups");
        drop(Database::create(&path, &key(), "vid").unwrap());
        drop(Database::open(&path, &key(), &backups).unwrap());
        assert!(backup_files(&backups).is_empty());
    }

    #[test]
    fn a_database_from_a_newer_app_is_refused_untouched() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.db");
        let backups = tmp.path().join("backups");
        let db = Database::create(&path, &key(), "vid").unwrap();
        db.connection()
            .pragma_update(None, "user_version", 99)
            .unwrap();
        drop(db);

        let result = Database::open(&path, &key(), &backups);
        assert!(matches!(result, Err(Error::UnsupportedFormat(_))));
        assert!(backup_files(&backups).is_empty());
    }

    #[test]
    fn a_failing_migration_changes_nothing() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("a.db");
        let backups = tmp.path().join("backups");
        drop(create_with(&path, &key(), "vid", &MIGRATIONS[..1]).unwrap());

        // Version 2 creates a table and then hits a syntax error.
        let broken: &[&str] = &[
            MIGRATION_1_META,
            "CREATE TABLE half_done (x); THIS IS NOT SQL;",
        ];
        let result = open_with(&path, &key(), &backups, broken);
        assert!(matches!(result, Err(Error::Database(_))));

        // Reopened with the real list, the database is still at version 1 with no leftovers,
        // and then upgrades normally.
        let db = open_with(&path, &key(), &backups, &MIGRATIONS[..1]).unwrap();
        assert_eq!(user_version(&db), 1);
        assert!(!table_exists(&db, "half_done"));
        drop(db);
        let db = Database::open(&path, &key(), &backups).unwrap();
        assert_eq!(user_version(&db), SCHEMA_VERSION);
    }

    #[test]
    fn the_device_row_is_created_once() {
        let tmp = TempDir::new().unwrap();
        let mut db = Database::create(&tmp.path().join("a.db"), &key(), "vid").unwrap();
        let first = db.ensure_device().unwrap();
        let second = db.ensure_device().unwrap();
        assert_eq!(first, second);
        let rows: i64 = db
            .connection()
            .query_row("SELECT count(*) FROM device", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);
    }
}

/// A device id in its one canonical form — the form
/// `crate::new_device_id` mints. Anything else in a
/// peer position is junk from a plaintext file, not a
/// peer.
pub(crate) fn is_canonical_uuid(s: &str) -> bool {
    Uuid::parse_str(s).is_ok_and(|u| u.to_string() == s)
}
