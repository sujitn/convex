//! Convexity calculations for fixed income instruments.
//!
//! Convexity measures the curvature of the price-yield relationship,
//! capturing the second-order effect that duration misses.
//!
//! ## Formula
//!
//! Analytical convexity:
//! ```text
//! C = Σ(t_i × (t_i + 1/f) × PV(CF_i)) / (P × (1 + y/f)²)
//! ```
//!
//! Effective convexity:
//! ```text
//! C_eff = (P₋ + P₊ - 2×P₀) / (P₀ × Δy²)
//! ```

mod analytical;
mod effective;

pub use analytical::*;
pub use effective::*;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Convexity value
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Convexity(Decimal);

impl Convexity {
    /// Create a new Convexity value
    pub fn new(value: Decimal) -> Self {
        Self(value)
    }

    /// Get the convexity value
    pub fn value(&self) -> Decimal {
        self.0
    }

    /// Get the convexity as f64
    pub fn as_f64(&self) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        self.0.to_f64().unwrap_or(0.0)
    }
}

impl std::fmt::Display for Convexity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.4}", self.0)
    }
}

impl From<Decimal> for Convexity {
    fn from(d: Decimal) -> Self {
        Self(d)
    }
}

impl From<f64> for Convexity {
    fn from(f: f64) -> Self {
        Self(Decimal::from_f64_retain(f).unwrap_or(Decimal::ZERO))
    }
}

/// Calculate price change including both duration and convexity effects.
///
/// # Formula
///
/// ```text
/// ΔP/P ≈ -D_mod × Δy + (1/2) × C × (Δy)²
/// ```
pub fn price_change_with_convexity(
    mod_duration: f64,
    convexity: f64,
    price: f64,
    yield_change: f64,
) -> f64 {
    let duration_effect = -mod_duration * price * yield_change;
    let convexity_effect = 0.5 * convexity * price * yield_change.powi(2);
    duration_effect + convexity_effect
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_price_change_with_convexity() {
        let mod_dur = 5.0;
        let conv = 50.0;
        let price = 100.0;
        let yield_change = 0.01; // 100 bps

        let change = price_change_with_convexity(mod_dur, conv, price, yield_change);

        // Duration effect: -5 × 100 × 0.01 = -5.0
        // Convexity effect: 0.5 × 50 × 100 × 0.0001 = 0.25
        // Total: -4.75
        assert_relative_eq!(change, -4.75, epsilon = 0.001);
    }
}
