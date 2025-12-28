//! Error types for the Convex engine.

use thiserror::Error;

/// Result type for engine operations.
pub type EngineResult<T> = Result<T, EngineError>;

/// Engine error type.
#[derive(Debug, Error)]
pub enum EngineError {
    /// Node not found in the calculation graph.
    #[error("Node not found: {0}")]
    NodeNotFound(String),

    /// Circular dependency detected in the graph.
    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    /// Calculation failed for a node.
    #[error("Calculation failed for node {node}: {reason}")]
    CalculationFailed {
        /// Node identifier.
        node: String,
        /// Failure reason.
        reason: String,
    },

    /// Curve not found in cache.
    #[error("Curve not found: {0}")]
    CurveNotFound(String),

    /// Bond not found.
    #[error("Bond not found: {0}")]
    BondNotFound(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Service unavailable (circuit breaker open).
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    /// Operation timed out.
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Rate limit exceeded.
    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    /// Storage error.
    #[error("Storage error: {0}")]
    Storage(#[from] convex_storage::StorageError),

    /// Analytics error.
    #[error("Analytics error: {0}")]
    Analytics(#[from] convex_analytics::AnalyticsError),

    /// Core error.
    #[error("Core error: {0}")]
    Core(#[from] convex_core::ConvexError),

    /// Configuration error.
    #[error("Config error: {0}")]
    Config(#[from] convex_config::ConfigError),

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),

    /// Shutdown in progress.
    #[error("Service is shutting down")]
    ShuttingDown,
}

impl EngineError {
    /// Creates a calculation failed error.
    pub fn calculation_failed(node: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::CalculationFailed {
            node: node.into(),
            reason: reason.into(),
        }
    }

    /// Creates an internal error.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// Returns true if this error is retriable.
    pub fn is_retriable(&self) -> bool {
        matches!(
            self,
            Self::Timeout(_) | Self::ServiceUnavailable(_) | Self::RateLimitExceeded
        )
    }

    /// Returns true if this error indicates the service should stop.
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::ShuttingDown | Self::CircularDependency(_))
    }
}
