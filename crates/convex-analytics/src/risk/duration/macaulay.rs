//! Cash-PV-weighted time from the native checked derivative owner.
use super::Duration;
use crate::error::AnalyticsResult;
use convex_core::types::Compounding;

/// Complete supplied cash, explicit positive times and annual compounding basis.
pub fn macaulay_duration(
    times: &[f64],
    cash_flows: &[f64],
    ytm: f64,
    compounding: Compounding,
) -> AnalyticsResult<Duration> {
    let risk = crate::risk::cash::evaluate(times, cash_flows, ytm, compounding)?;
    Ok(Duration::new(crate::risk::cash::decimal(
        risk.macaulay_duration,
    )?))
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
