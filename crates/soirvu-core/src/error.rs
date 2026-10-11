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

pub type Result<T> = std::result::Result<T, Error>;
