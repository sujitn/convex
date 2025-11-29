//! Error types for risk calculations.

use thiserror::Error;

/// Errors that can occur during risk calculations.
#[derive(Debug, Error)]
pub enum RiskError {
    /// Invalid input parameters
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Calculation failed
    #[error("calculation failed: {0}")]
    CalculationFailed(String),

    /// Pricing error from bonds crate
    #[error("pricing error: {0}")]
    PricingError(String),

    /// Curve error from curves crate
    #[error("curve error: {0}")]
    CurveError(String),

    /// Division by zero
    #[error("division by zero in {context}")]
    DivisionByZero { context: String },

    /// Insufficient data for calculation
    #[error("insufficient data: {0}")]
    InsufficientData(String),
}
