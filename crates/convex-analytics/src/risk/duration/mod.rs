//! Duration calculations for fixed income instruments.
//!
//! Duration measures the sensitivity of a bond's price to changes in interest rates.
//! This module provides multiple duration measures:
//!
//! - **Macaulay Duration**: Weighted average time to receive cash flows
//! - **Modified Duration**: Price sensitivity measure (∂P/∂y × 1/P)
//! - **Effective Duration**: For bonds with embedded options
//! - **Key Rate Duration**: Sensitivity to specific points on the yield curve
//! - **Spread Duration**: Sensitivity to spread changes

mod effective;
mod key_rate;
mod macaulay;
mod modified;
mod spread_duration;

pub use effective::*;
pub use key_rate::*;
pub use macaulay::*;
pub use modified::*;
pub use spread_duration::*;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Duration value (in years)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Duration(#[serde(deserialize_with = "super::cash::deserialize_decimal")] Decimal);

impl Duration {
    /// Create a new Duration value
    pub fn new(years: Decimal) -> Self {
        Self(years)
    }

    /// Get the duration in years
    pub fn years(&self) -> Decimal {
        self.0
    }

    /// Get the duration as f64
    pub fn as_f64(&self) -> f64 {
        self.0.as_f64()
    }
}

impl std::fmt::Display for Duration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.4} years", self.0)
    }
}

impl From<Decimal> for Duration {
    fn from(d: Decimal) -> Self {
        Self(d)
    }
}

impl TryFrom<f64> for Duration {
    type Error = crate::error::AnalyticsError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        crate::risk::cash::decimal(value).map(Self)
    }
}
