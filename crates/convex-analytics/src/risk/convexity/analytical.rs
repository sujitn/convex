//! Analytical convexity calculation.

use convex_core::types::Compounding;

use super::Convexity;
use crate::error::{AnalyticsError, AnalyticsResult};

/// Analytical convexity. Periodic: `Σ t·(t+1/f)·PV / (P·(1+y/f)²)`.
/// Continuous: `Σ t²·PV / P` (limit as `f → ∞`).
/// Simple: `Σ 2·t²·PV / (P·(1+y·t)²)` (second derivative of `1/(1+y·t)`).
pub fn analytical_convexity(
    times: &[f64],
    cash_flows: &[f64],
    ytm: f64,
    compounding: Compounding,
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

    let mut weighted_sum = 0.0;
    let mut price = 0.0;
    for (&t, &cf) in times.iter().zip(cash_flows.iter()) {
        let (df, weight) = match compounding {
            Compounding::Continuous => ((-ytm * t).exp(), t * t),
            Compounding::Simple => {
                let g = 1.0 + ytm * t;
                (1.0 / g, 2.0 * t * t / (g * g))
            }
            _ => {
                let f = compounding.periods_per_year() as f64;
                let base = 1.0 + ytm / f;
                (base.powf(-t * f), t * (t + 1.0 / f))
            }
        };
        let pv = cf * df;
        weighted_sum += weight * pv;
        price += pv;
    }

    if price.abs() < 1e-10 {
        return Err(AnalyticsError::CalculationFailed(
            "price is zero in convexity calculation".to_string(),
        ));
    }

    // Periodic conventions divide by (1+y/f)²; continuous and simple already
    // bake the second-derivative scaling into the per-cash-flow `weight`.
    let scale = match compounding {
        Compounding::Continuous | Compounding::Simple => 1.0,
        _ => {
            let f = compounding.periods_per_year() as f64;
            (1.0 + ytm / f).powi(2)
        }
    };
    Ok(Convexity::from(weighted_sum / (price * scale)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_analytical_convexity_par_bond() {
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];
        let conv = analytical_convexity(&times, &cash_flows, 0.05, Compounding::SemiAnnual).unwrap();
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
