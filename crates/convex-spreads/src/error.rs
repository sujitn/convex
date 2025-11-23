//! Error types for spread calculations.

use thiserror::Error;

/// Result type for spread operations.
pub type SpreadResult<T> = Result<T, SpreadError>;

/// Errors that can occur during spread calculations.
#[derive(Debug, Error)]
pub enum SpreadError {
    /// Spread calculation failed to converge.
    #[error("Spread calculation failed to converge after {iterations} iterations")]
    ConvergenceFailed {
        /// Number of iterations attempted.
        iterations: u32,
    },

    /// Invalid input for spread calculation.
    #[error("Invalid input: {reason}")]
    InvalidInput {
        /// Reason for the invalid input.
        reason: String,
    },

    /// Curve error during spread calculation.
    #[error("Curve error: {0}")]
    CurveError(String),

    /// Bond error during spread calculation.
    #[error("Bond error: {0}")]
    BondError(String),

    /// Settlement date is after maturity.
    #[error("Settlement date {settlement} is after maturity {maturity}")]
    SettlementAfterMaturity {
        /// Settlement date.
        settlement: String,
        /// Maturity date.
        maturity: String,
    },
}

impl SpreadError {
    /// Creates a new convergence failed error.
    #[must_use]
    pub fn convergence_failed(iterations: u32) -> Self {
        Self::ConvergenceFailed { iterations }
    }

    /// Creates a new invalid input error.
    #[must_use]
    pub fn invalid_input(reason: impl Into<String>) -> Self {
        Self::InvalidInput {
            reason: reason.into(),
        }
    }

    /// Creates a new curve error.
    #[must_use]
    pub fn curve_error(msg: impl Into<String>) -> Self {
        Self::CurveError(msg.into())
    }

    /// Creates a new bond error.
    #[must_use]
    pub fn bond_error(msg: impl Into<String>) -> Self {
        Self::BondError(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = SpreadError::convergence_failed(100);
        assert!(err.to_string().contains("100"));
    }
}
