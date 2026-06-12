//! Error types for trait operations.

use thiserror::Error;

/// Common error type for trait operations.
#[derive(Debug, Error)]
pub enum TraitError {
    /// Source not available
    #[error("source not available: {0}")]
    SourceNotAvailable(String),

    /// Parse/deserialization error
    #[error("parse error: {0}")]
    ParseError(String),

    /// Serialization error
    #[error("serialization error: {0}")]
    SerializationError(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(String),

    /// Database error
    #[error("database error: {0}")]
    DatabaseError(String),
}

impl From<std::io::Error> for TraitError {
    fn from(e: std::io::Error) -> Self {
        TraitError::IoError(e.to_string())
    }
}
