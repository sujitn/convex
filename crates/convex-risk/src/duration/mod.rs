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

mod macaulay;
mod modified;
mod effective;
mod key_rate;
mod spread_duration;

pub use macaulay::*;
pub use modified::*;
pub use effective::*;
pub use key_rate::*;
pub use spread_duration::*;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Duration value (in years)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Duration(Decimal);

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
        use rust_decimal::prelude::ToPrimitive;
        self.0.to_f64().unwrap_or(0.0)
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

impl From<f64> for Duration {
    fn from(f: f64) -> Self {
        Self(Decimal::from_f64_retain(f).unwrap_or(Decimal::ZERO))
    }
}
