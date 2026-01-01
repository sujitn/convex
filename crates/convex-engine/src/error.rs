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

    /// Reference data error
    #[error("reference data error: {0}")]
    ReferenceDataError(String),

    /// Storage error
    #[error("storage error: {0}")]
    StorageError(String),

    /// Calculation error
    #[error("calculation error: {0}")]
    CalculationError(String),

    /// Node not found
    #[error("node not found: {0}")]
    NodeNotFound(String),

    /// Curve build error
    #[error("curve build error: {0}")]
    CurveBuildError(String),

    /// Pricing error
    #[error("pricing error: {0}")]
    PricingError(String),

    /// Missing dependency
    #[error("missing dependency: {0}")]
    MissingDependency(String),

    /// Circular dependency
    #[error("circular dependency detected")]
    CircularDependency,

    /// Timeout
    #[error("timeout")]
    Timeout,

    /// Shutdown
    #[error("engine is shutting down")]
    Shutdown,

    /// Internal error
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<convex_traits::TraitError> for EngineError {
    fn from(e: convex_traits::TraitError) -> Self {
        EngineError::Internal(e.to_string())
    }
}
