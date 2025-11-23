//! Price type for bond pricing.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Sub};

use super::Currency;
use crate::error::{ConvexError, ConvexResult};

/// A bond price with currency.
///
/// Prices are typically quoted as a percentage of face value (e.g., 98.50 = 98.50%).
///
/// # Example
///
/// ```rust
/// use convex_core::types::{Price, Currency};
/// use rust_decimal_macros::dec;
///
/// let price = Price::new(dec!(98.50), Currency::USD);
/// assert_eq!(price.as_percentage(), dec!(98.50));
/// assert_eq!(price.as_decimal(), dec!(0.9850));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Price {
    /// Price as a percentage of par (e.g., 98.50 for 98.50%)
    value: Decimal,
    /// Currency of the price
    currency: Currency,
}

impl Price {
    /// Creates a new price from a percentage value.
    ///
    /// The value should be expressed as a percentage of par (e.g., 98.50 for 98.50%).
    #[must_use]
    pub fn new(percentage: Decimal, currency: Currency) -> Self {
        Self {
            value: percentage,
            currency,
        }
    }

    /// Creates a price from a decimal value (0.985 = 98.5%).
    #[must_use]
    pub fn from_decimal(decimal: Decimal, currency: Currency) -> Self {
        Self {
            value: decimal * Decimal::ONE_HUNDRED,
            currency,
        }
    }

    /// Validates that the price is positive.
    ///
    /// # Errors
    ///
    /// Returns `ConvexError::InvalidPrice` if the price is not positive.
    pub fn validate(&self) -> ConvexResult<()> {
        if self.value <= Decimal::ZERO {
            return Err(ConvexError::InvalidPrice {
                value: self.value,
                reason: "Price must be positive".into(),
            });
        }
        Ok(())
    }

    /// Returns the price as a percentage of par.
    #[must_use]
    pub fn as_percentage(&self) -> Decimal {
        self.value
    }

    /// Returns the price as a decimal (percentage / 100).
    #[must_use]
    pub fn as_decimal(&self) -> Decimal {
        self.value / Decimal::ONE_HUNDRED
    }

    /// Returns the currency.
    #[must_use]
    pub fn currency(&self) -> Currency {
        self.currency
    }

    /// Returns true if the bond is trading at par.
    #[must_use]
    pub fn is_at_par(&self) -> bool {
        self.value == Decimal::ONE_HUNDRED
    }

    /// Returns true if the bond is trading at a discount.
    #[must_use]
    pub fn is_discount(&self) -> bool {
        self.value < Decimal::ONE_HUNDRED
    }

    /// Returns true if the bond is trading at a premium.
    #[must_use]
    pub fn is_premium(&self) -> bool {
        self.value > Decimal::ONE_HUNDRED
    }

    /// Creates a par price (100) in the given currency.
    #[must_use]
    pub fn par(currency: Currency) -> Self {
        Self {
            value: Decimal::ONE_HUNDRED,
            currency,
        }
    }

    /// Calculates the price difference from par.
    ///
    /// Positive for premium bonds, negative for discount bonds.
    #[must_use]
    pub fn discount_or_premium(&self) -> Decimal {
        self.value - Decimal::ONE_HUNDRED
    }

    /// Calculates dirty price from clean price and accrued interest.
    ///
    /// # Arguments
    ///
    /// * `accrued` - Accrued interest as percentage of par
    #[must_use]
    pub fn to_dirty(&self, accrued: Decimal) -> Self {
        Self {
            value: self.value + accrued,
            currency: self.currency,
        }
    }

    /// Calculates clean price from dirty price and accrued interest.
    ///
    /// # Arguments
    ///
    /// * `accrued` - Accrued interest as percentage of par
    #[must_use]
    pub fn to_clean(&self, accrued: Decimal) -> Self {
        Self {
            value: self.value - accrued,
            currency: self.currency,
        }
    }

    /// Calculates the dollar value for a given face amount.
    ///
    /// # Arguments
    ///
    /// * `face_value` - Face value of the position
    #[must_use]
    pub fn dollar_value(&self, face_value: Decimal) -> Decimal {
        self.as_decimal() * face_value
    }

    /// Rounds the price to the specified number of decimal places.
    #[must_use]
    pub fn round(&self, decimal_places: u32) -> Self {
        Self {
            value: self.value.round_dp(decimal_places),
            currency: self.currency,
        }
    }

    /// Returns true if prices have the same currency.
    #[must_use]
    pub fn same_currency(&self, other: &Self) -> bool {
        self.currency == other.currency
    }
}

impl Add<Decimal> for Price {
    type Output = Self;

    /// Adds a decimal value to the price.
    fn add(self, rhs: Decimal) -> Self::Output {
        Self {
            value: self.value + rhs,
            currency: self.currency,
        }
    }
}

impl Sub<Decimal> for Price {
    type Output = Self;

    /// Subtracts a decimal value from the price.
    fn sub(self, rhs: Decimal) -> Self::Output {
        Self {
            value: self.value - rhs,
            currency: self.currency,
        }
    }
}

impl PartialOrd for Price {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.currency != other.currency {
            None // Can't compare prices in different currencies
        } else {
            self.value.partial_cmp(&other.value)
        }
    }
}

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.4} {}", self.value, self.currency)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_price_creation() {
        let price = Price::new(dec!(98.50), Currency::USD);
        assert_eq!(price.as_percentage(), dec!(98.50));
        assert_eq!(price.currency(), Currency::USD);
    }

    #[test]
    fn test_price_from_decimal() {
        let price = Price::from_decimal(dec!(0.985), Currency::USD);
        assert_eq!(price.as_percentage(), dec!(98.5));
        assert_eq!(price.as_decimal(), dec!(0.985));
    }

    #[test]
    fn test_discount_premium() {
        let discount = Price::new(dec!(98.50), Currency::USD);
        let premium = Price::new(dec!(101.50), Currency::USD);
        let par = Price::new(dec!(100.00), Currency::USD);

        assert!(discount.is_discount());
        assert!(!discount.is_premium());
        assert!(premium.is_premium());
        assert!(!premium.is_discount());
        assert!(par.is_at_par());
        assert!(!par.is_discount());
        assert!(!par.is_premium());
    }

    #[test]
    fn test_price_validation() {
        let valid = Price::new(dec!(100.00), Currency::USD);
        let invalid = Price::new(dec!(-1.00), Currency::USD);

        assert!(valid.validate().is_ok());
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_par_price() {
        let par = Price::par(Currency::EUR);
        assert_eq!(par.as_percentage(), dec!(100));
        assert_eq!(par.currency(), Currency::EUR);
        assert!(par.is_at_par());
    }

    #[test]
    fn test_discount_or_premium() {
        let discount = Price::new(dec!(98.50), Currency::USD);
        let premium = Price::new(dec!(101.50), Currency::USD);
        let par = Price::par(Currency::USD);

        assert_eq!(discount.discount_or_premium(), dec!(-1.50));
        assert_eq!(premium.discount_or_premium(), dec!(1.50));
        assert_eq!(par.discount_or_premium(), dec!(0));
    }

    #[test]
    fn test_dirty_clean_conversion() {
        let clean = Price::new(dec!(98.50), Currency::USD);
        let accrued = dec!(1.25);

        let dirty = clean.to_dirty(accrued);
        assert_eq!(dirty.as_percentage(), dec!(99.75));

        let back_to_clean = dirty.to_clean(accrued);
        assert_eq!(back_to_clean.as_percentage(), dec!(98.50));
    }

    #[test]
    fn test_dollar_value() {
        let price = Price::new(dec!(98.50), Currency::USD);
        let face_value = dec!(1_000_000);

        let dollar_value = price.dollar_value(face_value);
        assert_eq!(dollar_value, dec!(985_000));
    }

    #[test]
    fn test_price_rounding() {
        let price = Price::new(dec!(98.12345), Currency::USD);

        let rounded = price.round(2);
        assert_eq!(rounded.as_percentage(), dec!(98.12));

        // rust_decimal uses banker's rounding (half to even)
        // 98.12345 -> 98.1234 (rounds down to even)
        let rounded4 = price.round(4);
        assert_eq!(rounded4.as_percentage(), dec!(98.1234));

        // Test a value that rounds up
        let price2 = Price::new(dec!(98.12355), Currency::USD);
        let rounded4_up = price2.round(4);
        assert_eq!(rounded4_up.as_percentage(), dec!(98.1236));
    }

    #[test]
    fn test_price_arithmetic() {
        let price = Price::new(dec!(98.50), Currency::USD);

        let added = price + dec!(1.50);
        assert_eq!(added.as_percentage(), dec!(100.00));

        let subtracted = price - dec!(0.50);
        assert_eq!(subtracted.as_percentage(), dec!(98.00));
    }

    #[test]
    fn test_price_comparison() {
        let p1 = Price::new(dec!(98.50), Currency::USD);
        let p2 = Price::new(dec!(99.50), Currency::USD);
        let p3 = Price::new(dec!(98.50), Currency::EUR);

        assert!(p1 < p2);
        assert!(p2 > p1);
        assert!(p1.partial_cmp(&p3).is_none()); // Different currencies
    }

    #[test]
    fn test_same_currency() {
        let p1 = Price::new(dec!(98.50), Currency::USD);
        let p2 = Price::new(dec!(99.50), Currency::USD);
        let p3 = Price::new(dec!(98.50), Currency::EUR);

        assert!(p1.same_currency(&p2));
        assert!(!p1.same_currency(&p3));
    }

    #[test]
    fn test_display() {
        let price = Price::new(dec!(98.5), Currency::USD);
        assert_eq!(format!("{}", price), "98.5000 USD");
    }

    #[test]
    fn test_serde() {
        let price = Price::new(dec!(98.50), Currency::USD);
        let json = serde_json::to_string(&price).unwrap();
        let parsed: Price = serde_json::from_str(&json).unwrap();
        assert_eq!(price, parsed);
    }
}
