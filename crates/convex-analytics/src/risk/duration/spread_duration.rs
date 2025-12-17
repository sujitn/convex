//! Spread duration calculation.
//!
//! Spread duration measures the sensitivity of a bond's price to changes
//! in its credit spread, rather than the underlying risk-free rate.

use super::Duration;
use crate::error::{AnalyticsError, AnalyticsResult};

/// Calculate spread duration using finite differences.
///
/// Spread duration is typically similar to modified duration for
/// fixed-rate bonds, but can differ for floating rate instruments.
///
/// # Arguments
///
/// * `price_spread_up` - Price when spread increases by bump size
/// * `price_spread_down` - Price when spread decreases by bump size
/// * `price_base` - Current/base price
/// * `spread_bump` - Spread bump size (as decimal, e.g., 0.0001 for 1bp)
pub fn spread_duration(
    price_spread_up: f64,
    price_spread_down: f64,
    price_base: f64,
    spread_bump: f64,
) -> AnalyticsResult<Duration> {
    if price_base.abs() < 1e-10 {
        return Err(AnalyticsError::CalculationFailed(
            "base price is zero".to_string(),
        ));
    }

    if spread_bump.abs() < 1e-12 {
        return Err(AnalyticsError::InvalidInput(
            "spread bump size too small".to_string(),
        ));
    }

    let sd = (price_spread_down - price_spread_up) / (2.0 * price_base * spread_bump);
    Ok(Duration::from(sd))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_spread_duration() {
        // For a fixed rate bond, spread duration ≈ modified duration
        let price_base = 100.0;
        let bump = 0.0001; // 1 bp

        let price_up = 99.95; // spread +1bp
        let price_down = 100.05; // spread -1bp

        let dur = spread_duration(price_up, price_down, price_base, bump).unwrap();

        assert_relative_eq!(dur.as_f64(), 5.0, epsilon = 0.1);
    }
}
