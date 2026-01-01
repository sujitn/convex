//! Error types for trait operations.

use thiserror::Error;

/// Common error type for trait operations.
#[derive(Debug, Error)]
pub enum TraitError {
    /// Connection to external service failed
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    /// Subscription to data stream failed
    #[error("subscription failed: {0}")]
    SubscriptionFailed(String),

    /// Requested resource not found
    #[error("not found: {0}")]
    NotFound(String),

    /// Resource already exists
    #[error("already exists: {0}")]
    AlreadyExists(String),

    /// Source not available
    #[error("source not available: {0}")]
    SourceNotAvailable(String),

    /// Operation timed out
    #[error("timeout")]
    Timeout,

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

    /// Invalid input
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Authentication failed
    #[error("authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Permission denied
    #[error("permission denied: {0}")]
    PermissionDenied(String),

    /// Rate limited
    #[error("rate limited")]
    RateLimited,

    /// Internal error
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<std::io::Error> for TraitError {
    fn from(e: std::io::Error) -> Self {
        TraitError::IoError(e.to_string())
    }
}
