//! Analytical convexity calculation.

use super::Convexity;
use crate::error::{AnalyticsError, AnalyticsResult};

/// Calculate analytical convexity from cash flows.
///
/// # Arguments
///
/// * `times` - Time to each cash flow in years
/// * `cash_flows` - Amount of each cash flow
/// * `ytm` - Yield to maturity (as decimal)
/// * `frequency` - Compounding frequency per year
///
/// # Returns
///
/// Convexity value
pub fn analytical_convexity(
    times: &[f64],
    cash_flows: &[f64],
    ytm: f64,
    frequency: u32,
) -> AnalyticsResult<Convexity> {
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

    let freq = frequency as f64;
    let periodic_rate = ytm / freq;
    let discount_factor_base = 1.0 + periodic_rate;

    let mut weighted_sum = 0.0;
    let mut price = 0.0;

    for (t, cf) in times.iter().zip(cash_flows.iter()) {
        let periods = t * freq;
        let df = discount_factor_base.powf(-periods);
        let pv = cf * df;

        // Convexity weight: t × (t + 1/f)
        let convexity_weight = t * (t + 1.0 / freq);
        weighted_sum += convexity_weight * pv;
        price += pv;
    }

    if price.abs() < 1e-10 {
        return Err(AnalyticsError::CalculationFailed(
            "price is zero in convexity calculation".to_string(),
        ));
    }

    let convexity = weighted_sum / (price * discount_factor_base.powi(2));
    Ok(Convexity::from(convexity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_analytical_convexity_par_bond() {
        // 2-year bond, 5% coupon, semi-annual
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];

        let conv = analytical_convexity(&times, &cash_flows, 0.05, 2).unwrap();

        // Convexity should be positive and reasonable
        assert!(conv.as_f64() > 0.0);
        assert!(conv.as_f64() < 10.0);
    }

    #[test]
    fn test_analytical_convexity_zero_coupon() {
        // 5-year zero coupon bond
        let times = vec![5.0];
        let cash_flows = vec![100.0];

        let conv = analytical_convexity(&times, &cash_flows, 0.05, 1).unwrap();

        // Zero coupon convexity ≈ t × (t + 1) / (1 + y)²
        // ≈ 5 × 6 / 1.1025 ≈ 27.2
        assert_relative_eq!(conv.as_f64(), 27.2, epsilon = 0.5);
    }
}
