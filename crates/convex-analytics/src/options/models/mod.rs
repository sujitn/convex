//! Short rate models for interest rate tree construction.
//!
//! This module provides:
//!
//! - **Hull-White**: One-factor mean-reverting model (industry standard)
//! - **Black-Derman-Toy**: Log-normal short rate model (planned)
//! - **Black-Karasinski**: Log-normal mean-reverting model (planned)
//!
//! # Overview
//!
//! Short rate models describe the dynamics of the instantaneous interest rate
//! and are used to construct binomial trees for pricing callable/puttable bonds.
//!
//! # Hull-White Model
//!
//! The Hull-White model is defined by:
//!
//! ```text
//! dr = (θ(t) - a*r)dt + σ*dW
//! ```
//!
//! Where:
//! - `a` = mean reversion speed (typically 0.01 - 0.10)
//! - `σ` = volatility (typically 0.005 - 0.02)
//! - `θ(t)` = time-dependent drift calibrated to fit the initial yield curve

mod hull_white;

pub use hull_white::HullWhite;

use super::BinomialTree;
use crate::AnalyticsError;

/// Error type for model operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ModelError {
    /// Calibration failed.
    #[error("calibration failed: {reason}")]
    CalibrationFailed {
        /// Reason for failure.
        reason: String,
    },

    /// Invalid parameter.
    #[error("invalid parameter: {name} = {value}")]
    InvalidParameter {
        /// Parameter name.
        name: &'static str,
        /// Invalid value.
        value: f64,
    },

    /// Tree construction failed.
    #[error("tree construction failed: {reason}")]
    TreeConstructionFailed {
        /// Reason for failure.
        reason: String,
    },
}

impl ModelError {
    /// Creates a calibration failed error.
    pub fn calibration_failed(reason: impl Into<String>) -> Self {
        Self::CalibrationFailed {
            reason: reason.into(),
        }
    }

    /// Creates an invalid parameter error.
    #[must_use]
    pub fn invalid_parameter(name: &'static str, value: f64) -> Self {
        Self::InvalidParameter { name, value }
    }

    /// Creates a tree construction failed error.
    pub fn tree_construction_failed(reason: impl Into<String>) -> Self {
        Self::TreeConstructionFailed {
            reason: reason.into(),
        }
    }
}

impl From<ModelError> for AnalyticsError {
    fn from(err: ModelError) -> Self {
        AnalyticsError::CalculationFailed(err.to_string())
    }
}

/// A short rate model for building interest rate trees.
///
/// Short rate models describe the evolution of the instantaneous interest rate
/// and are used to construct binomial/trinomial trees for pricing bonds with
/// embedded options.
pub trait ShortRateModel: Send + Sync {
    /// Builds an interest rate tree.
    ///
    /// # Arguments
    ///
    /// * `zero_rates` - Zero rates as function of time f(t) -> rate
    /// * `maturity` - Tree maturity in years
    /// * `steps` - Number of time steps
    ///
    /// # Returns
    ///
    /// A binomial tree with short rates at each node.
    fn build_tree(
        &self,
        zero_rates: &dyn Fn(f64) -> f64,
        maturity: f64,
        steps: usize,
    ) -> BinomialTree;

    /// Returns the volatility at time t.
    fn volatility(&self, t: f64) -> f64;

    /// Returns the mean reversion speed.
    fn mean_reversion(&self) -> f64;

    /// Returns the model name.
    fn name(&self) -> &'static str;
}
