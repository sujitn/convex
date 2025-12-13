//! Yield calculation method.
//!
//! This module defines the yield calculation methodology used for bond analytics.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Yield calculation methodology.
///
/// Controls HOW to calculate yield from price. The bond itself owns the
/// conventions (day count, frequency) - this enum only controls the
/// mathematical approach.
///
/// # Example
///
/// ```rust
/// use convex_core::types::YieldMethod;
///
/// let method = YieldMethod::Compounded;
/// assert!(!method.is_simple());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum YieldMethod {
    /// Compounded yield (Newton-Raphson).
    ///
    /// Standard yield-to-maturity calculation using compound discounting:
    /// ```text
    /// Price = Σ CF_i / (1 + y/f)^(f × t_i)
    /// ```
    ///
    /// Used for: US Street Convention, ISMA/ICMA, True Yield, etc.
    /// This is the most common method for coupon-bearing bonds.
    #[default]
    Compounded,

    /// Simple yield (no compounding).
    ///
    /// Linear yield calculation without compounding effects:
    /// ```text
    /// y = (Annual Coupon + (Redemption - Price) / Years) / Price
    /// ```
    ///
    /// Used for: Japanese JGBs and some Asian markets.
    Simple,

    /// Discount yield (bank discount basis).
    ///
    /// Used for zero-coupon instruments like T-Bills:
    /// ```text
    /// y = (Face - Price) / Face × (Basis / Days)
    /// ```
    ///
    /// The basis is typically 360 for US markets.
    Discount,

    /// Add-on yield (money market).
    ///
    /// Used for money market instruments and short-dated bonds:
    /// ```text
    /// y = (Face - Price) / Price × (Basis / Days)
    /// ```
    ///
    /// For coupon bonds under the money market threshold, this uses
    /// sequential roll-forward with simple interest reinvestment.
    AddOn,
}

impl YieldMethod {
    /// Returns true if this method uses simple (non-compounding) interest.
    ///
    /// Simple and Discount methods are considered "simple" - they don't
    /// compound interest over time.
    #[must_use]
    pub const fn is_simple(&self) -> bool {
        matches!(self, YieldMethod::Simple | YieldMethod::Discount)
    }

    /// Returns true if this method uses compounding.
    #[must_use]
    pub const fn is_compounded(&self) -> bool {
        matches!(self, YieldMethod::Compounded)
    }

    /// Returns true if this is a money market method.
    ///
    /// Both Discount and AddOn are money market methods, typically used
    /// for short-dated instruments.
    #[must_use]
    pub const fn is_money_market(&self) -> bool {
        matches!(self, YieldMethod::Discount | YieldMethod::AddOn)
    }

    /// Returns a human-readable description of this method.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            YieldMethod::Compounded => "Compounded yield (Newton-Raphson)",
            YieldMethod::Simple => "Simple yield (Japanese convention)",
            YieldMethod::Discount => "Discount yield (bank discount basis)",
            YieldMethod::AddOn => "Add-on yield (money market)",
        }
    }
}

impl fmt::Display for YieldMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            YieldMethod::Compounded => "Compounded",
            YieldMethod::Simple => "Simple",
            YieldMethod::Discount => "Discount",
            YieldMethod::AddOn => "Add-On",
        };
        write!(f, "{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yield_method_default() {
        let method = YieldMethod::default();
        assert_eq!(method, YieldMethod::Compounded);
    }

    #[test]
    fn test_yield_method_is_simple() {
        assert!(!YieldMethod::Compounded.is_simple());
        assert!(YieldMethod::Simple.is_simple());
        assert!(YieldMethod::Discount.is_simple());
        assert!(!YieldMethod::AddOn.is_simple());
    }

    #[test]
    fn test_yield_method_is_compounded() {
        assert!(YieldMethod::Compounded.is_compounded());
        assert!(!YieldMethod::Simple.is_compounded());
        assert!(!YieldMethod::Discount.is_compounded());
        assert!(!YieldMethod::AddOn.is_compounded());
    }

    #[test]
    fn test_yield_method_is_money_market() {
        assert!(!YieldMethod::Compounded.is_money_market());
        assert!(!YieldMethod::Simple.is_money_market());
        assert!(YieldMethod::Discount.is_money_market());
        assert!(YieldMethod::AddOn.is_money_market());
    }

    #[test]
    fn test_yield_method_display() {
        assert_eq!(format!("{}", YieldMethod::Compounded), "Compounded");
        assert_eq!(format!("{}", YieldMethod::Simple), "Simple");
        assert_eq!(format!("{}", YieldMethod::Discount), "Discount");
        assert_eq!(format!("{}", YieldMethod::AddOn), "Add-On");
    }

    #[test]
    fn test_yield_method_description() {
        assert!(YieldMethod::Compounded.description().contains("Newton-Raphson"));
        assert!(YieldMethod::Simple.description().contains("Japanese"));
        assert!(YieldMethod::Discount.description().contains("bank discount"));
        assert!(YieldMethod::AddOn.description().contains("money market"));
    }

    #[test]
    fn test_serde() {
        let method = YieldMethod::Compounded;
        let json = serde_json::to_string(&method).unwrap();
        let parsed: YieldMethod = serde_json::from_str(&json).unwrap();
        assert_eq!(method, parsed);

        // Test all variants
        for m in [
            YieldMethod::Compounded,
            YieldMethod::Simple,
            YieldMethod::Discount,
            YieldMethod::AddOn,
        ] {
            let json = serde_json::to_string(&m).unwrap();
            let parsed: YieldMethod = serde_json::from_str(&json).unwrap();
            assert_eq!(m, parsed);
        }
    }
}
