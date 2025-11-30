//! Error types for curve operations.

use convex_core::Date;
use thiserror::Error;

/// A specialized Result type for curve operations.
pub type CurveResult<T> = Result<T, CurveError>;

/// Errors that can occur during curve operations.
#[derive(Error, Debug, Clone)]
pub enum CurveError {
    /// Curve construction failed.
    #[error("Curve construction failed: {reason}")]
    ConstructionFailed {
        /// Description of the failure.
        reason: String,
    },

    /// Requested date is outside curve range.
    #[error("Date {date} is outside curve range [{min_date}, {max_date}]")]
    DateOutOfRange {
        /// The requested date.
        date: Date,
        /// Minimum date in curve.
        min_date: Date,
        /// Maximum date in curve.
        max_date: Date,
    },

    /// Bootstrap failed to converge.
    #[error("Bootstrap failed at tenor {tenor}: {reason}")]
    BootstrapFailed {
        /// The tenor where bootstrap failed.
        tenor: String,
        /// Description of the failure.
        reason: String,
    },

    /// Invalid curve data.
    #[error("Invalid curve data: {reason}")]
    InvalidData {
        /// Description of what's invalid.
        reason: String,
    },

    /// Interpolation error.
    #[error("Interpolation failed: {reason}")]
    InterpolationFailed {
        /// Description of the failure.
        reason: String,
    },

    /// Missing reference date.
    #[error("Reference date not set")]
    MissingReferenceDate,

    /// No data points in curve.
    #[error("Curve has no data points")]
    EmptyCurve,

    /// Core library error.
    #[error("Core error: {0}")]
    CoreError(#[from] convex_core::ConvexError),

    /// Repricing validation failed.
    #[error("Repricing validation failed: {failed_count} instruments exceeded tolerance (max error: {max_error:.2e})")]
    RepricingFailed {
        /// Number of instruments that failed
        failed_count: usize,
        /// Maximum repricing error
        max_error: f64,
        /// Names of failed instruments
        failed_instruments: Vec<String>,
    },
}

impl CurveError {
    /// Creates a construction failed error.
    #[must_use]
    pub fn construction_failed(reason: impl Into<String>) -> Self {
        Self::ConstructionFailed {
            reason: reason.into(),
        }
    }

    /// Creates an invalid data error.
    #[must_use]
    pub fn invalid_data(reason: impl Into<String>) -> Self {
        Self::InvalidData {
            reason: reason.into(),
        }
    }

    /// Creates a bootstrap failed error.
    #[must_use]
    pub fn bootstrap_failed(tenor: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::BootstrapFailed {
            tenor: tenor.into(),
            reason: reason.into(),
        }
    }

    /// Creates a repricing failed error.
    #[must_use]
    pub fn repricing_failed(
        failed_count: usize,
        max_error: f64,
        failed_instruments: Vec<String>,
    ) -> Self {
        Self::RepricingFailed {
            failed_count,
            max_error,
            failed_instruments,
        }
    }
}
