//! Error types for the pricing framework.

use thiserror::Error;

/// Result type for pricing operations.
pub type PricingResult<T> = Result<T, PricingError>;

/// Errors that can occur during pricing calculations.
#[derive(Debug, Error)]
pub enum PricingError {
    /// No cash flows provided for pricing.
    #[error("no cash flows provided for pricing")]
    NoCashFlows,

    /// No future cash flows after settlement date.
    #[error("no future cash flows after settlement date {settlement}")]
    NoFutureCashFlows { settlement: String },

    /// Settlement date is after or at maturity.
    #[error("settlement {settlement} is at or after maturity {maturity}")]
    SettlementAfterMaturity {
        settlement: String,
        maturity: String,
    },

    /// Spread solver did not converge.
    #[error("spread solver did not converge after {iterations} iterations")]
    SpreadNotConverged { iterations: u32 },

    /// Yield solver did not converge.
    #[error("yield solver did not converge after {iterations} iterations")]
    YieldNotConverged { iterations: u32 },

    /// Invalid input parameter.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Curve error during discount factor calculation.
    #[error("curve error: {0}")]
    CurveError(String),

    /// Mathematical error during calculation.
    #[error("math error: {0}")]
    MathError(String),
}

impl PricingError {
    /// Creates a new invalid input error.
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::InvalidInput(msg.into())
    }

    /// Creates a new curve error.
    pub fn curve_error(msg: impl Into<String>) -> Self {
        Self::CurveError(msg.into())
    }

    /// Creates a new math error.
    pub fn math_error(msg: impl Into<String>) -> Self {
        Self::MathError(msg.into())
    }

    /// Creates a no future cash flows error.
    pub fn no_future_cash_flows(settlement: impl ToString) -> Self {
        Self::NoFutureCashFlows {
            settlement: settlement.to_string(),
        }
    }

    /// Creates a settlement after maturity error.
    pub fn settlement_after_maturity(settlement: impl ToString, maturity: impl ToString) -> Self {
        Self::SettlementAfterMaturity {
            settlement: settlement.to_string(),
            maturity: maturity.to_string(),
        }
    }
}

// Convert from convex_core error
impl From<convex_core::error::ConvexError> for PricingError {
    fn from(err: convex_core::error::ConvexError) -> Self {
        PricingError::CurveError(err.to_string())
    }
}

// Convert from convex_curves error
impl From<convex_curves::error::CurveError> for PricingError {
    fn from(err: convex_curves::error::CurveError) -> Self {
        PricingError::CurveError(err.to_string())
    }
}

// Convert from convex_math error
impl From<convex_math::error::MathError> for PricingError {
    fn from(err: convex_math::error::MathError) -> Self {
        PricingError::MathError(err.to_string())
    }
}
