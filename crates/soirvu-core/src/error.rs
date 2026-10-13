use std::fmt;

/// Everything that can go wrong in this crate's vault and crypto code.
#[derive(Debug)]
pub enum Error {
    /// The header says it was written by a format or algorithm suite we don't know.
    UnsupportedFormat(String),
    /// The header is structurally wrong (bad length, bad hex, no slots...).
    InvalidHeader(String),
    /// The Argon2id settings are outside the allowed range.
    BadKdfParams(String),
    /// The recovery key text could not be read.
    InvalidRecoveryKey(String),
    /// Wrong passphrase, wrong recovery key, OR a modified header.
    /// These cases are deliberately NOT told apart: telling them apart
    /// would help an attacker who is guessing.
    Unlock,
    /// Creating a vault with an empty passphrase is not allowed.
    EmptyPassphrase,
    /// A new vault can only be created in an empty folder.
    FolderNotEmpty,
    /// The header and the database are not from the same vault.
    VaultMismatch,
    /// A file could not be read or written.
    Io(std::io::Error),
    /// The SQLite/SQLCipher database reported an error.
    Database(rusqlite::Error),
    /// No (live) entry has this id.
    EntryNotFound,
    /// Something the caller supplied is not acceptable (bad date, bad tag...).
    /// The message never contains diary text.
    InvalidInput(String),
    /// No tag with this name is used by a live entry.
    TagNotFound,
    /// A rename would give a tag the name of a different tag that is in use.
    /// The caller has to choose `merge_tags` on purpose.
    TagExists,
    /// A search query has no word of at least 3 characters.
    QueryTooShort,
    /// The operating system could not give us random bytes.
    Random(String),
    /// JSON could not be read or written.
    Json(serde_json::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnsupportedFormat(s) => write!(f, "unsupported vault format: {s}"),
            Error::InvalidHeader(s) => write!(f, "invalid vault header: {s}"),
            Error::BadKdfParams(s) => write!(f, "bad key-derivation settings: {s}"),
            Error::InvalidRecoveryKey(s) => write!(f, "invalid recovery key: {s}"),
            Error::Unlock => write!(f, "could not unlock the vault"),
            Error::EmptyPassphrase => write!(f, "the passphrase must not be empty"),
            Error::FolderNotEmpty => write!(f, "the folder is not empty"),
            Error::VaultMismatch => write!(f, "the database does not belong to this vault header"),
            Error::Io(e) => write!(f, "file error: {e}"),
            Error::Database(e) => write!(f, "database error: {e}"),
            Error::EntryNotFound => write!(f, "entry not found"),
            Error::InvalidInput(s) => write!(f, "invalid input: {s}"),
            Error::TagNotFound => write!(f, "tag not found"),
            Error::TagExists => write!(f, "a tag with that name already exists"),
            Error::QueryTooShort => write!(f, "search words need at least 3 characters"),
            Error::Random(s) => write!(f, "random number generator failed: {s}"),
            Error::Json(e) => write!(f, "header JSON error: {e}"),
        }
    }
}

impl std::error::Error for Error {}

// This lets `?` turn a serde_json error into our Error automatically.
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Database(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
