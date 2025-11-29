//! Error types for YAS calculations.

use thiserror::Error;

/// Errors that can occur during YAS calculations.
#[derive(Debug, Error)]
pub enum YasError {
    /// Invalid input parameters
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Pricing error
    #[error("pricing error: {0}")]
    PricingError(String),

    /// Curve error
    #[error("curve error: {0}")]
    CurveError(String),

    /// Solver did not converge
    #[error("solver did not converge: {context}, iterations: {iterations}")]
    SolverNoConvergence { context: String, iterations: u32 },

    /// Missing required data
    #[error("missing data: {0}")]
    MissingData(String),

    /// Calculation failed
    #[error("calculation failed: {0}")]
    CalculationFailed(String),

    /// Settlement date error
    #[error("settlement error: {0}")]
    SettlementError(String),
}
