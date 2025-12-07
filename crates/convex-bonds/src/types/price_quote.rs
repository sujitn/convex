//! Price quote conventions for bond markets.
//!
//! Different bond markets quote prices in different formats:
//! - Most markets: decimal (e.g., 99.50)
//! - US Treasuries: 32nds (e.g., 99-16 = 99.50)
//! - T-Bills: discount yield
//!
//! This module provides types and parsing for these conventions.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::error::BondError;

/// Price quote convention used in different bond markets.
///
/// # Example
///
/// ```rust
/// use convex_bonds::types::PriceQuoteConvention;
///
/// let convention = PriceQuoteConvention::ThirtySeconds;
/// assert!(convention.is_fractional());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PriceQuoteConvention {
    /// Decimal price (e.g., 99.50).
    ///
    /// Standard convention for most markets.
    #[default]
    Decimal,

    /// Price in 32nds (e.g., 99-16 = 99.50).
    ///
    /// Used for US Treasury notes and bonds.
    /// Format: "XX-YY" where YY is 32nds (00-31).
    ThirtySeconds,

    /// Price in 32nds with plus (e.g., 99-16+ = 99.515625).
    ///
    /// The "+" adds 1/64 to the price.
    /// Used for very liquid Treasury securities.
    ThirtySecondsPlus,

    /// Price in 64ths (e.g., 99-32 = 99.50).
    ///
    /// Used for Treasury STRIPS and some agency securities.
    SixtyFourths,

    /// Price in 128ths.
    ///
    /// Used for mortgage-backed securities.
    OneHundredTwentyEighths,

    /// Discount rate (T-Bills).
    ///
    /// Quote is the discount yield, not the price.
    /// Price = 100 × (1 - Rate × Days / 360)
    Discount,

    /// Yield quote.
    ///
    /// Some markets quote yield rather than price.
    Yield,

    /// Percentage of par (same as decimal but explicitly stated).
    Percentage,

    /// Price per unit (e.g., per 1 JPY face for JGBs).
    PerUnit,
}

impl PriceQuoteConvention {
    /// Returns true if this is a fractional quote convention.
    #[must_use]
    pub const fn is_fractional(&self) -> bool {
        matches!(
            self,
            PriceQuoteConvention::ThirtySeconds
                | PriceQuoteConvention::ThirtySecondsPlus
                | PriceQuoteConvention::SixtyFourths
                | PriceQuoteConvention::OneHundredTwentyEighths
        )
    }

    /// Returns true if this quotes a rate rather than a price.
    #[must_use]
    pub const fn is_rate_quote(&self) -> bool {
        matches!(
            self,
            PriceQuoteConvention::Discount | PriceQuoteConvention::Yield
        )
    }

    /// Returns the denominator for fractional quotes.
    #[must_use]
    pub const fn denominator(&self) -> Option<u32> {
        match self {
            PriceQuoteConvention::ThirtySeconds | PriceQuoteConvention::ThirtySecondsPlus => {
                Some(32)
            }
            PriceQuoteConvention::SixtyFourths => Some(64),
            PriceQuoteConvention::OneHundredTwentyEighths => Some(128),
            _ => None,
        }
    }

    /// Returns the minimum tick size for this convention.
    #[must_use]
    pub fn tick_size(&self) -> Decimal {
        match self {
            PriceQuoteConvention::Decimal => Decimal::new(1, 2), // 0.01
            PriceQuoteConvention::ThirtySeconds => Decimal::new(3125, 6), // 1/32 = 0.03125
            PriceQuoteConvention::ThirtySecondsPlus => Decimal::new(15625, 7), // 1/64 = 0.015625
            PriceQuoteConvention::SixtyFourths => Decimal::new(15625, 7), // 1/64
            PriceQuoteConvention::OneHundredTwentyEighths => Decimal::new(78125, 8), // 1/128
            PriceQuoteConvention::Discount => Decimal::new(1, 4), // 0.0001 (1 bp)
            PriceQuoteConvention::Yield => Decimal::new(1, 4),   // 0.0001 (1 bp)
            PriceQuoteConvention::Percentage => Decimal::new(1, 2), // 0.01
            PriceQuoteConvention::PerUnit => Decimal::new(1, 4), // 0.0001
        }
    }
}

impl std::fmt::Display for PriceQuoteConvention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PriceQuoteConvention::Decimal => "Decimal",
            PriceQuoteConvention::ThirtySeconds => "32nds",
            PriceQuoteConvention::ThirtySecondsPlus => "32nds+",
            PriceQuoteConvention::SixtyFourths => "64ths",
            PriceQuoteConvention::OneHundredTwentyEighths => "128ths",
            PriceQuoteConvention::Discount => "Discount",
            PriceQuoteConvention::Yield => "Yield",
            PriceQuoteConvention::Percentage => "Percentage",
            PriceQuoteConvention::PerUnit => "Per Unit",
        };
        write!(f, "{s}")
    }
}

/// A price quote that can represent different conventions.
///
/// This struct provides parsing and conversion between quote formats.
///
/// # Example
///
/// ```rust
/// use convex_bonds::types::PriceQuote;
///
/// // Parse Treasury price in 32nds
/// let quote = PriceQuote::from_thirty_seconds(99, 16, false).unwrap();
/// assert_eq!(quote.decimal_price().to_string(), "99.50");
///
/// // Parse with plus (adds 1/64)
/// let quote_plus = PriceQuote::from_thirty_seconds(99, 16, true).unwrap();
/// assert_eq!(quote_plus.decimal_price().to_string(), "99.515625");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceQuote {
    /// The decimal price value.
    decimal: Decimal,
    /// The original quote convention.
    convention: PriceQuoteConvention,
    /// Original quote string (if parsed from string).
    original: Option<String>,
}

impl PriceQuote {
    /// Creates a new price quote from a decimal value.
    #[must_use]
    pub fn new(decimal: Decimal) -> Self {
        Self {
            decimal,
            convention: PriceQuoteConvention::Decimal,
            original: None,
        }
    }

    /// Creates a price quote from 32nds notation.
    ///
    /// # Arguments
    /// * `handle` - The whole number part (e.g., 99 in 99-16)
    /// * `thirty_seconds` - The 32nds part (0-31)
    /// * `plus` - Whether to add 1/64 (the "+" notation)
    ///
    /// # Errors
    /// Returns error if `thirty_seconds` > 31.
    pub fn from_thirty_seconds(
        handle: u32,
        thirty_seconds: u32,
        plus: bool,
    ) -> Result<Self, BondError> {
        if thirty_seconds > 31 {
            return Err(BondError::InvalidPrice {
                reason: format!("32nds value must be 0-31, got {thirty_seconds}"),
            });
        }

        let handle_dec = Decimal::from(handle);
        let frac = Decimal::from(thirty_seconds) / Decimal::from(32);
        let plus_adj = if plus {
            Decimal::ONE / Decimal::from(64)
        } else {
            Decimal::ZERO
        };

        let decimal = handle_dec + frac + plus_adj;
        let convention = if plus {
            PriceQuoteConvention::ThirtySecondsPlus
        } else {
            PriceQuoteConvention::ThirtySeconds
        };

        let original = if plus {
            format!("{handle}-{thirty_seconds}+")
        } else {
            format!("{handle}-{thirty_seconds:02}")
        };

        Ok(Self {
            decimal,
            convention,
            original: Some(original),
        })
    }

    /// Creates a price quote from 64ths notation.
    ///
    /// # Errors
    /// Returns error if `sixty_fourths` > 63.
    pub fn from_sixty_fourths(handle: u32, sixty_fourths: u32) -> Result<Self, BondError> {
        if sixty_fourths > 63 {
            return Err(BondError::InvalidPrice {
                reason: format!("64ths value must be 0-63, got {sixty_fourths}"),
            });
        }

        let handle_dec = Decimal::from(handle);
        let frac = Decimal::from(sixty_fourths) / Decimal::from(64);
        let decimal = handle_dec + frac;

        Ok(Self {
            decimal,
            convention: PriceQuoteConvention::SixtyFourths,
            original: Some(format!("{handle}-{sixty_fourths:02}")),
        })
    }

    /// Parses a price quote string based on the given convention.
    ///
    /// # Formats
    /// - Decimal: "99.50"
    /// - 32nds: "99-16" or "99-16+"
    /// - 64ths: "99-32" (context-dependent)
    ///
    /// # Errors
    /// Returns error for invalid format.
    pub fn parse(s: &str, convention: PriceQuoteConvention) -> Result<Self, BondError> {
        let s = s.trim();

        match convention {
            PriceQuoteConvention::Decimal | PriceQuoteConvention::Percentage => {
                let decimal = Decimal::from_str(s).map_err(|e| BondError::InvalidPrice {
                    reason: format!("Invalid decimal price '{s}': {e}"),
                })?;
                Ok(Self {
                    decimal,
                    convention,
                    original: Some(s.to_string()),
                })
            }

            PriceQuoteConvention::ThirtySeconds | PriceQuoteConvention::ThirtySecondsPlus => {
                Self::parse_thirty_seconds(s)
            }

            PriceQuoteConvention::SixtyFourths => Self::parse_sixty_fourths(s),

            PriceQuoteConvention::Discount | PriceQuoteConvention::Yield => {
                // For rate quotes, store the rate as-is
                let rate = Decimal::from_str(s).map_err(|e| BondError::InvalidPrice {
                    reason: format!("Invalid rate '{s}': {e}"),
                })?;
                Ok(Self {
                    decimal: rate,
                    convention,
                    original: Some(s.to_string()),
                })
            }

            _ => {
                let decimal = Decimal::from_str(s).map_err(|e| BondError::InvalidPrice {
                    reason: format!("Invalid price '{s}': {e}"),
                })?;
                Ok(Self {
                    decimal,
                    convention,
                    original: Some(s.to_string()),
                })
            }
        }
    }

    /// Parses 32nds notation (e.g., "99-16" or "99-16+").
    fn parse_thirty_seconds(s: &str) -> Result<Self, BondError> {
        let plus = s.ends_with('+');
        let s = s.trim_end_matches('+');

        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 2 {
            return Err(BondError::InvalidPrice {
                reason: format!("Invalid 32nds format '{s}': expected 'handle-32nds'"),
            });
        }

        let handle: u32 = parts[0].parse().map_err(|_| BondError::InvalidPrice {
            reason: format!("Invalid handle in '{s}'"),
        })?;

        let thirty_seconds: u32 = parts[1].parse().map_err(|_| BondError::InvalidPrice {
            reason: format!("Invalid 32nds value in '{s}'"),
        })?;

        Self::from_thirty_seconds(handle, thirty_seconds, plus)
    }

    /// Parses 64ths notation (e.g., "99-32").
    fn parse_sixty_fourths(s: &str) -> Result<Self, BondError> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 2 {
            return Err(BondError::InvalidPrice {
                reason: format!("Invalid 64ths format '{s}': expected 'handle-64ths'"),
            });
        }

        let handle: u32 = parts[0].parse().map_err(|_| BondError::InvalidPrice {
            reason: format!("Invalid handle in '{s}'"),
        })?;

        let sixty_fourths: u32 = parts[1].parse().map_err(|_| BondError::InvalidPrice {
            reason: format!("Invalid 64ths value in '{s}'"),
        })?;

        Self::from_sixty_fourths(handle, sixty_fourths)
    }

    /// Returns the decimal price value.
    #[must_use]
    pub fn decimal_price(&self) -> Decimal {
        self.decimal
    }

    /// Returns the original quote convention.
    #[must_use]
    pub fn convention(&self) -> PriceQuoteConvention {
        self.convention
    }

    /// Returns the original quote string if available.
    #[must_use]
    pub fn original_quote(&self) -> Option<&str> {
        self.original.as_deref()
    }

    /// Formats the price in 32nds notation.
    ///
    /// Returns (handle, `thirty_seconds`, `has_plus`).
    #[must_use]
    pub fn to_thirty_seconds(&self) -> (u32, u32, bool) {
        let handle = self.decimal.trunc();
        let frac = self.decimal - handle;

        // Convert to 64ths first to detect the plus
        let sixty_fourths = (frac * Decimal::from(64))
            .round()
            .to_string()
            .parse::<u32>()
            .unwrap_or(0);

        let thirty_seconds = sixty_fourths / 2;
        let has_plus = sixty_fourths % 2 == 1;

        let handle_u32 = handle.to_string().parse::<u32>().unwrap_or(0);

        (handle_u32, thirty_seconds, has_plus)
    }

    /// Formats the price as a 32nds string.
    #[must_use]
    pub fn format_thirty_seconds(&self) -> String {
        let (handle, thirty_seconds, has_plus) = self.to_thirty_seconds();
        if has_plus {
            format!("{handle}-{thirty_seconds:02}+")
        } else {
            format!("{handle}-{thirty_seconds:02}")
        }
    }

    /// Converts a discount rate to price given days to maturity.
    ///
    /// Price = 100 × (1 - Rate × Days / 360)
    #[must_use]
    pub fn discount_to_price(rate: Decimal, days: u32) -> Decimal {
        let days_dec = Decimal::from(days);
        let hundred = Decimal::from(100);
        let three_sixty = Decimal::from(360);

        hundred * (Decimal::ONE - rate * days_dec / three_sixty)
    }

    /// Converts a price to discount rate given days to maturity.
    ///
    /// Rate = (100 - Price) / 100 × (360 / Days)
    #[must_use]
    pub fn price_to_discount(price: Decimal, days: u32) -> Decimal {
        if days == 0 {
            return Decimal::ZERO;
        }

        let days_dec = Decimal::from(days);
        let hundred = Decimal::from(100);
        let three_sixty = Decimal::from(360);

        (hundred - price) / hundred * (three_sixty / days_dec)
    }
}

impl From<Decimal> for PriceQuote {
    fn from(decimal: Decimal) -> Self {
        Self::new(decimal)
    }
}

impl From<f64> for PriceQuote {
    fn from(value: f64) -> Self {
        Self::new(Decimal::try_from(value).unwrap_or(Decimal::ZERO))
    }
}

impl std::fmt::Display for PriceQuote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.convention {
            PriceQuoteConvention::ThirtySeconds | PriceQuoteConvention::ThirtySecondsPlus => {
                write!(f, "{}", self.format_thirty_seconds())
            }
            _ => write!(f, "{}", self.decimal),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_quote_convention_default() {
        let conv = PriceQuoteConvention::default();
        assert_eq!(conv, PriceQuoteConvention::Decimal);
    }

    #[test]
    fn test_convention_properties() {
        assert!(PriceQuoteConvention::ThirtySeconds.is_fractional());
        assert!(PriceQuoteConvention::SixtyFourths.is_fractional());
        assert!(!PriceQuoteConvention::Decimal.is_fractional());

        assert!(PriceQuoteConvention::Discount.is_rate_quote());
        assert!(PriceQuoteConvention::Yield.is_rate_quote());
        assert!(!PriceQuoteConvention::Decimal.is_rate_quote());

        assert_eq!(PriceQuoteConvention::ThirtySeconds.denominator(), Some(32));
        assert_eq!(PriceQuoteConvention::SixtyFourths.denominator(), Some(64));
        assert_eq!(PriceQuoteConvention::Decimal.denominator(), None);
    }

    #[test]
    fn test_price_quote_from_thirty_seconds() {
        // 99-16 = 99.50
        let quote = PriceQuote::from_thirty_seconds(99, 16, false).unwrap();
        assert_eq!(quote.decimal_price(), Decimal::new(995, 1));

        // 99-00 = 99.00
        let quote = PriceQuote::from_thirty_seconds(99, 0, false).unwrap();
        assert_eq!(quote.decimal_price(), Decimal::from(99));

        // 100-00 = 100.00
        let quote = PriceQuote::from_thirty_seconds(100, 0, false).unwrap();
        assert_eq!(quote.decimal_price(), Decimal::from(100));

        // 99-08 = 99.25
        let quote = PriceQuote::from_thirty_seconds(99, 8, false).unwrap();
        assert_eq!(quote.decimal_price(), Decimal::new(9925, 2));
    }

    #[test]
    fn test_price_quote_from_thirty_seconds_plus() {
        // 99-16+ = 99.515625
        let quote = PriceQuote::from_thirty_seconds(99, 16, true).unwrap();
        assert_eq!(quote.decimal_price(), Decimal::new(99515625, 6));

        // 99-00+ = 99.015625
        let quote = PriceQuote::from_thirty_seconds(99, 0, true).unwrap();
        assert_eq!(quote.decimal_price(), Decimal::new(99015625, 6));
    }

    #[test]
    fn test_price_quote_invalid_thirty_seconds() {
        let result = PriceQuote::from_thirty_seconds(99, 32, false);
        assert!(result.is_err());

        let result = PriceQuote::from_thirty_seconds(99, 100, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_price_quote_parse_thirty_seconds() {
        let quote = PriceQuote::parse("99-16", PriceQuoteConvention::ThirtySeconds).unwrap();
        assert_eq!(quote.decimal_price(), Decimal::new(995, 1));

        let quote = PriceQuote::parse("99-16+", PriceQuoteConvention::ThirtySecondsPlus).unwrap();
        assert_eq!(quote.decimal_price(), Decimal::new(99515625, 6));

        let quote = PriceQuote::parse("100-08", PriceQuoteConvention::ThirtySeconds).unwrap();
        assert_eq!(quote.decimal_price(), Decimal::new(10025, 2));
    }

    #[test]
    fn test_price_quote_parse_decimal() {
        let quote = PriceQuote::parse("99.50", PriceQuoteConvention::Decimal).unwrap();
        assert_eq!(quote.decimal_price(), Decimal::new(995, 1));

        let quote = PriceQuote::parse("100.125", PriceQuoteConvention::Decimal).unwrap();
        assert_eq!(quote.decimal_price(), Decimal::new(100125, 3));
    }

    #[test]
    fn test_price_quote_to_thirty_seconds() {
        let quote = PriceQuote::new(Decimal::new(995, 1)); // 99.5
        let (handle, thirty_seconds, has_plus) = quote.to_thirty_seconds();
        assert_eq!(handle, 99);
        assert_eq!(thirty_seconds, 16);
        assert!(!has_plus);

        let quote = PriceQuote::new(Decimal::new(99515625, 6)); // 99.515625
        let (handle, thirty_seconds, has_plus) = quote.to_thirty_seconds();
        assert_eq!(handle, 99);
        assert_eq!(thirty_seconds, 16);
        assert!(has_plus);
    }

    #[test]
    fn test_price_quote_format() {
        let quote = PriceQuote::from_thirty_seconds(99, 16, false).unwrap();
        assert_eq!(quote.format_thirty_seconds(), "99-16");
        assert_eq!(format!("{}", quote), "99-16");

        let quote = PriceQuote::from_thirty_seconds(99, 16, true).unwrap();
        assert_eq!(quote.format_thirty_seconds(), "99-16+");
        assert_eq!(format!("{}", quote), "99-16+");
    }

    #[test]
    fn test_discount_conversion() {
        // 5% discount rate, 90 days
        let rate = Decimal::new(5, 2); // 0.05
        let price = PriceQuote::discount_to_price(rate, 90);
        // Price = 100 * (1 - 0.05 * 90 / 360) = 100 * (1 - 0.0125) = 98.75
        assert_eq!(price, Decimal::new(9875, 2));

        // Convert back
        let calculated_rate = PriceQuote::price_to_discount(price, 90);
        assert_eq!(calculated_rate, rate);
    }

    #[test]
    fn test_price_quote_from_f64() {
        let quote: PriceQuote = 99.5.into();
        assert_eq!(quote.decimal_price(), Decimal::new(995, 1));
    }

    #[test]
    fn test_sixty_fourths() {
        let quote = PriceQuote::from_sixty_fourths(99, 32).unwrap();
        assert_eq!(quote.decimal_price(), Decimal::new(995, 1));

        let quote = PriceQuote::from_sixty_fourths(99, 0).unwrap();
        assert_eq!(quote.decimal_price(), Decimal::from(99));

        // Invalid
        let result = PriceQuote::from_sixty_fourths(99, 64);
        assert!(result.is_err());
    }

    #[test]
    fn test_tick_size() {
        assert_eq!(
            PriceQuoteConvention::Decimal.tick_size(),
            Decimal::new(1, 2)
        );
        assert_eq!(
            PriceQuoteConvention::ThirtySeconds.tick_size(),
            Decimal::new(3125, 6)
        );
    }
}
