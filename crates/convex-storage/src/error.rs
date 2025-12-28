//! Storage error types.

use thiserror::Error;

/// Storage operation result type.
pub type StorageResult<T> = Result<T, StorageError>;

/// Storage error types.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Database error from the underlying storage engine.
    #[error("Database error: {0}")]
    Database(String),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization error.
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// Record not found.
    #[error("Record not found: {entity_type} with key '{key}'")]
    NotFound {
        /// The type of entity (e.g., "Security", "Curve", "Quote").
        entity_type: &'static str,
        /// The key that was not found.
        key: String,
    },

    /// Duplicate key error.
    #[error("Duplicate key: {entity_type} with key '{key}' already exists")]
    DuplicateKey {
        /// The type of entity.
        entity_type: &'static str,
        /// The duplicate key.
        key: String,
    },

    /// Version conflict for optimistic locking.
    #[error("Version conflict: expected version {expected}, found {actual}")]
    VersionConflict {
        /// Expected version.
        expected: u64,
        /// Actual version found.
        actual: u64,
    },

    /// Transaction error.
    #[error("Transaction error: {0}")]
    Transaction(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Storage not initialized.
    #[error("Storage not initialized")]
    NotInitialized,

    /// Storage is read-only.
    #[error("Storage is read-only")]
    ReadOnly,

    /// Feature not implemented.
    #[error("Feature not implemented: {0}")]
    NotImplemented(String),
}

impl From<redb::Error> for StorageError {
    fn from(err: redb::Error) -> Self {
        StorageError::Database(err.to_string())
    }
}

impl From<redb::DatabaseError> for StorageError {
    fn from(err: redb::DatabaseError) -> Self {
        StorageError::Database(err.to_string())
    }
}

impl From<redb::TableError> for StorageError {
    fn from(err: redb::TableError) -> Self {
        StorageError::Database(err.to_string())
    }
}

impl From<redb::TransactionError> for StorageError {
    fn from(err: redb::TransactionError) -> Self {
        StorageError::Transaction(err.to_string())
    }
}

impl From<redb::CommitError> for StorageError {
    fn from(err: redb::CommitError) -> Self {
        StorageError::Transaction(err.to_string())
    }
}

impl From<redb::StorageError> for StorageError {
    fn from(err: redb::StorageError) -> Self {
        StorageError::Database(err.to_string())
    }
}

impl From<redb::CompactionError> for StorageError {
    fn from(err: redb::CompactionError) -> Self {
        StorageError::Database(err.to_string())
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(err: serde_json::Error) -> Self {
        if err.is_data() {
            StorageError::Deserialization(err.to_string())
        } else {
            StorageError::Serialization(err.to_string())
        }
    }
}
