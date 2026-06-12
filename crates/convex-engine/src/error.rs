//! Engine error types.

use thiserror::Error;

/// Engine error type.
#[derive(Debug, Error)]
pub enum EngineError {
    /// Configuration error
    #[error("configuration error: {0}")]
    ConfigError(String),

    /// Market data error
    #[error("market data error: {0}")]
    MarketDataError(String),

    /// Curve build error
    #[error("curve build error: {0}")]
    CurveBuildError(String),

    /// Pricing error
    #[error("pricing error: {0}")]
    PricingError(String),

    /// Internal error
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<crate::ports::error::TraitError> for EngineError {
    fn from(e: crate::ports::error::TraitError) -> Self {
        EngineError::Internal(e.to_string())
    }
}
