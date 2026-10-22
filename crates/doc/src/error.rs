use std::path::PathBuf;

/// Errors of the document model. Everything fallible returns one of these instead of panicking.
#[derive(Debug, thiserror::Error)]
pub enum DocError {
    #[error("cannot read {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("invalid data URL: {0}")]
    DataUrl(String),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

pub type DocResult<T> = Result<T, DocError>;
