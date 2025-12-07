//! Effective duration calculation.
//!
//! Effective duration uses finite differences to measure price sensitivity.
//! This is essential for bonds with embedded options where analytical
//! duration formulas don't apply.
//!
//! ## Formula
//!
//! ```text
//! D_eff = (P₋ - P₊) / (2 × P₀ × Δy)
//! ```
//!
//! where:
//! - P₋ = price when yield decreases by Δy
//! - P₊ = price when yield increases by Δy
//! - P₀ = current price
//! - Δy = yield bump size

use super::Duration;
use crate::RiskError;

/// Calculate effective duration using finite differences.
///
/// # Arguments
///
/// * `price_up` - Price when yield increases by bump size
/// * `price_down` - Price when yield decreases by bump size
/// * `price_base` - Current/base price
/// * `bump_size` - Yield bump size (as decimal, e.g., 0.0001 for 1bp)
///
/// # Returns
///
/// Effective duration
pub fn effective_duration(
    price_up: f64,
    price_down: f64,
    price_base: f64,
    bump_size: f64,
) -> Result<Duration, RiskError> {
    if price_base.abs() < 1e-10 {
        return Err(RiskError::DivisionByZero {
            context: "base price is zero".to_string(),
        });
    }

    if bump_size.abs() < 1e-12 {
        return Err(RiskError::InvalidInput("bump size too small".to_string()));
    }

    let eff_dur = (price_down - price_up) / (2.0 * price_base * bump_size);
    Ok(Duration::from(eff_dur))
}

/// Standard bump size for effective duration (10 basis points)
pub const DEFAULT_BUMP_SIZE: f64 = 0.001;

/// Small bump size for numerical precision (1 basis point)
pub const SMALL_BUMP_SIZE: f64 = 0.0001;

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_effective_duration() {
        // Simulated prices for a bond with modified duration ~5
        let price_base = 100.0;
        let bump = 0.001; // 10 bps

        // If mod dur ≈ 5, then for +10bps: ΔP ≈ -5 × 100 × 0.001 = -0.5
        let price_up = 99.5;
        let price_down = 100.5;

        let dur = effective_duration(price_up, price_down, price_base, bump).unwrap();

        assert_relative_eq!(dur.as_f64(), 5.0, epsilon = 0.01);
    }

    #[test]
    fn test_effective_duration_zero_price_error() {
        let result = effective_duration(99.5, 100.5, 0.0, 0.001);
        assert!(result.is_err());
    }

    #[test]
    fn test_effective_duration_zero_bump_error() {
        let result = effective_duration(99.5, 100.5, 100.0, 0.0);
        assert!(result.is_err());
    }
}
