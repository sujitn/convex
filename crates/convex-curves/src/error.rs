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
}
