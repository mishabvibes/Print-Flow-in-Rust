use serde::Serialize;

#[derive(Debug, Clone, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum EngineError {
    #[error("unknown command: {0}")]
    UnknownCommand(String),
    /// A parameter is missing or has the wrong type/range.
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Failed(String),
    #[error("{0} is not available right now")]
    Disabled(String),
    /// A bug: the command panicked and was rolled back.
    #[error("internal error: {0}")]
    Internal(String),
}

pub type EngineResult<T> = Result<T, EngineError>;

impl EngineError {
    pub fn invalid(m: impl Into<String>) -> Self {
        Self::Invalid(m.into())
    }
    pub fn failed(m: impl std::fmt::Display) -> Self {
        Self::Failed(m.to_string())
    }
}
