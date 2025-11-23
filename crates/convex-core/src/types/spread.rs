//! Spread types for fixed income analytics.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Neg, Sub};

/// Types of spreads used in fixed income analytics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpreadType {
    /// Zero-volatility spread over benchmark curve.
    ZSpread,
    /// Spread over government curve at specific tenor (G-Spread).
    GSpread,
    /// Interpolated spread between two benchmark points (I-Spread).
    ISpread,
    /// Par-par asset swap spread.
    AssetSwapPar,
    /// Proceeds asset swap spread.
    AssetSwapProceeds,
    /// Option-adjusted spread (for callable/putable bonds).
    OAS,
    /// Generic credit spread.
    Credit,
}

impl fmt::Display for SpreadType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            SpreadType::ZSpread => "Z-Spread",
            SpreadType::GSpread => "G-Spread",
            SpreadType::ISpread => "I-Spread",
            SpreadType::AssetSwapPar => "ASW (Par)",
            SpreadType::AssetSwapProceeds => "ASW (Proceeds)",
            SpreadType::OAS => "OAS",
            SpreadType::Credit => "Credit",
        };
        write!(f, "{name}")
    }
}

/// A spread value in basis points.
///
/// Spreads represent the additional yield over a benchmark rate or curve.
///
/// # Example
///
/// ```rust
/// use convex_core::types::{Spread, SpreadType};
/// use rust_decimal_macros::dec;
///
/// let spread = Spread::new(dec!(125), SpreadType::ZSpread);
/// assert_eq!(spread.as_bps(), dec!(125));
/// assert_eq!(spread.as_decimal(), dec!(0.0125));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spread {
    /// Spread value in basis points
    value_bps: Decimal,
    /// Type of spread
    spread_type: SpreadType,
}

impl Spread {
    /// Creates a new spread from basis points.
    #[must_use]
    pub fn new(bps: Decimal, spread_type: SpreadType) -> Self {
        Self {
            value_bps: bps,
            spread_type,
        }
    }

    /// Creates a spread from a decimal value.
    #[must_use]
    pub fn from_decimal(decimal: Decimal, spread_type: SpreadType) -> Self {
        Self {
            value_bps: decimal * Decimal::from(10_000),
            spread_type,
        }
    }

    /// Returns the spread in basis points.
    #[must_use]
    pub fn as_bps(&self) -> Decimal {
        self.value_bps
    }

    /// Returns the spread as a decimal (125 bps = 0.0125).
    #[must_use]
    pub fn as_decimal(&self) -> Decimal {
        self.value_bps / Decimal::from(10_000)
    }

    /// Returns the spread as a percentage.
    #[must_use]
    pub fn as_percentage(&self) -> Decimal {
        self.value_bps / Decimal::ONE_HUNDRED
    }

    /// Returns the spread type.
    #[must_use]
    pub fn spread_type(&self) -> SpreadType {
        self.spread_type
    }

    /// Returns true if the spread is positive (credit risk premium).
    #[must_use]
    pub fn is_positive(&self) -> bool {
        self.value_bps > Decimal::ZERO
    }

    /// Returns true if the spread is negative (rare, usually indicates flight to quality).
    #[must_use]
    pub fn is_negative(&self) -> bool {
        self.value_bps < Decimal::ZERO
    }

    /// Returns true if the spread is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.value_bps == Decimal::ZERO
    }

    /// Creates a spread from an integer basis point value.
    #[must_use]
    pub fn from_bps_i32(bps: i32, spread_type: SpreadType) -> Self {
        Self {
            value_bps: Decimal::from(bps),
            spread_type,
        }
    }

    /// Returns the absolute value of the spread.
    #[must_use]
    pub fn abs(&self) -> Self {
        Self {
            value_bps: self.value_bps.abs(),
            spread_type: self.spread_type,
        }
    }

    /// Rounds the spread to the nearest basis point.
    #[must_use]
    pub fn round(&self) -> Self {
        Self {
            value_bps: self.value_bps.round(),
            spread_type: self.spread_type,
        }
    }

    /// Returns true if two spreads are of the same type.
    #[must_use]
    pub fn same_type(&self, other: &Self) -> bool {
        self.spread_type == other.spread_type
    }
}

impl Add for Spread {
    type Output = Self;

    /// Adds two spreads of the same type.
    ///
    /// # Panics
    ///
    /// Panics if the spreads are of different types.
    fn add(self, rhs: Self) -> Self::Output {
        assert!(
            self.spread_type == rhs.spread_type,
            "Cannot add spreads of different types"
        );
        Self {
            value_bps: self.value_bps + rhs.value_bps,
            spread_type: self.spread_type,
        }
    }
}

impl Sub for Spread {
    type Output = Self;

    /// Subtracts two spreads of the same type.
    ///
    /// # Panics
    ///
    /// Panics if the spreads are of different types.
    fn sub(self, rhs: Self) -> Self::Output {
        assert!(
            self.spread_type == rhs.spread_type,
            "Cannot subtract spreads of different types"
        );
        Self {
            value_bps: self.value_bps - rhs.value_bps,
            spread_type: self.spread_type,
        }
    }
}

impl Neg for Spread {
    type Output = Self;

    /// Negates the spread value.
    fn neg(self) -> Self::Output {
        Self {
            value_bps: -self.value_bps,
            spread_type: self.spread_type,
        }
    }
}

impl PartialOrd for Spread {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.spread_type != other.spread_type {
            None // Can't compare spreads of different types
        } else {
            self.value_bps.partial_cmp(&other.value_bps)
        }
    }
}

impl Default for SpreadType {
    fn default() -> Self {
        SpreadType::ZSpread
    }
}

impl fmt::Display for Spread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} bps ({})", self.value_bps, self.spread_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_spread_creation() {
        let spread = Spread::new(dec!(125), SpreadType::ZSpread);
        assert_eq!(spread.as_bps(), dec!(125));
        assert_eq!(spread.as_decimal(), dec!(0.0125));
        assert_eq!(spread.spread_type(), SpreadType::ZSpread);
    }

    #[test]
    fn test_spread_from_decimal() {
        let spread = Spread::from_decimal(dec!(0.0125), SpreadType::GSpread);
        assert_eq!(spread.as_bps(), dec!(125));
    }

    #[test]
    fn test_spread_from_bps_i32() {
        let spread = Spread::from_bps_i32(125, SpreadType::ISpread);
        assert_eq!(spread.as_bps(), dec!(125));
        assert_eq!(spread.spread_type(), SpreadType::ISpread);
    }

    #[test]
    fn test_spread_sign() {
        let positive = Spread::new(dec!(100), SpreadType::Credit);
        let negative = Spread::new(dec!(-10), SpreadType::Credit);
        let zero = Spread::new(dec!(0), SpreadType::Credit);

        assert!(positive.is_positive());
        assert!(!positive.is_negative());
        assert!(!positive.is_zero());

        assert!(negative.is_negative());
        assert!(!negative.is_positive());
        assert!(!negative.is_zero());

        assert!(zero.is_zero());
        assert!(!zero.is_positive());
        assert!(!zero.is_negative());
    }

    #[test]
    fn test_spread_arithmetic() {
        let s1 = Spread::new(dec!(100), SpreadType::ZSpread);
        let s2 = Spread::new(dec!(50), SpreadType::ZSpread);

        let sum = s1 + s2;
        assert_eq!(sum.as_bps(), dec!(150));

        let diff = s1 - s2;
        assert_eq!(diff.as_bps(), dec!(50));

        let negated = -s1;
        assert_eq!(negated.as_bps(), dec!(-100));
    }

    #[test]
    fn test_spread_abs() {
        let negative = Spread::new(dec!(-50), SpreadType::Credit);
        let positive = negative.abs();
        assert_eq!(positive.as_bps(), dec!(50));
    }

    #[test]
    fn test_spread_round() {
        let spread = Spread::new(dec!(125.5), SpreadType::ZSpread);
        let rounded = spread.round();
        assert_eq!(rounded.as_bps(), dec!(126));
    }

    #[test]
    fn test_spread_comparison() {
        let s1 = Spread::new(dec!(100), SpreadType::ZSpread);
        let s2 = Spread::new(dec!(150), SpreadType::ZSpread);
        let s3 = Spread::new(dec!(100), SpreadType::GSpread);

        assert!(s1 < s2);
        assert!(s2 > s1);
        assert!(s1.partial_cmp(&s3).is_none()); // Different types
    }

    #[test]
    fn test_same_type() {
        let s1 = Spread::new(dec!(100), SpreadType::ZSpread);
        let s2 = Spread::new(dec!(150), SpreadType::ZSpread);
        let s3 = Spread::new(dec!(100), SpreadType::GSpread);

        assert!(s1.same_type(&s2));
        assert!(!s1.same_type(&s3));
    }

    #[test]
    fn test_spread_as_percentage() {
        let spread = Spread::new(dec!(125), SpreadType::ZSpread);
        assert_eq!(spread.as_percentage(), dec!(1.25)); // 125 bps = 1.25%
    }

    #[test]
    fn test_display() {
        let spread = Spread::new(dec!(125), SpreadType::ZSpread);
        let display = format!("{}", spread);
        assert!(display.contains("125"));
        assert!(display.contains("Z-Spread"));
    }

    #[test]
    fn test_spread_type_display() {
        assert_eq!(format!("{}", SpreadType::ZSpread), "Z-Spread");
        assert_eq!(format!("{}", SpreadType::GSpread), "G-Spread");
        assert_eq!(format!("{}", SpreadType::ISpread), "I-Spread");
        assert_eq!(format!("{}", SpreadType::OAS), "OAS");
    }

    #[test]
    fn test_serde() {
        let spread = Spread::new(dec!(125), SpreadType::ZSpread);
        let json = serde_json::to_string(&spread).unwrap();
        let parsed: Spread = serde_json::from_str(&json).unwrap();
        assert_eq!(spread, parsed);
    }
}
