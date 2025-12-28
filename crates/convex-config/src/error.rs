//! Configuration error types.

use thiserror::Error;

/// Configuration operation result type.
pub type ConfigResult<T> = Result<T, ConfigError>;

/// Configuration error types.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Configuration not found.
    #[error("Configuration not found: {key}")]
    NotFound {
        /// The configuration key that was not found.
        key: String,
    },

    /// Validation error.
    #[error("Validation error: {message}")]
    Validation {
        /// Field that failed validation.
        field: String,
        /// Validation error message.
        message: String,
    },

    /// Multiple validation errors.
    #[error("Multiple validation errors: {0:?}")]
    MultipleValidationErrors(Vec<ValidationError>),

    /// Configuration conflict (e.g., circular inheritance).
    #[error("Configuration conflict: {0}")]
    Conflict(String),

    /// Invalid override - override key doesn't match any base config field.
    #[error("Invalid override: field '{field}' does not exist in configuration '{config}'")]
    InvalidOverride {
        /// The configuration being overridden.
        config: String,
        /// The field that doesn't exist.
        field: String,
    },

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization error.
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// Storage error.
    #[error("Storage error: {0}")]
    Storage(#[from] convex_storage::StorageError),

    /// Version conflict for optimistic locking.
    #[error("Version conflict: expected {expected}, found {actual}")]
    VersionConflict {
        /// Expected version.
        expected: u64,
        /// Actual version found.
        actual: u64,
    },

    /// Configuration is read-only.
    #[error("Configuration '{key}' is read-only")]
    ReadOnly {
        /// The read-only configuration key.
        key: String,
    },

    /// Feature not implemented.
    #[error("Feature not implemented: {0}")]
    NotImplemented(String),
}

/// A single validation error.
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Field that failed validation.
    pub field: String,
    /// Validation error message.
    pub message: String,
    /// Validation rule that was violated.
    pub rule: Option<String>,
}

impl ValidationError {
    /// Creates a new validation error.
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            rule: None,
        }
    }

    /// Creates a validation error with a rule name.
    pub fn with_rule(
        field: impl Into<String>,
        message: impl Into<String>,
        rule: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            rule: Some(rule.into()),
        }
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref rule) = self.rule {
            write!(f, "{}: {} (rule: {})", self.field, self.message, rule)
        } else {
            write!(f, "{}: {}", self.field, self.message)
        }
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(err: serde_json::Error) -> Self {
        if err.is_data() {
            ConfigError::Deserialization(err.to_string())
        } else {
            ConfigError::Serialization(err.to_string())
        }
    }
}

/// Trait for validatable configurations.
pub trait Validate {
    /// Validates the configuration.
    ///
    /// Returns a list of validation errors, or an empty vector if valid.
    fn validate(&self) -> Vec<ValidationError>;

    /// Returns true if the configuration is valid.
    fn is_valid(&self) -> bool {
        self.validate().is_empty()
    }

    /// Validates and returns an error if invalid.
    fn validate_or_error(&self) -> ConfigResult<()> {
        let errors = self.validate();
        if errors.is_empty() {
            Ok(())
        } else if errors.len() == 1 {
            let err = errors.into_iter().next().unwrap();
            Err(ConfigError::Validation {
                field: err.field,
                message: err.message,
            })
        } else {
            Err(ConfigError::MultipleValidationErrors(errors))
        }
    }
}
