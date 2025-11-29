//! Macaulay duration calculation.
//!
//! Macaulay duration is the weighted average time to receive cash flows,
//! where the weights are the present values of the cash flows.
//!
//! ## Formula
//!
//! ```text
//! D_mac = Σ(t_i × PV(CF_i)) / P
//! ```
//!
//! where:
//! - t_i = time to cash flow i (in years)
//! - PV(CF_i) = present value of cash flow i
//! - P = bond price (sum of all PVs)

use super::Duration;
use crate::RiskError;

/// Calculate Macaulay duration from cash flows and yield.
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
/// Macaulay duration in years
///
/// # Example
///
/// ```ignore
/// let times = vec![0.5, 1.0, 1.5, 2.0];
/// let cash_flows = vec![25.0, 25.0, 25.0, 1025.0];
/// let duration = macaulay_duration(&times, &cash_flows, 0.05, 2)?;
/// ```
pub fn macaulay_duration(
    times: &[f64],
    cash_flows: &[f64],
    ytm: f64,
    frequency: u32,
) -> Result<Duration, RiskError> {
    if times.len() != cash_flows.len() {
        return Err(RiskError::InvalidInput(
            "times and cash_flows must have same length".to_string(),
        ));
    }

    if times.is_empty() {
        return Err(RiskError::InsufficientData(
            "no cash flows provided".to_string(),
        ));
    }

    let freq = frequency as f64;
    let periodic_rate = ytm / freq;

    let mut weighted_sum = 0.0;
    let mut price = 0.0;

    for (t, cf) in times.iter().zip(cash_flows.iter()) {
        let periods = t * freq;
        let df = (1.0 + periodic_rate).powf(-periods);
        let pv = cf * df;

        weighted_sum += t * pv;
        price += pv;
    }

    if price.abs() < 1e-10 {
        return Err(RiskError::DivisionByZero {
            context: "price is zero in macaulay duration".to_string(),
        });
    }

    let mac_dur = weighted_sum / price;
    Ok(Duration::from(mac_dur))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_macaulay_duration_par_bond() {
        // 2-year bond, 5% coupon, semi-annual, priced at par
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5]; // per $100 face

        let dur = macaulay_duration(&times, &cash_flows, 0.05, 2).unwrap();

        // For a 2-year 5% bond at par, Macaulay duration should be ~1.93 years
        assert_relative_eq!(dur.as_f64(), 1.93, epsilon = 0.01);
    }

    #[test]
    fn test_macaulay_duration_zero_coupon() {
        // Zero coupon bond maturing in 5 years
        let times = vec![5.0];
        let cash_flows = vec![100.0];

        let dur = macaulay_duration(&times, &cash_flows, 0.05, 1).unwrap();

        // Zero coupon bond duration equals maturity
        assert_relative_eq!(dur.as_f64(), 5.0, epsilon = 0.0001);
    }

    #[test]
    fn test_macaulay_duration_mismatched_lengths() {
        let times = vec![0.5, 1.0];
        let cash_flows = vec![2.5];

        let result = macaulay_duration(&times, &cash_flows, 0.05, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_macaulay_duration_empty() {
        let times: Vec<f64> = vec![];
        let cash_flows: Vec<f64> = vec![];

        let result = macaulay_duration(&times, &cash_flows, 0.05, 2);
        assert!(result.is_err());
    }
}
