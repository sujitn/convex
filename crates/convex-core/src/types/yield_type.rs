//! Yield type for bond analytics.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

use super::Compounding;
use crate::error::{ConvexError, ConvexResult};

/// A yield value with compounding convention.
///
/// Yields are expressed as annualized rates (e.g., 0.05 = 5%).
///
/// # Example
///
/// ```rust
/// use convex_core::types::{Yield, Compounding};
/// use rust_decimal_macros::dec;
///
/// let ytm = Yield::new(dec!(0.05), Compounding::SemiAnnual);
/// assert_eq!(ytm.as_percentage(), dec!(5.0));
/// assert_eq!(ytm.as_bps(), 500);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Yield {
    /// Yield as a decimal (0.05 = 5%)
    value: Decimal,
    /// Compounding frequency convention
    compounding: Compounding,
}

impl Yield {
    /// Creates a new yield from a decimal value.
    ///
    /// The value should be expressed as a decimal (0.05 = 5%).
    #[must_use]
    pub fn new(value: Decimal, compounding: Compounding) -> Self {
        Self { value, compounding }
    }

    /// Creates a yield from a percentage value.
    #[must_use]
    pub fn from_percentage(percentage: Decimal, compounding: Compounding) -> Self {
        Self {
            value: percentage / Decimal::ONE_HUNDRED,
            compounding,
        }
    }

    /// Creates a yield from basis points.
    #[must_use]
    pub fn from_bps(bps: i32, compounding: Compounding) -> Self {
        Self {
            value: Decimal::from(bps) / Decimal::from(10_000),
            compounding,
        }
    }

    /// Validates the yield value.
    ///
    /// # Errors
    ///
    /// Returns `ConvexError::InvalidYield` if the yield is out of reasonable bounds.
    pub fn validate(&self) -> ConvexResult<()> {
        // Allow negative yields (common in some markets) but check for extreme values
        if self.value < Decimal::from(-1) || self.value > Decimal::from(1) {
            return Err(ConvexError::InvalidYield {
                value: self.value,
                reason: "Yield out of reasonable bounds (-100% to +100%)".into(),
            });
        }
        Ok(())
    }

    /// Returns the yield as a decimal (0.05 = 5%).
    #[must_use]
    pub fn value(&self) -> Decimal {
        self.value
    }

    /// Returns the yield as a percentage.
    #[must_use]
    pub fn as_percentage(&self) -> Decimal {
        self.value * Decimal::ONE_HUNDRED
    }

    /// Returns the yield in basis points.
    #[must_use]
    pub fn as_bps(&self) -> i64 {
        let bps = self.value * Decimal::from(10_000);
        bps.trunc().to_string().parse().unwrap_or(0)
    }

    /// Returns the compounding convention.
    #[must_use]
    pub fn compounding(&self) -> Compounding {
        self.compounding
    }

    /// Converts the yield to a different compounding convention.
    ///
    /// Uses the formula for equivalent yields:
    /// `(1 + r1/n1)^n1 = (1 + r2/n2)^n2`
    #[must_use]
    pub fn convert_to(&self, target: Compounding) -> Self {
        if self.compounding == target {
            return *self;
        }

        let n1 = self.compounding.periods_per_year() as f64;
        let n2 = target.periods_per_year() as f64;
        let r1 = self.value.to_string().parse::<f64>().unwrap_or(0.0);

        // (1 + r1/n1)^n1 = (1 + r2/n2)^n2
        // r2 = n2 * ((1 + r1/n1)^(n1/n2) - 1)
        let r2 = if target == Compounding::Continuous {
            // Convert to continuous: r_c = n * ln(1 + r/n)
            n1 * (1.0 + r1 / n1).ln()
        } else if self.compounding == Compounding::Continuous {
            // Convert from continuous: r = n * (e^(r_c/n) - 1)
            n2 * ((r1 / n2).exp() - 1.0)
        } else {
            n2 * ((1.0 + r1 / n1).powf(n1 / n2) - 1.0)
        };

        Self {
            value: Decimal::from_f64_retain(r2).unwrap_or(self.value),
            compounding: target,
        }
    }
}

impl fmt::Display for Yield {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.4}% ({})", self.as_percentage(), self.compounding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_yield_creation() {
        let y = Yield::new(dec!(0.05), Compounding::SemiAnnual);
        assert_eq!(y.value(), dec!(0.05));
        assert_eq!(y.as_percentage(), dec!(5.0));
        assert_eq!(y.compounding(), Compounding::SemiAnnual);
    }

    #[test]
    fn test_yield_from_bps() {
        let y = Yield::from_bps(250, Compounding::SemiAnnual);
        assert_eq!(y.value(), dec!(0.025));
        assert_eq!(y.as_bps(), 250);
    }

    #[test]
    fn test_yield_from_percentage() {
        let y = Yield::from_percentage(dec!(5.0), Compounding::SemiAnnual);
        assert_eq!(y.value(), dec!(0.05));
        assert_eq!(y.as_percentage(), dec!(5.0));
    }

    #[test]
    fn test_yield_validation() {
        let valid = Yield::new(dec!(0.05), Compounding::SemiAnnual);
        let invalid = Yield::new(dec!(2.0), Compounding::SemiAnnual);
        let negative_valid = Yield::new(dec!(-0.005), Compounding::SemiAnnual);

        assert!(valid.validate().is_ok());
        assert!(invalid.validate().is_err());
        assert!(negative_valid.validate().is_ok()); // Negative yields are allowed
    }

    #[test]
    fn test_yield_conversion_same_compounding() {
        let y = Yield::new(dec!(0.05), Compounding::SemiAnnual);
        let converted = y.convert_to(Compounding::SemiAnnual);
        assert_eq!(y, converted);
    }

    #[test]
    fn test_yield_conversion_different_compounding() {
        let semi = Yield::new(dec!(0.05), Compounding::SemiAnnual);
        let annual = semi.convert_to(Compounding::Annual);

        // Semi-annual 5% should convert to slightly higher annual rate
        assert!(annual.value() > semi.value());
        assert_eq!(annual.compounding(), Compounding::Annual);
    }

    #[test]
    fn test_display() {
        let y = Yield::new(dec!(0.05), Compounding::SemiAnnual);
        let display = format!("{}", y);
        assert!(display.contains("5.0"));
        assert!(display.contains("Semi-Annual"));
    }

    #[test]
    fn test_serde() {
        let y = Yield::new(dec!(0.05), Compounding::SemiAnnual);
        let json = serde_json::to_string(&y).unwrap();
        let parsed: Yield = serde_json::from_str(&json).unwrap();
        assert_eq!(y, parsed);
    }
}
