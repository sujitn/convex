//! Error types for bond operations.

use thiserror::Error;

/// A specialized Result type for bond operations.
pub type BondResult<T> = Result<T, BondError>;

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
