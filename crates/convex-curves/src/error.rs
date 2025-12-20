//! Error types for curve operations.
//!
//! This module provides comprehensive error handling for curve construction,
//! interpolation, calibration, and value conversion operations.

use convex_core::types::Date;
use thiserror::Error;

/// A specialized Result type for curve operations.
pub type CurveResult<T> = Result<T, CurveError>;

/// Error types for curve operations.
#[derive(Error, Debug, Clone)]
pub enum CurveError {
    /// Requested tenor is outside the curve's valid range.
    #[error("Tenor {requested:.4} out of range [{min:.4}, {max:.4}]")]
    TenorOutOfRange {
        /// The requested tenor in years.
        requested: f64,
        /// Minimum valid tenor.
        min: f64,
        /// Maximum valid tenor.
        max: f64,
    },

    /// Curve calibration failed to converge.
    #[error(
        "Calibration failed after {iterations} iterations (residual: {residual:.2e}): {message}"
    )]
    CalibrationFailure {
        /// Number of iterations attempted.
        iterations: usize,
        /// Final residual value.
        residual: f64,
        /// Description of failure.
        message: String,
    },

    /// Reference dates between curves don't match.
    #[error("Reference date mismatch: expected {expected}, got {got}")]
    ReferenceDateMismatch {
        /// Expected reference date.
        expected: Date,
        /// Actual reference date.
        got: Date,
    },

    /// Value types are incompatible for the requested operation.
    #[error("Incompatible value type: expected {expected}, got {got}")]
    IncompatibleValueType {
        /// Expected value type.
        expected: String,
        /// Actual value type.
        got: String,
    },

    /// Interpolation failed.
    #[error("Interpolation error: {reason}")]
    InterpolationError {
        /// Description of the interpolation error.
        reason: String,
    },

    /// Not enough data points for interpolation.
    #[error("Insufficient points: need at least {required}, got {got}")]
    InsufficientPoints {
        /// Minimum required points.
        required: usize,
        /// Actual number of points provided.
        got: usize,
    },

    /// Tenors are not monotonically increasing.
    #[error("Non-monotonic tenors at index {index}: {prev:.4} >= {current:.4}")]
    NonMonotonicTenors {
        /// Index where monotonicity violation occurred.
        index: usize,
        /// Previous tenor value.
        prev: f64,
        /// Current tenor value.
        current: f64,
    },

    /// Invalid calibration instrument.
    #[error("Invalid instrument: {reason}")]
    InvalidInstrument {
        /// Description of what's wrong with the instrument.
        reason: String,
    },

    /// Curve segments overlap.
    #[error("Segment overlap at tenor {tenor:.4}")]
    SegmentOverlap {
        /// Tenor where overlap occurs.
        tenor: f64,
    },

    /// No segment covers the requested tenor.
    #[error("No segment covers tenor {tenor:.4}")]
    NoSegmentCoverage {
        /// Tenor not covered by any segment.
        tenor: f64,
    },

    /// Segment gap - no segment covers the range.
    #[error("Gap in segment coverage between {from:.4} and {to:.4}")]
    SegmentGap {
        /// Start of gap.
        from: f64,
        /// End of gap.
        to: f64,
    },

    /// Invalid segment range.
    #[error("Invalid segment range: start {start:.4} >= end {end:.4}")]
    InvalidSegmentRange {
        /// Segment start.
        start: f64,
        /// Segment end.
        end: f64,
    },

    /// Conversion error between value types.
    #[error("Conversion error: {reason}")]
    ConversionError {
        /// Description of the conversion failure.
        reason: String,
    },

    /// Invalid value (NaN, Inf, or domain error).
    #[error("Invalid value: {reason}")]
    InvalidValue {
        /// Description of why value is invalid.
        reason: String,
    },

    /// Derivative not available.
    #[error("Derivative not available at t={tenor:.4}")]
    DerivativeNotAvailable {
        /// Tenor where derivative was requested.
        tenor: f64,
    },

    /// Mathematical error.
    #[error("Math error: {reason}")]
    MathError {
        /// Description of the mathematical error.
        reason: String,
    },

    /// Builder error.
    #[error("Builder error: {reason}")]
    BuilderError {
        /// Description of the builder error.
        reason: String,
    },

    /// Curve not found in environment.
    #[error("Curve not found: {name}")]
    CurveNotFound {
        /// Name/identifier of the missing curve.
        name: String,
    },
}

impl CurveError {
    /// Creates a tenor out of range error.
    #[must_use]
    pub fn tenor_out_of_range(requested: f64, min: f64, max: f64) -> Self {
        Self::TenorOutOfRange {
            requested,
            min,
            max,
        }
    }

    /// Creates a calibration failure error.
    #[must_use]
    pub fn calibration_failed(
        iterations: usize,
        residual: f64,
        message: impl Into<String>,
    ) -> Self {
        Self::CalibrationFailure {
            iterations,
            residual,
            message: message.into(),
        }
    }

    /// Creates a reference date mismatch error.
    #[must_use]
    pub fn reference_date_mismatch(expected: Date, got: Date) -> Self {
        Self::ReferenceDateMismatch { expected, got }
    }

    /// Creates an incompatible value type error.
    #[must_use]
    pub fn incompatible_value_type(expected: impl Into<String>, got: impl Into<String>) -> Self {
        Self::IncompatibleValueType {
            expected: expected.into(),
            got: got.into(),
        }
    }

    /// Creates an interpolation error.
    #[must_use]
    pub fn interpolation_error(reason: impl Into<String>) -> Self {
        Self::InterpolationError {
            reason: reason.into(),
        }
    }

    /// Creates an insufficient points error.
    #[must_use]
    pub fn insufficient_points(required: usize, got: usize) -> Self {
        Self::InsufficientPoints { required, got }
    }

    /// Creates a non-monotonic tenors error.
    #[must_use]
    pub fn non_monotonic_tenors(index: usize, prev: f64, current: f64) -> Self {
        Self::NonMonotonicTenors {
            index,
            prev,
            current,
        }
    }

    /// Creates an invalid instrument error.
    #[must_use]
    pub fn invalid_instrument(reason: impl Into<String>) -> Self {
        Self::InvalidInstrument {
            reason: reason.into(),
        }
    }

    /// Creates a segment overlap error.
    #[must_use]
    pub fn segment_overlap(tenor: f64) -> Self {
        Self::SegmentOverlap { tenor }
    }

    /// Creates a no segment coverage error.
    #[must_use]
    pub fn no_segment_coverage(tenor: f64) -> Self {
        Self::NoSegmentCoverage { tenor }
    }

    /// Creates a conversion error.
    #[must_use]
    pub fn conversion_error(reason: impl Into<String>) -> Self {
        Self::ConversionError {
            reason: reason.into(),
        }
    }

    /// Creates an invalid value error.
    #[must_use]
    pub fn invalid_value(reason: impl Into<String>) -> Self {
        Self::InvalidValue {
            reason: reason.into(),
        }
    }

    /// Creates a math error.
    #[must_use]
    pub fn math_error(reason: impl Into<String>) -> Self {
        Self::MathError {
            reason: reason.into(),
        }
    }

    /// Creates a builder error.
    #[must_use]
    pub fn builder_error(reason: impl Into<String>) -> Self {
        Self::BuilderError {
            reason: reason.into(),
        }
    }

    /// Creates a curve not found error.
    #[must_use]
    pub fn curve_not_found(name: impl Into<String>) -> Self {
        Self::CurveNotFound { name: name.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::types::Date;

    #[test]
    fn test_error_display() {
        let err = CurveError::tenor_out_of_range(15.0, 0.0, 10.0);
        let msg = format!("{}", err);
        assert!(msg.contains("15.0"));
        assert!(msg.contains("out of range"));
    }

    #[test]
    fn test_calibration_failure() {
        let err = CurveError::calibration_failed(100, 1e-6, "Failed to converge");
        let msg = format!("{}", err);
        assert!(msg.contains("100 iterations"));
        assert!(msg.contains("Failed to converge"));
    }

    #[test]
    fn test_reference_date_mismatch() {
        let d1 = Date::from_ymd(2024, 1, 1).unwrap();
        let d2 = Date::from_ymd(2024, 1, 2).unwrap();
        let err = CurveError::reference_date_mismatch(d1, d2);
        let msg = format!("{}", err);
        assert!(msg.contains("mismatch"));
    }

    #[test]
    fn test_non_monotonic_tenors() {
        let err = CurveError::non_monotonic_tenors(3, 2.0, 1.5);
        let msg = format!("{}", err);
        assert!(msg.contains("Non-monotonic"));
        assert!(msg.contains("index 3"));
    }
}
