//! Modified duration calculation.
//!
//! Modified duration measures the percentage price change per unit change in yield.
//! It's derived from Macaulay duration:
//!
//! ## Formula
//!
//! ```text
//! D_mod = D_mac / (1 + y/f)
//! ```
//!
//! where:
//! - D_mac = Macaulay duration
//! - y = yield to maturity
//! - f = compounding frequency

use super::{macaulay_duration, Duration};
use crate::error::AnalyticsResult;

/// Calculate modified duration from cash flows and yield.
///
/// # Arguments
///
/// * `times` - Time to each cash flow in years
/// * `cash_flows` - Amount of each cash flow
/// * `ytm` - Yield to maturity (as decimal, e.g., 0.05 for 5%)
/// * `frequency` - Compounding frequency per year
///
/// # Returns
///
/// Modified duration in years
pub fn modified_duration(
    times: &[f64],
    cash_flows: &[f64],
    ytm: f64,
    frequency: u32,
) -> AnalyticsResult<Duration> {
    let mac_dur = macaulay_duration(times, cash_flows, ytm, frequency)?;
    let mod_dur = mac_dur.as_f64() / (1.0 + ytm / frequency as f64);
    Ok(Duration::from(mod_dur))
}

/// Convert Macaulay duration to modified duration.
///
/// # Arguments
///
/// * `macaulay` - Macaulay duration
/// * `ytm` - Yield to maturity (as decimal)
/// * `frequency` - Compounding frequency per year
pub fn modified_from_macaulay(macaulay: Duration, ytm: f64, frequency: u32) -> Duration {
    let mod_dur = macaulay.as_f64() / (1.0 + ytm / frequency as f64);
    Duration::from(mod_dur)
}

/// Calculate approximate price change using modified duration.
///
/// # Arguments
///
/// * `mod_duration` - Modified duration
/// * `price` - Current price
/// * `yield_change` - Change in yield (as decimal, e.g., 0.01 for 100bps)
///
/// # Returns
///
/// Approximate price change
pub fn price_change_from_duration(mod_duration: Duration, price: f64, yield_change: f64) -> f64 {
    -mod_duration.as_f64() * price * yield_change
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_modified_duration_par_bond() {
        // 2-year bond, 5% coupon, semi-annual
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];

        let dur = modified_duration(&times, &cash_flows, 0.05, 2).unwrap();

        // Modified duration should be slightly less than Macaulay
        assert_relative_eq!(dur.as_f64(), 1.88, epsilon = 0.01);
    }

    #[test]
    fn test_modified_from_macaulay() {
        let mac_dur = Duration::from(5.0);
        let mod_dur = modified_from_macaulay(mac_dur, 0.06, 2);

        // D_mod = 5.0 / (1 + 0.06/2) = 5.0 / 1.03 ≈ 4.854
        assert_relative_eq!(mod_dur.as_f64(), 4.854, epsilon = 0.001);
    }

    #[test]
    fn test_price_change_approximation() {
        let mod_dur = Duration::from(5.0);
        let price = 100.0;
        let yield_change = 0.01; // 100 bps

        let change = price_change_from_duration(mod_dur, price, yield_change);

        // Price should drop by approximately 5% for 100bp yield increase
        assert_relative_eq!(change, -5.0, epsilon = 0.01);
    }
}
