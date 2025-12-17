//! Unified error types for the analytics engine.
//!
//! This module consolidates all error types from yield calculations, spreads,
//! risk metrics, and YAS functionality.

use rust_decimal::Decimal;
use thiserror::Error;

/// Unified error type for all analytics operations.
#[derive(Debug, Error)]
pub enum AnalyticsError {
    // ========== Yield/Pricing Errors ==========
    /// Yield solver failed to converge
    #[error("yield solver failed to converge after {iterations} iterations: {reason}")]
    YieldSolverFailed {
        /// Number of iterations before failure.
        iterations: u32,
        /// Reason for failure.
        reason: String,
    },

    /// Invalid yield value
    #[error("invalid yield value: {0}")]
    InvalidYield(String),

    /// Price calculation failed
    #[error("price calculation failed: {0}")]
    PriceCalculationFailed(String),

    // ========== Spread Errors ==========
    /// Z-spread calculation failed
    #[error("Z-spread calculation failed: {0}")]
    ZSpreadFailed(String),

    /// G-spread calculation failed
    #[error("G-spread calculation failed: {0}")]
    GSpreadFailed(String),

    /// I-spread calculation failed
    #[error("I-spread calculation failed: {0}")]
    ISpreadFailed(String),

    /// OAS calculation failed
    #[error("OAS calculation failed: {0}")]
    OASFailed(String),

    /// Asset swap spread calculation failed
    #[error("asset swap spread calculation failed: {0}")]
    ASWFailed(String),

    /// Discount margin calculation failed
    #[error("discount margin calculation failed: {0}")]
    DiscountMarginFailed(String),

    /// Benchmark not found
    #[error("benchmark not found: {0}")]
    BenchmarkNotFound(String),

    // ========== Risk Errors ==========
    /// Duration calculation failed
    #[error("duration calculation failed: {0}")]
    DurationFailed(String),

    /// Convexity calculation failed
    #[error("convexity calculation failed: {0}")]
    ConvexityFailed(String),

    /// DV01 calculation failed
    #[error("DV01 calculation failed: {0}")]
    DV01Failed(String),

    /// Key rate duration calculation failed
    #[error("key rate duration calculation failed: {0}")]
    KeyRateDurationFailed(String),

    /// VaR calculation failed
    #[error("VaR calculation failed: {0}")]
    VaRFailed(String),

    // ========== Cash Flow Errors ==========
    /// Cash flow generation failed
    #[error("cash flow generation failed: {0}")]
    CashFlowGenerationFailed(String),

    /// Accrued interest calculation failed
    #[error("accrued interest calculation failed: {0}")]
    AccruedInterestFailed(String),

    /// Schedule generation failed
    #[error("schedule generation failed: {0}")]
    ScheduleGenerationFailed(String),

    // ========== Options Errors ==========
    /// Binomial tree construction failed
    #[error("binomial tree construction failed: {0}")]
    BinomialTreeFailed(String),

    /// Model calibration failed
    #[error("model calibration failed: {0}")]
    ModelCalibrationFailed(String),

    // ========== YAS Errors ==========
    /// YAS calculation failed
    #[error("YAS calculation failed: {0}")]
    YASFailed(String),

    /// Settlement invoice calculation failed
    #[error("settlement invoice calculation failed: {0}")]
    SettlementInvoiceFailed(String),

    // ========== General Errors ==========
    /// Invalid input parameter
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Invalid date
    #[error("invalid date: {0}")]
    InvalidDate(String),

    /// Invalid settlement date
    #[error("invalid settlement date: settlement {settlement} must be before maturity {maturity}")]
    InvalidSettlement {
        /// The settlement date that was provided.
        settlement: String,
        /// The maturity date of the instrument.
        maturity: String,
    },

    /// Curve error
    #[error("curve error: {0}")]
    CurveError(String),

    /// Math/solver error
    #[error("math error: {0}")]
    MathError(String),

    /// Day count error
    #[error("day count error: {0}")]
    DayCountError(String),

    /// Bond error (from convex-bonds)
    #[error("bond error: {0}")]
    BondError(String),

    /// Value out of bounds
    #[error("{name} value {value} is out of bounds [{min}, {max}]")]
    OutOfBounds {
        /// Name of the parameter that is out of bounds.
        name: String,
        /// The value that was provided.
        value: Decimal,
        /// Minimum allowed value.
        min: Decimal,
        /// Maximum allowed value.
        max: Decimal,
    },

    /// Not implemented
    #[error("not implemented: {0}")]
    NotImplemented(String),

    /// General calculation failure
    #[error("calculation failed: {0}")]
    CalculationFailed(String),

    /// Solver convergence failed
    #[error("{solver} failed to converge after {iterations} iterations (residual: {residual})")]
    SolverConvergenceFailed {
        /// Name of the solver that failed.
        solver: String,
        /// Number of iterations before failure.
        iterations: u32,
        /// Final residual value when the solver stopped.
        residual: f64,
    },
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
