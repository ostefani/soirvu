//! Error codes across the IPC boundary.
//!
//! Commands return `Result<T, String>` where the string is a stable
//! code ("locked", "wrong_passphrase", "tag_exists", ...). The UI
//! maps codes to its own messages. Core error text never crosses:
//! it is written for developers, and the UI must not depend on it
//! (core errors never contain diary text, but the contract here is
//! codes, not prose).

use soirvu_core::Error;

/// A bridge error: either a stable code for the UI, or backoff
/// information after failed unlocks (§6.2).
#[derive(Debug)]
pub enum AppError {
    Code(String),
    Backoff(u64),
    /// A message that is safe and useful as-is (never diary text).
    Other(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    /// The exact string the command returns to the webview.
    pub fn to_ipc(&self) -> String {
        match self {
            AppError::Code(code) => code.clone(),
            AppError::Backoff(secs) => format!("backoff:{secs}"),
            AppError::Other(msg) => msg.clone(),
        }
    }
}

impl From<Error> for AppError {
    fn from(err: Error) -> Self {
        use Error::*;
        let code = match &err {
            // One code for a wrong passphrase AND a wrong recovery
            // key: the core cannot tell them apart, and the UI must
            // not pretend it can.
            Unlock => "wrong_passphrase",
            EmptyPassphrase => "empty_passphrase",
            InvalidRecoveryKey(_) => "invalid_recovery_key",
            UnsupportedFormat(_) => "unsupported_version",
            InvalidHeader(_) | CorruptAttachment(_) => "corrupt",
            BadKdfParams(_) => "bad_kdf",
            FolderNotEmpty => "folder_not_empty",
            VaultMismatch => "vault_mismatch",
            EntryNotFound => "entry_not_found",
            AttachmentNotFound => "attachment_not_found",
            TagExists => "tag_exists",
            TagNotFound => "tag_not_found",
            QueryTooShort => "query_too_short",
            InvalidInput(_) => "invalid_input",
            Sync(_) => "sync",
            Io(_) | Database(_) => "storage",
            Random(_) | Json(_) => "internal",
        };
        AppError::Code(code.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(_: std::io::Error) -> Self {
        AppError::Code("io".into())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(_: serde_json::Error) -> Self {
        AppError::Code("internal".into())
    }
}

/// Converts a command's result to the IPC result type.
pub fn ipc<T>(result: AppResult<T>) -> Result<T, String> {
    result.map_err(|e| e.to_ipc())
}
