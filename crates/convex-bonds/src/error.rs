//! Error types for bond operations.

use thiserror::Error;

/// A specialized Result type for bond operations.
pub type BondResult<T> = Result<T, BondError>;

/// Errors that can occur during identifier validation.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum IdentifierError {
    /// Invalid length for identifier.
    #[error("Invalid {id_type} length: expected {expected}, got {actual}")]
    InvalidLength {
        /// Type of identifier (CUSIP, ISIN, etc.).
        id_type: &'static str,
        /// Expected length.
        expected: usize,
        /// Actual length.
        actual: usize,
    },

    /// Invalid check digit.
    #[error("Invalid {id_type} check digit for '{value}'")]
    InvalidCheckDigit {
        /// Type of identifier.
        id_type: &'static str,
        /// The invalid value.
        value: String,
    },

    /// Invalid character in identifier.
    #[error("Invalid character '{ch}' at position {position} in {id_type}")]
    InvalidCharacter {
        /// Type of identifier.
        id_type: &'static str,
        /// The invalid character.
        ch: char,
        /// Position in the string.
        position: usize,
    },

    /// Invalid format.
    #[error("Invalid {id_type} format: {reason}")]
    InvalidFormat {
        /// Type of identifier.
        id_type: &'static str,
        /// Reason for invalidity.
        reason: String,
    },
}

/// Errors that can occur during bond operations.
#[derive(Error, Debug, Clone)]
pub enum BondError {
    /// Invalid bond specification.
    #[error("Invalid bond specification: {reason}")]
    InvalidSpec {
        /// Description of what's invalid.
        reason: String,
    },

    /// Missing required field.
    #[error("Missing required field: {field}")]
    MissingField {
        /// The missing field name.
        field: String,
    },

    /// Pricing calculation failed.
    #[error("Pricing failed: {reason}")]
    PricingFailed {
        /// Description of the failure.
        reason: String,
    },

    /// Yield calculation failed to converge.
    #[error("Yield calculation failed to converge after {iterations} iterations")]
    YieldConvergenceFailed {
        /// Number of iterations attempted.
        iterations: u32,
    },

    /// Invalid price.
    #[error("Invalid price: {reason}")]
    InvalidPrice {
        /// Description of what's invalid.
        reason: String,
    },

    /// Cash flow generation failed.
    #[error("Cash flow generation failed: {reason}")]
    CashFlowFailed {
        /// Description of the failure.
        reason: String,
    },

    /// Settlement date is after maturity.
    #[error("Settlement date {settlement} is after maturity {maturity}")]
    SettlementAfterMaturity {
        /// Settlement date.
        settlement: String,
        /// Maturity date.
        maturity: String,
    },

    /// Invalid schedule configuration.
    #[error("Invalid schedule: {message}")]
    InvalidSchedule {
        /// Description of the schedule error.
        message: String,
    },

    /// Core library error.
    #[error("Core error: {0}")]
    CoreError(#[from] convex_core::ConvexError),

    /// Curve error.
    #[error("Curve error: {0}")]
    CurveError(#[from] convex_curves::CurveError),
}

impl BondError {
    /// Creates an invalid specification error.
    #[must_use]
    pub fn invalid_spec(reason: impl Into<String>) -> Self {
        Self::InvalidSpec {
            reason: reason.into(),
        }
    }

    /// Creates a missing field error.
    #[must_use]
    pub fn missing_field(field: impl Into<String>) -> Self {
        Self::MissingField {
            field: field.into(),
        }
    }

    /// Creates a pricing failed error.
    #[must_use]
    pub fn pricing_failed(reason: impl Into<String>) -> Self {
        Self::PricingFailed {
            reason: reason.into(),
        }
    }
}
