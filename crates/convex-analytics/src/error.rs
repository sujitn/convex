//! Analytics error types.
//!
//! Every variant listed here is constructed somewhere in the crate. Historical
//! per-calculation variants (ZSpreadFailed, DurationFailed, etc.) were pure
//! scaffolding — no caller ever constructed them and no consumer ever matched
//! on them. They have been folded into [`AnalyticsError::CalculationFailed`]
//! for generic failures or the more structured variants below.

use thiserror::Error;

/// Error type for convex-analytics calculations.
#[derive(Debug, Error)]
pub enum AnalyticsError {
    /// A yield solver ran out of iterations.
    #[error("yield solver failed to converge after {iterations} iterations: {reason}")]
    YieldSolverFailed {
        /// Number of iterations attempted before giving up.
        iterations: u32,
        /// Reason for failure (e.g. divergent derivative).
        reason: String,
    },

    /// A generic numerical solver failed to converge.
    #[error("{solver} failed to converge after {iterations} iterations (residual: {residual})")]
    SolverConvergenceFailed {
        /// Name of the solver that stopped short.
        solver: String,
        /// Number of iterations before stopping.
        iterations: u32,
        /// Final residual value.
        residual: f64,
    },

    /// Settlement date is on or after maturity.
    #[error("invalid settlement date: settlement {settlement} must be before maturity {maturity}")]
    InvalidSettlement {
        /// The settlement date that was provided.
        settlement: String,
        /// The maturity date of the instrument.
        maturity: String,
    },

    /// A lookup by benchmark name did not resolve.
    #[error("benchmark not found: {0}")]
    BenchmarkNotFound(String),

    /// Cash flow generation failed.
    #[error("cash flow generation failed: {0}")]
    CashFlowGenerationFailed(String),

    /// Accrued interest calculation failed.
    #[error("accrued interest calculation failed: {0}")]
    AccruedInterestFailed(String),

    /// Schedule generation failed.
    #[error("schedule generation failed: {0}")]
    ScheduleGenerationFailed(String),

    /// Caller supplied an invalid input parameter.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Day count convention parse or computation failure.
    #[error("day count error: {0}")]
    DayCountError(String),

    /// Curve-backed computation failed (wrapped from convex-curves).
    #[error("curve error: {0}")]
    CurveError(String),

    /// Math primitive failed (wrapped from convex-math or convex-core).
    #[error("math error: {0}")]
    MathError(String),

    /// Bond-side computation failed (wrapped from convex-bonds).
    #[error("bond error: {0}")]
    BondError(String),

    /// Generic calculation failure; prefer one of the structured variants above.
    #[error("calculation failed: {0}")]
    CalculationFailed(String),
}

/// Result type alias for analytics operations.
pub type AnalyticsResult<T> = Result<T, AnalyticsError>;

impl From<convex_core::error::ConvexError> for AnalyticsError {
    fn from(err: convex_core::error::ConvexError) -> Self {
        AnalyticsError::MathError(err.to_string())
    }
}

impl From<convex_math::error::MathError> for AnalyticsError {
    fn from(err: convex_math::error::MathError) -> Self {
        AnalyticsError::MathError(err.to_string())
    }
}

impl From<convex_curves::CurveError> for AnalyticsError {
    fn from(err: convex_curves::CurveError) -> Self {
        AnalyticsError::CurveError(err.to_string())
    }
}

impl From<convex_bonds::BondError> for AnalyticsError {
    fn from(err: convex_bonds::BondError) -> Self {
        AnalyticsError::BondError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = AnalyticsError::YieldSolverFailed {
            iterations: 100,
            reason: "did not converge".to_string(),
        };
        assert!(err.to_string().contains("100 iterations"));

        let err = AnalyticsError::InvalidSettlement {
            settlement: "2025-01-01".to_string(),
            maturity: "2024-01-01".to_string(),
        };
        assert!(err.to_string().contains("settlement"));
    }
}
