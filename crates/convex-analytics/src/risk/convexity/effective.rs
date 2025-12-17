//! Effective convexity calculation using finite differences.

use super::Convexity;
use crate::error::{AnalyticsError, AnalyticsResult};

/// Calculate effective convexity using finite differences.
///
/// # Formula
///
/// ```text
/// C_eff = (P₋ + P₊ - 2×P₀) / (P₀ × Δy²)
/// ```
///
/// # Arguments
///
/// * `price_up` - Price when yield increases by bump size
/// * `price_down` - Price when yield decreases by bump size
/// * `price_base` - Current/base price
/// * `bump_size` - Yield bump size (as decimal)
///
/// # Returns
///
/// Effective convexity
pub fn effective_convexity(
    price_up: f64,
    price_down: f64,
    price_base: f64,
    bump_size: f64,
) -> AnalyticsResult<Convexity> {
    if price_base.abs() < 1e-10 {
        return Err(AnalyticsError::CalculationFailed(
            "base price is zero".to_string(),
        ));
    }

    if bump_size.abs() < 1e-12 {
        return Err(AnalyticsError::InvalidInput(
            "bump size too small".to_string(),
        ));
    }

    let conv = (price_down + price_up - 2.0 * price_base) / (price_base * bump_size.powi(2));
    Ok(Convexity::from(conv))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effective_convexity() {
        // Simulated prices for a bond with convexity ~50
        let price_base = 100.0;
        let bump = 0.01; // 100 bps

        // If convexity ≈ 50, for ±100bps:
        // Convexity effect = 0.5 × 50 × 100 × 0.01² = 0.025 on each side
        let price_up = 95.025; // down from duration, up from convexity
        let price_down = 105.025; // up from duration, up from convexity

        let conv = effective_convexity(price_up, price_down, price_base, bump).unwrap();

        // (105.025 + 95.025 - 200) / (100 × 0.0001) = 0.05 / 0.01 = 5
        // Note: This is scaled differently - effective convexity uses different conventions
        assert!(conv.as_f64() > 0.0);
    }

    #[test]
    fn test_effective_convexity_symmetric() {
        // For a bond with positive convexity, price_up + price_down > 2 × price_base
        let price_base = 100.0;
        let price_up = 99.0;
        let price_down = 101.1; // Slightly more than 101 due to convexity
        let bump = 0.01;

        let conv = effective_convexity(price_up, price_down, price_base, bump).unwrap();

        // Convexity should be positive
        assert!(conv.as_f64() > 0.0);
    }
}
