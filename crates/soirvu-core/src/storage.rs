// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Olha Stefanishyna
//! The encrypted SQLite database (`soirvu.db`), using SQLCipher, and its schema.

use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use data_encoding::HEXLOWER;
use rusqlite::config::DbConfig;
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

/// Logical folders, entry membership, and the title search key (step 2
/// of the folders-and-links plan). Root is represented by NULL, not a
/// folder row. Membership and parent references intentionally have no
/// hard foreign keys: sync can deliver children before parents.
const MIGRATION_5_FOLDERS: &str = "
CREATE TABLE folder (
  id         TEXT PRIMARY KEY,
  parent_id  TEXT,
  name       TEXT NOT NULL,
  name_key   TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER,
  rev        INTEGER NOT NULL,
  device_id  TEXT NOT NULL REFERENCES device(id),
  lseq       INTEGER NOT NULL,
  base_rev   INTEGER NOT NULL DEFAULT 0,
  base_device TEXT NOT NULL DEFAULT ''
);
CREATE INDEX folder_parent_idx ON folder(parent_id) WHERE deleted_at IS NULL;
CREATE INDEX folder_lseq_idx ON folder(lseq);
ALTER TABLE entry ADD COLUMN folder_id TEXT;
CREATE INDEX entry_folder_date_idx
  ON entry(folder_id, entry_date DESC, created_at DESC, id DESC)
  WHERE deleted_at IS NULL;
ALTER TABLE entry ADD COLUMN title_key TEXT;
CREATE INDEX entry_title_key_idx ON entry(title_key) WHERE deleted_at IS NULL;
INSERT INTO meta (key, value) VALUES ('sync_epoch', '1'), ('bootstrap_pending', '1');
";

// Write guards preserve table layouts and the external-content FTS index.
const MIGRATION_6_ROW_INVARIANTS: &str = "
CREATE TRIGGER entry_invariants_insert BEFORE INSERT ON entry
WHEN typeof(NEW.rev) != 'integer' OR NEW.rev < 1 OR typeof(NEW.base_rev) != 'integer' OR NEW.base_rev < 0
BEGIN
  SELECT RAISE(ABORT, 'invalid entry row');
END;
CREATE TRIGGER entry_invariants_update BEFORE UPDATE OF rev, base_rev ON entry
WHEN typeof(NEW.rev) != 'integer' OR NEW.rev < 1 OR typeof(NEW.base_rev) != 'integer' OR NEW.base_rev < 0
BEGIN
  SELECT RAISE(ABORT, 'invalid entry row');
END;
CREATE TRIGGER folder_invariants_insert BEFORE INSERT ON folder
WHEN typeof(NEW.rev) != 'integer' OR NEW.rev < 1 OR typeof(NEW.base_rev) != 'integer' OR NEW.base_rev < 0
BEGIN
  SELECT RAISE(ABORT, 'invalid folder row');
END;
CREATE TRIGGER folder_invariants_update BEFORE UPDATE OF rev, base_rev ON folder
WHEN typeof(NEW.rev) != 'integer' OR NEW.rev < 1 OR typeof(NEW.base_rev) != 'integer' OR NEW.base_rev < 0
BEGIN
  SELECT RAISE(ABORT, 'invalid folder row');
END;
CREATE TRIGGER attachment_invariants_insert BEFORE INSERT ON attachment
WHEN typeof(NEW.size) != 'integer' OR NEW.size < 0 OR typeof(NEW.sha256) != 'blob' OR length(NEW.sha256) != 32
BEGIN
  SELECT RAISE(ABORT, 'invalid attachment row');
END;
CREATE TRIGGER attachment_invariants_update BEFORE UPDATE OF size, sha256 ON attachment
WHEN typeof(NEW.size) != 'integer' OR NEW.size < 0 OR typeof(NEW.sha256) != 'blob' OR length(NEW.sha256) != 32
BEGIN
  SELECT RAISE(ABORT, 'invalid attachment row');
END;
";

#[derive(Clone, Copy)]
pub(crate) struct Migration {
    sql: &'static str,
    preflight: Option<fn(&Connection) -> Result<()>>,
}

impl Migration {
    pub(crate) const fn new(sql: &'static str) -> Self {
        Self { sql, preflight: None }
    }
}

pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration::new(MIGRATION_1_META),
    Migration::new(MIGRATION_2_ENTRIES),
    Migration::new(MIGRATION_3_ATTACHMENTS),
    Migration::new(MIGRATION_4_SYNC),
    Migration::new(MIGRATION_5_FOLDERS),
    Migration { sql: MIGRATION_6_ROW_INVARIANTS, preflight: Some(check_row_invariants) },
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
    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    /// The raw connection (write access; needed to start transactions).
    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

pub(crate) fn create_with(
    path: &Path,
    key: &Key,
    vault_id: &str,
    migrations: &[Migration],
) -> Result<Database> {
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
    harden(&conn)?;
    configure(&conn)?;

    migrate(&mut conn, migrations, 0)?;
    // A fresh vault has no history to bootstrap: the migration's pending
    // flag applies to upgrades only. The epoch stays: this is a v2 vault.
    conn.execute(
        "DELETE FROM meta WHERE key = 'bootstrap_pending'",
        [],
    )?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('vault_id', ?1), ('created_by_app_version', ?2)",
        params![vault_id, env!("CARGO_PKG_VERSION")],
    )?;
    Ok(Database { conn })
}

pub(crate) fn open_with(
    path: &Path,
    key: &Key,
    backup_dir: &Path,
    migrations: &[Migration],
) -> Result<Database> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    silence_sqlite_log();
    let mut conn = Connection::open_with_flags(path, flags)?;
    apply_key(&conn, key)?;
    harden(&conn)?;

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
        // The folders migration only creates columns; titles that
        // predate it get their search keys here. Idempotent: only
        // rows the backfill has not reached are touched, so a crash
        // between migrate and backfill simply reruns it.
        if version < 5 {
            backfill_title_keys(&mut conn)?;
        }
    }
    Ok(Database { conn })
}

/// Fills `title_key` for rows written before the folders migration,
/// using the same normalizer as live writes (see `entries`).
fn backfill_title_keys(conn: &mut Connection) -> Result<()> {
    let rows: Vec<(i64, Option<String>)> = {
        let mut stmt = conn.prepare(
            "SELECT rid, title FROM entry
              WHERE title_key IS NULL AND title IS NOT NULL",
        )?;
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let tx = conn.transaction()?;
    for (rid, title) in &rows {
        let key = crate::entries::normalize_key(title.as_deref().unwrap_or(""));
        tx.execute("UPDATE entry SET title_key = ?1 WHERE rid = ?2", params![key, rid])?;
    }
    tx.commit()?;
    Ok(())
}

fn check_row_invariants(conn: &Connection) -> Result<()> {
    let invalid: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM entry
             WHERE typeof(rev) != 'integer' OR rev < 1 OR typeof(base_rev) != 'integer' OR base_rev < 0)
          OR EXISTS(SELECT 1 FROM folder
             WHERE typeof(rev) != 'integer' OR rev < 1 OR typeof(base_rev) != 'integer' OR base_rev < 0)
          OR EXISTS(SELECT 1 FROM attachment
             WHERE typeof(size) != 'integer' OR size < 0 OR typeof(sha256) != 'blob' OR length(sha256) != 32)",
        [],
        |row| row.get(0),
    )?;
    if invalid {
        return Err(Error::InvalidInput(
            "vault has invalid revision or attachment metadata; restore a verified backup before upgrading".into(),
        ));
    }
    Ok(())
}

/// Runs the migrations after `from`, each in its own transaction.
fn migrate(conn: &mut Connection, migrations: &[Migration], from: i64) -> Result<()> {
    for (index, migration) in migrations.iter().enumerate().skip(from as usize) {
        let version = index as i64 + 1;
        // Keep the check and installation in the same write transaction,
        // so another connection cannot change the rows between them.
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some(preflight) = migration.preflight {
            preflight(&tx)?;
        }
        tx.execute_batch(migration.sql)?;
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
    let busy: i64 = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
    if busy != 0 {
        return Err(Error::Io(io::Error::other("vault backup checkpoint is busy")));
    }
    fs::create_dir_all(backup_dir)?;
    let backup = crate::fs_boundary::Directory::open(backup_dir)?;
    let name = format!("soirvu-v{from}-before-v{to}-{}.db", uuid::Uuid::now_v7());
    let mut source = fs::File::open(path)?;
    backup.publish(Path::new(&name), |file| Ok(io::copy(&mut source, file)?))?;
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
        let path = dest.to_str()
            .ok_or_else(|| Error::InvalidInput("database export path must be UTF-8".into()))?;
        fs::OpenOptions::new().write(true).create_new(true).open(dest)?;
        {
            let pre = Connection::open_with_flags(
                dest,
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            apply_key(&pre, key)?;
            harden(&pre)?;
            pre.execute_batch(
                "CREATE TABLE _soirvu_export (x); DROP TABLE _soirvu_export;",
            )?;
        }
        let hex = Zeroizing::new(HEXLOWER.encode(&key[..]));
        let raw_key = Zeroizing::new(format!("x'{}'", hex.as_str()));
        // The export carries schema and data but not the user_version stamp.
        let conn = self.connection();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        conn.execute("ATTACH DATABASE ?1 AS rotated KEY ?2", params![path, raw_key.as_str()])?;
        let exported = conn.query_row("SELECT sqlcipher_export('rotated')", [], |_| Ok(()));
        let detached = conn.execute_batch("DETACH DATABASE rotated");
        exported?;
        detached?;
        let dest_conn = Connection::open_with_flags(
            dest,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        apply_key(&dest_conn, key)?;
        harden(&dest_conn)?;
        dest_conn.execute_batch(&format!("PRAGMA user_version = {version};"))?;
        Ok(())
    }
}

/// Set the raw key before any SQL reads. Argon2id already derived it.
fn apply_key(conn: &Connection, key: &Key) -> Result<()> {
    let hex = Zeroizing::new(HEXLOWER.encode(&key[..]));
    let sql = Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", hex.as_str()));
    conn.execute_batch(&sql)?;
    Ok(())
}

fn harden(conn: &Connection) -> Result<()> {
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)?;
    conn.execute_batch("PRAGMA cipher_log_level = 'NONE';")?;
    Ok(())
}

fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA cipher_memory_security = ON;",
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

    #[test]
    fn production_connections_harden_schema_access() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("vault.db");
        let db = Database::create(&path, &key(), "vault").unwrap();
        let conn = db.connection();
        assert!(conn.db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE).unwrap());
        assert!(!conn.db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA).unwrap());
        assert!(conn.execute("UPDATE sqlite_schema SET sql = NULL", []).is_err());
        drop(db);
        let db = Database::open(&path, &key(), tmp.path()).unwrap();
        assert!(db.connection().db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE).unwrap());
        assert!(!db.connection().db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA).unwrap());
    }

    #[test]
    fn row_guard_upgrade_preserves_valid_rows_and_rejects_new_invalid_metadata() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("vault.db");
        let backup = tmp.path().join("backup");
        let db = create_with(&path, &key(), "vault", &MIGRATIONS[..5]).unwrap();
        db.connection().execute_batch(
            "INSERT INTO device VALUES ('d', 'device', 0);
             INSERT INTO entry (id, entry_date, created_at, updated_at, device_id, lseq, body_md)
               VALUES ('e', '2026-10-01', 0, 0, 'd', 1, 'searchable note');
             INSERT INTO attachment (id, entry_id, name, mime, size, sha256, created_at, lseq)
               VALUES ('a', 'e', 'note', 'text/plain', 0, zeroblob(32), 0, 2);",
        ).unwrap();
        drop(db);
        let db = Database::open(&path, &key(), &backup).unwrap();
        assert_eq!(user_version(&db), SCHEMA_VERSION);
        let backups = backup_files(&backup);
        assert_eq!(backups.len(), 1);
        let restored = open_with(&backups[0], &key(), &backup, &MIGRATIONS[..5]).unwrap();
        assert_eq!(user_version(&restored), 5);
        assert_eq!(
            restored.connection().query_row("SELECT size FROM attachment", [], |row| row.get::<_, i64>(0)).unwrap(),
            0,
        );
        let conn = db.connection();
        for sql in [
            "UPDATE entry SET rev = 0",
            "UPDATE entry SET base_rev = -1",
            "UPDATE attachment SET size = -1",
            "UPDATE attachment SET sha256 = zeroblob(31)",
        ] {
            assert!(conn.execute(sql, []).is_err(), "{sql}");
        }
        assert_eq!(conn.query_row("SELECT count(*) FROM entry_fts WHERE entry_fts MATCH 'searchable'",
            [], |row| row.get::<_, i64>(0)).unwrap(), 1);
    }

    #[test]
    fn row_guard_upgrade_refuses_invalid_legacy_rows_without_rewriting_them() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("vault.db");
        let backup = tmp.path().join("backup");
        let db = create_with(&path, &key(), "vault", &MIGRATIONS[..5]).unwrap();
        db.connection().execute_batch(
            "INSERT INTO device VALUES ('d', 'device', 0);
             INSERT INTO entry (id, entry_date, created_at, updated_at, device_id, lseq, rev)
               VALUES ('e', '2026-10-01', 0, 0, 'd', 1, 0);",
        ).unwrap();
        drop(db);
        assert!(matches!(Database::open(&path, &key(), &backup), Err(Error::InvalidInput(_))));
        let db = open_with(&path, &key(), &backup, &MIGRATIONS[..5]).unwrap();
        assert_eq!(user_version(&db), 5);
        assert_eq!(db.connection().query_row("SELECT rev FROM entry", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(backup_files(&backup).len(), 1);
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
        let broken = &[
            Migration::new(MIGRATION_1_META),
            Migration::new("CREATE TABLE half_done (x); THIS IS NOT SQL;"),
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
