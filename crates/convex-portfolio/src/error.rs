//! Error types for portfolio analytics.

use thiserror::Error;

/// Result type for portfolio operations.
pub type PortfolioResult<T> = Result<T, PortfolioError>;

/// Errors that can occur during portfolio operations.
#[derive(Error, Debug, Clone)]
#[allow(missing_docs)]
pub enum PortfolioError {
    /// Invalid portfolio configuration.
    #[error("Invalid portfolio: {reason}")]
    InvalidPortfolio {
        /// The reason the portfolio is invalid.
        reason: String,
    },

    /// Missing required field during construction.
    #[error("Missing required field: {field}")]
    MissingField {
        /// The name of the missing field.
        field: String,
    },

    /// Invalid holding data.
    #[error("Invalid holding '{id}': {reason}")]
    InvalidHolding {
        /// The holding ID.
        id: String,
        /// The reason the holding is invalid.
        reason: String,
    },

    /// Empty portfolio.
    #[error("Portfolio has no holdings")]
    EmptyPortfolio,

    /// Invalid FX rate.
    #[error("Invalid FX rate for {currency}: {rate}")]
    InvalidFxRate {
        /// The currency code.
        currency: String,
        /// The invalid rate value.
        rate: String,
    },
}

impl PortfolioError {
    /// Create an invalid portfolio error.
    #[must_use]
    pub fn invalid_portfolio(reason: impl Into<String>) -> Self {
        Self::InvalidPortfolio {
            reason: reason.into(),
        }
    }

    /// Create a missing field error.
    #[must_use]
    pub fn missing_field(field: impl Into<String>) -> Self {
        Self::MissingField {
            field: field.into(),
        }
    }

    /// Create an invalid holding error.
    #[must_use]
    pub fn invalid_holding(id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidHolding {
            id: id.into(),
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = PortfolioError::invalid_portfolio("test reason");
        assert!(err.to_string().contains("test reason"));

        let err = PortfolioError::missing_field("name");
        assert!(err.to_string().contains("name"));

        let err = PortfolioError::invalid_holding("BOND1", "negative par");
        assert!(err.to_string().contains("BOND1"));
        assert!(err.to_string().contains("negative par"));
    }

    #[test]
    fn test_error_clone() {
        let err = PortfolioError::EmptyPortfolio;
        let cloned = err.clone();
        assert_eq!(err.to_string(), cloned.to_string());
    }
}
