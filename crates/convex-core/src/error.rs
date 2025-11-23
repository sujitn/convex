//! Error types for the Convex library.
//!
//! This module defines the error types used throughout Convex,
//! providing structured error handling with context.

use rust_decimal::Decimal;
use thiserror::Error;

/// A specialized Result type for Convex operations.
pub type ConvexResult<T> = Result<T, ConvexError>;

/// The main error type for Convex operations.
#[derive(Error, Debug, Clone)]
pub enum ConvexError {
    /// Error in date calculations or invalid date.
    #[error("Invalid date: {message}")]
    InvalidDate {
        /// Description of the date error.
        message: String,
    },

    /// Error in pricing calculations.
    #[error("Pricing error: {reason}")]
    PricingError {
        /// Description of what went wrong.
        reason: String,
    },

    /// Numerical solver failed to converge.
    #[error("Convergence failed after {iterations} iterations (residual: {residual})")]
    ConvergenceFailed {
        /// Number of iterations attempted.
        iterations: u32,
        /// Final residual value.
        residual: f64,
    },

    /// Invalid yield value.
    #[error("Invalid yield: {value} - {reason}")]
    InvalidYield {
        /// The invalid yield value.
        value: Decimal,
        /// Reason for invalidity.
        reason: String,
    },

    /// Invalid price value.
    #[error("Invalid price: {value} - {reason}")]
    InvalidPrice {
        /// The invalid price value.
        value: Decimal,
        /// Reason for invalidity.
        reason: String,
    },

    /// Invalid spread value.
    #[error("Invalid spread: {value_bps} bps - {reason}")]
    InvalidSpread {
        /// The invalid spread value in basis points.
        value_bps: Decimal,
        /// Reason for invalidity.
        reason: String,
    },

    /// Curve not found or unavailable.
    #[error("Curve not found: {curve_id}")]
    CurveNotFound {
        /// Identifier of the missing curve.
        curve_id: String,
    },

    /// Curve construction failed.
    #[error("Curve construction failed: {reason}")]
    CurveConstructionFailed {
        /// Description of the failure.
        reason: String,
    },

    /// Interpolation error.
    #[error("Interpolation error at {date}: {reason}")]
    InterpolationError {
        /// Date where interpolation failed.
        date: String,
        /// Reason for the failure.
        reason: String,
    },

    /// Invalid cash flow schedule.
    #[error("Invalid cash flow: {reason}")]
    InvalidCashFlow {
        /// Description of the invalid cash flow.
        reason: String,
    },

    /// Invalid bond specification.
    #[error("Invalid bond specification: {reason}")]
    InvalidBondSpec {
        /// Description of what's invalid.
        reason: String,
    },

    /// Day count calculation error.
    #[error("Day count error: {reason}")]
    DayCountError {
        /// Description of the error.
        reason: String,
    },

    /// Calendar or business day error.
    #[error("Calendar error: {reason}")]
    CalendarError {
        /// Description of the error.
        reason: String,
    },

    /// Mathematical error (division by zero, overflow, etc.).
    #[error("Mathematical error: {reason}")]
    MathError {
        /// Description of the error.
        reason: String,
    },

    /// Configuration error.
    #[error("Configuration error: {reason}")]
    ConfigError {
        /// Description of the configuration error.
        reason: String,
    },
}

impl ConvexError {
    /// Creates an invalid date error.
    #[must_use]
    pub fn invalid_date(message: impl Into<String>) -> Self {
        Self::InvalidDate {
            message: message.into(),
        }
    }

    /// Creates a pricing error.
    #[must_use]
    pub fn pricing_error(reason: impl Into<String>) -> Self {
        Self::PricingError {
            reason: reason.into(),
        }
    }

    /// Creates a convergence failure error.
    #[must_use]
    pub fn convergence_failed(iterations: u32, residual: f64) -> Self {
        Self::ConvergenceFailed {
            iterations,
            residual,
        }
    }

    /// Creates a curve not found error.
    #[must_use]
    pub fn curve_not_found(curve_id: impl Into<String>) -> Self {
        Self::CurveNotFound {
            curve_id: curve_id.into(),
        }
    }

    /// Creates a math error.
    #[must_use]
    pub fn math_error(reason: impl Into<String>) -> Self {
        Self::MathError {
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ConvexError::invalid_date("2024-02-30 is not a valid date");
        assert!(err.to_string().contains("Invalid date"));
    }

    #[test]
    fn test_convergence_error() {
        let err = ConvexError::convergence_failed(100, 1e-6);
        assert!(err.to_string().contains("100 iterations"));
    }
}
