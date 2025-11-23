//! Error types for mathematical operations.

use thiserror::Error;

/// A specialized Result type for mathematical operations.
pub type MathResult<T> = Result<T, MathError>;

/// Errors that can occur during mathematical operations.
#[derive(Error, Debug, Clone)]
pub enum MathError {
    /// Root-finding algorithm failed to converge.
    #[error("Convergence failed after {iterations} iterations (residual: {residual:.2e})")]
    ConvergenceFailed {
        /// Number of iterations attempted.
        iterations: u32,
        /// Final residual value.
        residual: f64,
    },

    /// Invalid bracket for root-finding.
    #[error("Invalid bracket: f({a}) = {fa:.2e} and f({b}) = {fb:.2e} have same sign")]
    InvalidBracket {
        /// Lower bound of bracket.
        a: f64,
        /// Upper bound of bracket.
        b: f64,
        /// Function value at a.
        fa: f64,
        /// Function value at b.
        fb: f64,
    },

    /// Division by zero or near-zero value.
    #[error("Division by zero or near-zero value: {value:.2e}")]
    DivisionByZero {
        /// The near-zero value.
        value: f64,
    },

    /// Matrix is singular (not invertible).
    #[error("Singular matrix: cannot invert")]
    SingularMatrix,

    /// Matrix dimensions are incompatible.
    #[error("Incompatible matrix dimensions: ({rows1}x{cols1}) and ({rows2}x{cols2})")]
    DimensionMismatch {
        /// Rows in first matrix.
        rows1: usize,
        /// Columns in first matrix.
        cols1: usize,
        /// Rows in second matrix.
        rows2: usize,
        /// Columns in second matrix.
        cols2: usize,
    },

    /// Interpolation point is outside the valid range.
    #[error("Extrapolation not allowed: {x} is outside [{min}, {max}]")]
    ExtrapolationNotAllowed {
        /// The query point.
        x: f64,
        /// Minimum valid value.
        min: f64,
        /// Maximum valid value.
        max: f64,
    },

    /// Insufficient data points for operation.
    #[error("Insufficient data: need at least {required}, got {actual}")]
    InsufficientData {
        /// Minimum required points.
        required: usize,
        /// Actual number of points.
        actual: usize,
    },

    /// Invalid input parameter.
    #[error("Invalid input: {reason}")]
    InvalidInput {
        /// Description of the invalid input.
        reason: String,
    },

    /// Numerical overflow.
    #[error("Numerical overflow in {operation}")]
    Overflow {
        /// The operation that caused overflow.
        operation: String,
    },

    /// Numerical underflow.
    #[error("Numerical underflow in {operation}")]
    Underflow {
        /// The operation that caused underflow.
        operation: String,
    },
}

impl MathError {
    /// Creates a convergence failed error.
    #[must_use]
    pub fn convergence_failed(iterations: u32, residual: f64) -> Self {
        Self::ConvergenceFailed {
            iterations,
            residual,
        }
    }

    /// Creates an invalid input error.
    #[must_use]
    pub fn invalid_input(reason: impl Into<String>) -> Self {
        Self::InvalidInput {
            reason: reason.into(),
        }
    }

    /// Creates an insufficient data error.
    #[must_use]
    pub fn insufficient_data(required: usize, actual: usize) -> Self {
        Self::InsufficientData { required, actual }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = MathError::convergence_failed(100, 1e-6);
        assert!(err.to_string().contains("100 iterations"));
    }
}
