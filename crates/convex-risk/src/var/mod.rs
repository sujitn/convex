//! Value at Risk (VaR) calculations.
//!
//! VaR estimates the potential loss over a specified time horizon
//! at a given confidence level.

mod historical;
mod parametric;

pub use historical::*;
pub use parametric::*;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Value at Risk result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaRResult {
    /// The VaR value (absolute loss)
    pub var: Decimal,
    /// Confidence level (e.g., 0.95 for 95%)
    pub confidence_level: f64,
    /// Time horizon in days
    pub horizon_days: u32,
    /// Method used for calculation
    pub method: VaRMethod,
}

/// VaR calculation method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VaRMethod {
    /// Historical simulation
    Historical,
    /// Parametric (variance-covariance)
    Parametric,
    /// Monte Carlo simulation
    MonteCarlo,
}

impl std::fmt::Display for VaRResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "VaR({:.0}%, {}d): ${:.2}",
            self.confidence_level * 100.0,
            self.horizon_days,
            self.var
        )
    }
}
