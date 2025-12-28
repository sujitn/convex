//! CLI error types.

use thiserror::Error;

/// CLI error type.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum CliError {
    /// Invalid date format.
    #[error("Invalid date format: {0}. Use YYYY-MM-DD.")]
    InvalidDate(String),

    /// Invalid coupon rate.
    #[error("Invalid coupon rate: {0}. Must be between 0 and 100.")]
    InvalidCoupon(f64),

    /// Invalid yield.
    #[error("Invalid yield: {0}. Must be between -10 and 100.")]
    InvalidYield(f64),

    /// Invalid price.
    #[error("Invalid price: {0}. Must be positive.")]
    InvalidPrice(f64),

    /// Missing required argument.
    #[error("Missing required argument: {0}")]
    MissingArgument(String),

    /// Calculation error.
    #[error("Calculation error: {0}")]
    Calculation(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),
}

/// CLI result type.
pub type CliResult<T> = Result<T, CliError>;
