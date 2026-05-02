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

use convex_core::types::Compounding;

use super::Duration;
use crate::error::{AnalyticsError, AnalyticsResult};

/// Discount factor for time `t` at yield `ytm` under `compounding`.
fn discount_factor(ytm: f64, compounding: Compounding, t: f64) -> f64 {
    match compounding {
        Compounding::Continuous => (-ytm * t).exp(),
        Compounding::Simple => 1.0 / (1.0 + ytm * t),
        _ => {
            let f = compounding.periods_per_year() as f64;
            (1.0 + ytm / f).powf(-t * f)
        }
    }
}

/// Macaulay duration: PV-weighted average time to cash flows.
pub fn macaulay_duration(
    times: &[f64],
    cash_flows: &[f64],
    ytm: f64,
    compounding: Compounding,
) -> AnalyticsResult<Duration> {
    if times.len() != cash_flows.len() {
        return Err(AnalyticsError::InvalidInput(
            "times and cash_flows must have same length".to_string(),
        ));
    }
    if times.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "no cash flows provided".to_string(),
        ));
    }

    let mut weighted_sum = 0.0;
    let mut price = 0.0;
    for (&t, &cf) in times.iter().zip(cash_flows.iter()) {
        let pv = cf * discount_factor(ytm, compounding, t);
        weighted_sum += t * pv;
        price += pv;
    }
    if price.abs() < 1e-10 {
        return Err(AnalyticsError::CalculationFailed(
            "price is zero in macaulay duration".to_string(),
        ));
    }
    Ok(Duration::from(weighted_sum / price))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_macaulay_duration_par_bond() {
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];
        let dur = macaulay_duration(&times, &cash_flows, 0.05, Compounding::SemiAnnual).unwrap();
        assert_relative_eq!(dur.as_f64(), 1.93, epsilon = 0.01);
    }

    #[test]
    fn test_macaulay_duration_zero_coupon() {
        let dur = macaulay_duration(&[5.0], &[100.0], 0.05, Compounding::Annual).unwrap();
        assert_relative_eq!(dur.as_f64(), 5.0, epsilon = 0.0001);
    }

    #[test]
    fn test_macaulay_duration_mismatched_lengths() {
        assert!(macaulay_duration(&[0.5, 1.0], &[2.5], 0.05, Compounding::SemiAnnual).is_err());
    }

    #[test]
    fn test_macaulay_duration_empty() {
        assert!(macaulay_duration(&[], &[], 0.05, Compounding::SemiAnnual).is_err());
    }
}
