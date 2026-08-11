//! Error type shared by every Tauri command.
//!
//! Commands return `Result<T, AppError>`; `AppError` serialises to a tagged
//! object so the frontend can branch on `kind` instead of string-matching.
//! Crucially, no variant is allowed to carry decrypted secret material — error
//! messages are surfaced verbatim in the UI and written to the log file.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("serialisation failed: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("access denied: {0}. Relaunch elevated, or use the walkdir fallback.")]
    AccessDenied(String),

    #[error("this operation is only available on Windows ({0})")]
    WindowsOnly(&'static str),

    #[error("volume {0} is not NTFS — the MFT fast path needs NTFS")]
    NotNtfs(String),

    #[error("corrupt or unexpected on-disk structure: {0}")]
    Parse(String),

    #[error("no scan with id {0}")]
    UnknownScan(String),

    #[error("cryptography failed: {0}")]
    Crypto(String),

    #[error("integrity check failed: {0}")]
    Integrity(String),

    #[error("{0}")]
    Other(String),
}

impl AppError {
    fn kind(&self) -> &'static str {
        match self {
            AppError::Io(_) => "io",
            AppError::Serde(_) => "serde",
            AppError::AccessDenied(_) => "access_denied",
            AppError::WindowsOnly(_) => "windows_only",
            AppError::NotNtfs(_) => "not_ntfs",
            AppError::Parse(_) => "parse",
            AppError::UnknownScan(_) => "unknown_scan",
            AppError::Crypto(_) => "crypto",
            AppError::Integrity(_) => "integrity",
            AppError::Other(_) => "other",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("AppError", 2)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Other(format!("{e:#}"))
    }
}

pub type AppResult<T> = std::result::Result<T, AppError>;
