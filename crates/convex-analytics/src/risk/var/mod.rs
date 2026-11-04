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
    /// Signed loss quantile; negative means a gain at the supplied confidence.
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

/// Common scalar admission; a zero exposure never hides invalid model inputs.
fn admit(exposure: f64, confidence: f64, horizon: u32) -> crate::error::AnalyticsResult<()> {
    if !exposure.is_finite()
        || !confidence.is_finite()
        || confidence <= 0.0
        || confidence >= 1.0
        || horizon == 0
    {
        return Err(crate::error::AnalyticsError::InvalidInput(
            "finite exposure/confidence, confidence in (0,1), and positive horizon required".into(),
        ));
    }
    Ok(())
}

fn product(values: &[f64]) -> crate::error::AnalyticsResult<f64> {
    if values.iter().any(|v| !v.is_finite()) {
        return Err(crate::error::AnalyticsError::InvalidInput(
            "finite VaR factors required".into(),
        ));
    }
    if values.contains(&0.0) {
        return Ok(0.0);
    }
    values.iter().try_fold(1.0_f64, |amount, factor| {
        let next = amount * factor;
        if next.is_finite() && next != 0.0 {
            Ok(next)
        } else {
            Err(crate::error::AnalyticsError::CalculationFailed(
                "VaR arithmetic overflow or nonzero underflow".into(),
            ))
        }
    })
}
