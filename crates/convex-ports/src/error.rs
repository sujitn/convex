//! Error types for trait operations.

use thiserror::Error;

/// Common error type for trait operations.
#[derive(Debug, Clone, Error)]
pub enum TraitError {
    /// The request violates a declared storage precondition.
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// A version or immutable identity has already been committed with different content.
    #[error("conflict: {0}")]
    Conflict(&'static str),

    /// The adapter does not provide the requested capability or schema.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),

    /// A declared local resource bound has been reached.
    #[error("storage capacity exceeded")]
    CapacityExceeded,

    /// A commit acknowledgment failed; read the committed identity before retrying.
    #[error("unknown commit outcome")]
    UnknownOutcome,

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
