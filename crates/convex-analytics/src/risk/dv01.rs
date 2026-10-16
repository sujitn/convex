//! DV01 (Dollar Value of 01) calculations.
//!
//! DV01, also known as PV01 or PVBP (Price Value of a Basis Point),
//! measures the absolute price change for a 1 basis point change in yield.
//!
//! ## Formula
//!
//! ```text
//! DV01 = Modified Duration × Dirty Price × Face Value × 0.0001
//! ```

use crate::error::AnalyticsResult;
use crate::risk::duration::Duration;
use convex_bonds::pricing::risk_math;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// DV01 value (dollar change per basis point)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(transparent)]
pub struct DV01(Decimal);

impl DV01 {
    /// Create a new DV01 value
    pub fn new(value: Decimal) -> Self {
        Self(value)
    }

    /// Get the DV01 value
    pub fn value(&self) -> Decimal {
        self.0
    }

    /// Get the DV01 as f64
    pub fn as_f64(&self) -> f64 {
        self.0.as_f64()
    }
}

impl std::fmt::Display for DV01 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "${:.4}", self.0)
    }
}

impl From<Decimal> for DV01 {
    fn from(d: Decimal) -> Self {
        Self(d)
    }
}

impl TryFrom<f64> for DV01 {
    type Error = crate::error::AnalyticsError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        crate::risk::cash::decimal(value).map(Self)
    }
}

/// Calculate DV01 from modified duration.
///
/// # Arguments
///
/// * `modified_duration` - Modified duration
/// * `dirty_price` - Dirty price as percentage of par (e.g., 105.5 for 105.5%)
/// * `face_value` - Face value of the bond
///
/// # Returns
///
/// DV01 in absolute dollar terms
pub fn dv01_from_duration(
    modified_duration: Duration,
    dirty_price: f64,
    face_value: f64,
) -> AnalyticsResult<DV01> {
    let value = risk_math::dv01(modified_duration.as_f64(), dirty_price, face_value)?;
    Ok(DV01::new(crate::risk::cash::decimal(value)?))
}

/// Calculate DV01 directly from price bumps.
///
/// # Arguments
///
/// * `price_up` - Price when yield increases by 1bp
/// * `price_down` - Price when yield decreases by 1bp
///
/// # Returns
///
/// DV01 as the average of up and down price changes
pub fn dv01_from_prices(price_up: f64, price_down: f64) -> AnalyticsResult<DV01> {
    let value = risk_math::dv01_from_prices(price_up, price_down)?;
    Ok(DV01::new(crate::risk::cash::decimal(value)?))
}

/// Calculate DV01 per $100 face value.
///
/// This is a common convention for quoting DV01.
///
/// # Arguments
///
/// * `modified_duration` - Modified duration
/// * `dirty_price` - Dirty price as percentage of par
pub fn dv01_per_100_face(modified_duration: Duration, dirty_price: f64) -> AnalyticsResult<DV01> {
    dv01_from_duration(modified_duration, dirty_price, 100.0)
}

/// Calculate notional equivalent from DV01.
///
/// Given a target DV01, calculate the face value needed to achieve it.
///
/// # Arguments
///
/// * `target_dv01` - Target DV01 value
/// * `modified_duration` - Modified duration of the instrument
/// * `dirty_price` - Dirty price as percentage of par
pub fn notional_from_dv01(
    target_dv01: DV01,
    modified_duration: Duration,
    dirty_price: f64,
) -> AnalyticsResult<f64> {
    risk_math::notional_from_dv01(
        target_dv01.as_f64(),
        modified_duration.as_f64(),
        dirty_price,
    )
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_dv01_from_duration() {
        let mod_dur = Duration::try_from(5.0).unwrap();
        let dirty_price = 105.0; // 105% of par
        let face_value = 1_000_000.0;

        let dv01 = dv01_from_duration(mod_dur, dirty_price, face_value).unwrap();

        // DV01 = 5 × 1.05 × 1,000,000 × 0.0001 = 525
        assert_relative_eq!(dv01.as_f64(), 525.0, epsilon = 0.01);
    }

    #[test]
    fn test_dv01_per_100_face() {
        let mod_dur = Duration::try_from(5.0).unwrap();
        let dirty_price = 100.0;

        let dv01 = dv01_per_100_face(mod_dur, dirty_price).unwrap();

        // DV01 = 5 × 1.0 × 100 × 0.0001 = 0.05
        assert_relative_eq!(dv01.as_f64(), 0.05, epsilon = 0.001);
    }

    #[test]
    fn test_dv01_from_prices() {
        let price_up = 99.95; // -0.05 from base
        let price_down = 100.05; // +0.05 from base

        let dv01 = dv01_from_prices(price_up, price_down).unwrap();

        // (100.05 - 99.95) / 2 = 0.05
        assert_relative_eq!(dv01.as_f64(), 0.05, epsilon = 0.001);
    }

    #[test]
    fn test_notional_from_dv01() {
        let target = DV01::try_from(1000.0).unwrap();
        let mod_dur = Duration::try_from(5.0).unwrap();
        let dirty_price = 100.0;

        let notional = notional_from_dv01(target, mod_dur, dirty_price).unwrap();

        // 1000 / (5 × 1.0 × 0.0001) = 2,000,000
        assert_relative_eq!(notional, 2_000_000.0, epsilon = 1.0);
    }
}
