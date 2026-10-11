//! The encrypted SQLite database (`soirvu.db`), using SQLCipher.

use std::io;
use std::path::Path;

use data_encoding::HEXLOWER;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use zeroize::Zeroizing;

use crate::crypto::Key;
use crate::error::{Error, Result};

/// Stored in SQLite's `user_version`. Later changes to the tables bump it,
/// and a migration brings older databases up to date.
pub const SCHEMA_VERSION: i64 = 1;

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Creates a new encrypted database file and writes the first table.
    /// Refuses to touch a file that already exists.
    pub fn create(path: &Path, key: &Key, vault_id: &str) -> Result<Database> {
        if path.exists() {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "database file already exists",
            )));
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(path, flags)?;
        apply_key(&conn, key)?;
        configure(&conn)?;

        conn.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('vault_id', ?1), ('created_by_app_version', ?2)",
            params![vault_id, env!("CARGO_PKG_VERSION")],
        )?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Database { conn })
    }

    /// Opens an existing database. Never creates a file: a missing file is an error.
    pub fn open(path: &Path, key: &Key) -> Result<Database> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(path, flags)?;
        apply_key(&conn, key)?;

        // SQLite does not check the key when it is set; the first real read does.
        // With a wrong key this fails with "file is not a database".
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })?;
        configure(&conn)?;

        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version != SCHEMA_VERSION {
            return Err(Error::UnsupportedFormat(format!(
                "database schema version {version}"
            )));
        }
        Ok(Database { conn })
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

    /// The raw connection, for repositories that will be added later.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

/// `PRAGMA key` must be the first statement on a connection.
/// The `x'...'` form hands SQLCipher the raw 32-byte key, so it does not run its
/// own (slower) password-to-key step; Argon2id already did the slow part.
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
         PRAGMA cipher_memory_security = ON;",
    )?;
    Ok(())
}
