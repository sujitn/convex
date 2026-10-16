//! Full-cash analytical convexity from the native checked derivative owner.
use super::Convexity;
use crate::error::AnalyticsResult;
use convex_core::types::Compounding;

/// Second annual-yield derivative divided by the same complete cash PV.
pub fn analytical_convexity(
    times: &[f64],
    cash_flows: &[f64],
    ytm: f64,
    compounding: Compounding,
) -> AnalyticsResult<Convexity> {
    let risk = crate::risk::cash::evaluate(times, cash_flows, ytm, compounding)?;
    Ok(Convexity::new(crate::risk::cash::decimal(risk.convexity)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_analytical_convexity_par_bond() {
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];
        let conv =
            analytical_convexity(&times, &cash_flows, 0.05, Compounding::SemiAnnual).unwrap();
        assert!(conv.as_f64() > 0.0 && conv.as_f64() < 10.0);
    }

    #[test]
    fn test_analytical_convexity_zero_coupon_annual() {
        // C ≈ t·(t+1)/(1+y)² = 30 / 1.1025 ≈ 27.2
        let conv = analytical_convexity(&[5.0], &[100.0], 0.05, Compounding::Annual).unwrap();
        assert_relative_eq!(conv.as_f64(), 27.2, epsilon = 0.5);
    }

    #[test]
    fn test_analytical_convexity_zero_coupon_continuous() {
        // For continuous compounding C = t² exactly (5² = 25), regardless of yield.
        let conv = analytical_convexity(&[5.0], &[100.0], 0.05, Compounding::Continuous).unwrap();
        assert_relative_eq!(conv.as_f64(), 25.0, epsilon = 1e-10);
    }
}
