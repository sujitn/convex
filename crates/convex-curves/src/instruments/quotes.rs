//! Market quote types for curve instruments.
//!
//! This module provides types for representing market observable quotes
//! and converting between different quote conventions.
//!
//! # Key Principle
//!
//! All curve inputs should be actual market observables - the values you can
//! see on a Bloomberg terminal or trading screen. This module ensures proper
//! handling of:
//!
//! - Different quoting conventions (price vs yield vs rate)
//! - Quote validation (reasonable ranges, consistency)
//! - Conversion between quote types
//!
//! # Quote Types by Instrument
//!
//! | Instrument | Primary Quote | Alternative |
//! |------------|---------------|-------------|
//! | Deposit    | Rate (ACT/360)| - |
//! | FRA        | Rate (ACT/360)| - |
//! | Futures    | Price (100-rate)| Implied Rate |
//! | Swap       | Par Rate      | - |
//! | OIS        | Par Rate      | - |
//! | T-Bill     | Price         | Discount Rate, BEY |
//! | T-Note/Bond| Clean Price   | YTM |

use crate::error::{CurveError, CurveResult};

/// How a bond is quoted in the market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BondQuoteType {
    /// Clean price per 100 face (excludes accrued interest)
    CleanPrice,
    /// Yield to maturity (bond equivalent yield for Treasuries)
    YieldToMaturity,
    /// Discount rate (T-bills only, bank discount basis)
    DiscountRate,
    /// Full/dirty price per 100 face (includes accrued interest)
    DirtyPrice,
}

impl BondQuoteType {
    /// Returns true if this quote type represents a price.
    #[must_use]
    pub fn is_price(&self) -> bool {
        matches!(self, Self::CleanPrice | Self::DirtyPrice)
    }

    /// Returns true if this quote type represents a rate/yield.
    #[must_use]
    pub fn is_rate(&self) -> bool {
        matches!(self, Self::YieldToMaturity | Self::DiscountRate)
    }
}

impl std::fmt::Display for BondQuoteType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CleanPrice => write!(f, "Clean Price"),
            Self::YieldToMaturity => write!(f, "YTM"),
            Self::DiscountRate => write!(f, "Discount Rate"),
            Self::DirtyPrice => write!(f, "Dirty Price"),
        }
    }
}

/// How a rate instrument is quoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RateQuoteType {
    /// Simple rate (money market convention)
    #[default]
    Simple,
    /// Continuously compounded rate
    Continuous,
    /// Annually compounded rate
    Annual,
    /// Semi-annually compounded rate
    SemiAnnual,
}

impl std::fmt::Display for RateQuoteType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Simple => write!(f, "Simple"),
            Self::Continuous => write!(f, "Continuous"),
            Self::Annual => write!(f, "Annual"),
            Self::SemiAnnual => write!(f, "Semi-Annual"),
        }
    }
}

/// A market quote with its type and metadata.
#[derive(Debug, Clone)]
pub struct MarketQuote {
    /// The quoted value
    pub value: f64,
    /// Type of quote
    pub quote_type: QuoteType,
    /// Optional bid value
    pub bid: Option<f64>,
    /// Optional ask value
    pub ask: Option<f64>,
    /// Optional source identifier
    pub source: Option<String>,
}

/// Union of all quote types.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QuoteType {
    /// Bond quote (price or yield)
    Bond(BondQuoteType),
    /// Rate quote (various compounding)
    Rate(RateQuoteType),
    /// Futures price (100 - rate)
    FuturesPrice,
}

impl MarketQuote {
    /// Creates a new market quote.
    #[must_use]
    pub fn new(value: f64, quote_type: QuoteType) -> Self {
        Self {
            value,
            quote_type,
            bid: None,
            ask: None,
            source: None,
        }
    }

    /// Creates a clean price quote.
    #[must_use]
    pub fn clean_price(price: f64) -> Self {
        Self::new(price, QuoteType::Bond(BondQuoteType::CleanPrice))
    }

    /// Creates a yield to maturity quote.
    #[must_use]
    pub fn ytm(yield_value: f64) -> Self {
        Self::new(yield_value, QuoteType::Bond(BondQuoteType::YieldToMaturity))
    }

    /// Creates a simple rate quote.
    #[must_use]
    pub fn rate(rate: f64) -> Self {
        Self::new(rate, QuoteType::Rate(RateQuoteType::Simple))
    }

    /// Creates a futures price quote.
    #[must_use]
    pub fn futures_price(price: f64) -> Self {
        Self::new(price, QuoteType::FuturesPrice)
    }

    /// Sets bid/ask spread.
    #[must_use]
    pub fn with_bid_ask(mut self, bid: f64, ask: f64) -> Self {
        self.bid = Some(bid);
        self.ask = Some(ask);
        self
    }

    /// Sets the source identifier.
    #[must_use]
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Returns the mid-market value.
    ///
    /// If bid/ask are available, returns (bid + ask) / 2.
    /// Otherwise returns the quoted value.
    #[must_use]
    pub fn mid(&self) -> f64 {
        match (self.bid, self.ask) {
            (Some(b), Some(a)) => (b + a) / 2.0,
            _ => self.value,
        }
    }

    /// Returns the bid-ask spread if available.
    #[must_use]
    pub fn spread(&self) -> Option<f64> {
        match (self.bid, self.ask) {
            (Some(b), Some(a)) => Some(a - b),
            _ => None,
        }
    }
}

/// Validation configuration for market data.
#[derive(Debug, Clone)]
pub struct QuoteValidationConfig {
    /// Minimum allowed rate (default: -0.10 = -10%)
    pub min_rate: f64,
    /// Maximum allowed rate (default: 0.50 = 50%)
    pub max_rate: f64,
    /// Minimum allowed price (default: 0.0)
    pub min_price: f64,
    /// Maximum allowed price (default: 200.0)
    pub max_price: f64,
    /// Maximum bid-ask spread for rates (default: 0.0050 = 50bp)
    pub max_rate_spread: f64,
    /// Maximum bid-ask spread for prices (default: 2.0)
    pub max_price_spread: f64,
}

impl Default for QuoteValidationConfig {
    fn default() -> Self {
        Self {
            min_rate: -0.10,
            max_rate: 0.50,
            min_price: 0.0,
            max_price: 200.0,
            max_rate_spread: 0.0050,
            max_price_spread: 2.0,
        }
    }
}

impl QuoteValidationConfig {
    /// Creates a strict configuration for production use.
    #[must_use]
    pub fn strict() -> Self {
        Self {
            min_rate: -0.05,
            max_rate: 0.20,
            min_price: 50.0,
            max_price: 150.0,
            max_rate_spread: 0.0010,
            max_price_spread: 0.5,
        }
    }

    /// Creates a relaxed configuration for testing.
    #[must_use]
    pub fn relaxed() -> Self {
        Self {
            min_rate: -1.0,
            max_rate: 1.0,
            min_price: 0.0,
            max_price: 500.0,
            max_rate_spread: 0.10,
            max_price_spread: 10.0,
        }
    }
}

/// Validates a market quote.
pub fn validate_quote(quote: &MarketQuote, config: &QuoteValidationConfig) -> CurveResult<()> {
    match quote.quote_type {
        QuoteType::Rate(_) => {
            if quote.value < config.min_rate {
                return Err(CurveError::invalid_data(format!(
                    "Rate {:.4}% is below minimum {:.4}%",
                    quote.value * 100.0,
                    config.min_rate * 100.0
                )));
            }
            if quote.value > config.max_rate {
                return Err(CurveError::invalid_data(format!(
                    "Rate {:.4}% exceeds maximum {:.4}%",
                    quote.value * 100.0,
                    config.max_rate * 100.0
                )));
            }
            if let Some(spread) = quote.spread() {
                if spread > config.max_rate_spread {
                    return Err(CurveError::invalid_data(format!(
                        "Rate bid-ask spread {:.2}bp exceeds maximum {:.2}bp",
                        spread * 10000.0,
                        config.max_rate_spread * 10000.0
                    )));
                }
            }
        }
        QuoteType::Bond(BondQuoteType::YieldToMaturity) => {
            // Validate yield like a rate
            if quote.value < config.min_rate {
                return Err(CurveError::invalid_data(format!(
                    "Yield {:.4}% is below minimum {:.4}%",
                    quote.value * 100.0,
                    config.min_rate * 100.0
                )));
            }
            if quote.value > config.max_rate {
                return Err(CurveError::invalid_data(format!(
                    "Yield {:.4}% exceeds maximum {:.4}%",
                    quote.value * 100.0,
                    config.max_rate * 100.0
                )));
            }
        }
        QuoteType::Bond(_) | QuoteType::FuturesPrice => {
            if quote.value < config.min_price {
                return Err(CurveError::invalid_data(format!(
                    "Price {:.3} is below minimum {:.3}",
                    quote.value, config.min_price
                )));
            }
            if quote.value > config.max_price {
                return Err(CurveError::invalid_data(format!(
                    "Price {:.3} exceeds maximum {:.3}",
                    quote.value, config.max_price
                )));
            }
            if let Some(spread) = quote.spread() {
                if spread > config.max_price_spread {
                    return Err(CurveError::invalid_data(format!(
                        "Price bid-ask spread {:.3} exceeds maximum {:.3}",
                        spread, config.max_price_spread
                    )));
                }
            }
        }
    }

    Ok(())
}

/// Validates a set of market quotes for curve building.
pub fn validate_market_data(
    quotes: &[MarketQuote],
    config: &QuoteValidationConfig,
) -> CurveResult<()> {
    if quotes.is_empty() {
        return Err(CurveError::invalid_data("No market quotes provided"));
    }

    for (i, quote) in quotes.iter().enumerate() {
        validate_quote(quote, config).map_err(|e| {
            CurveError::invalid_data(format!("Quote {} validation failed: {}", i + 1, e))
        })?;
    }

    Ok(())
}

/// Converts a futures price to an implied rate.
///
/// For SOFR and Eurodollar futures: Rate = (100 - Price) / 100
#[must_use]
pub fn futures_price_to_rate(price: f64) -> f64 {
    (100.0 - price) / 100.0
}

/// Converts a rate to a futures price.
///
/// Price = 100 - Rate * 100
#[must_use]
pub fn rate_to_futures_price(rate: f64) -> f64 {
    100.0 - rate * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bond_quote_type_is_price() {
        assert!(BondQuoteType::CleanPrice.is_price());
        assert!(BondQuoteType::DirtyPrice.is_price());
        assert!(!BondQuoteType::YieldToMaturity.is_price());
        assert!(!BondQuoteType::DiscountRate.is_price());
    }

    #[test]
    fn test_bond_quote_type_is_rate() {
        assert!(!BondQuoteType::CleanPrice.is_rate());
        assert!(!BondQuoteType::DirtyPrice.is_rate());
        assert!(BondQuoteType::YieldToMaturity.is_rate());
        assert!(BondQuoteType::DiscountRate.is_rate());
    }

    #[test]
    fn test_market_quote_mid() {
        let quote = MarketQuote::rate(0.05).with_bid_ask(0.049, 0.051);
        assert!((quote.mid() - 0.05).abs() < 1e-10);
    }

    #[test]
    fn test_market_quote_spread() {
        let quote = MarketQuote::rate(0.05).with_bid_ask(0.049, 0.051);
        assert!((quote.spread().unwrap() - 0.002).abs() < 1e-10);
    }

    #[test]
    fn test_validate_quote_rate_valid() {
        let quote = MarketQuote::rate(0.05);
        let config = QuoteValidationConfig::default();
        assert!(validate_quote(&quote, &config).is_ok());
    }

    #[test]
    fn test_validate_quote_rate_too_high() {
        let quote = MarketQuote::rate(0.60); // 60%
        let config = QuoteValidationConfig::default();
        assert!(validate_quote(&quote, &config).is_err());
    }

    #[test]
    fn test_validate_quote_rate_too_low() {
        let quote = MarketQuote::rate(-0.20); // -20%
        let config = QuoteValidationConfig::default();
        assert!(validate_quote(&quote, &config).is_err());
    }

    #[test]
    fn test_validate_quote_price_valid() {
        let quote = MarketQuote::clean_price(98.50);
        let config = QuoteValidationConfig::default();
        assert!(validate_quote(&quote, &config).is_ok());
    }

    #[test]
    fn test_validate_quote_price_too_high() {
        let quote = MarketQuote::clean_price(250.0);
        let config = QuoteValidationConfig::default();
        assert!(validate_quote(&quote, &config).is_err());
    }

    #[test]
    fn test_validate_quote_spread_too_wide() {
        let quote = MarketQuote::rate(0.05).with_bid_ask(0.04, 0.06); // 200bp spread
        let config = QuoteValidationConfig::default();
        assert!(validate_quote(&quote, &config).is_err());
    }

    #[test]
    fn test_validate_market_data_empty() {
        let config = QuoteValidationConfig::default();
        assert!(validate_market_data(&[], &config).is_err());
    }

    #[test]
    fn test_validate_market_data_valid() {
        let quotes = vec![
            MarketQuote::rate(0.05),
            MarketQuote::rate(0.045),
            MarketQuote::clean_price(99.50),
        ];
        let config = QuoteValidationConfig::default();
        assert!(validate_market_data(&quotes, &config).is_ok());
    }

    #[test]
    fn test_futures_price_to_rate() {
        // 95.25 price = 4.75% rate
        assert!((futures_price_to_rate(95.25) - 0.0475).abs() < 1e-10);
    }

    #[test]
    fn test_rate_to_futures_price() {
        // 4.75% rate = 95.25 price
        assert!((rate_to_futures_price(0.0475) - 95.25).abs() < 1e-10);
    }

    #[test]
    fn test_strict_config() {
        let config = QuoteValidationConfig::strict();

        // Valid rate in strict config
        let valid_rate = MarketQuote::rate(0.05);
        assert!(validate_quote(&valid_rate, &config).is_ok());

        // Invalid rate (too high for strict)
        let high_rate = MarketQuote::rate(0.25); // 25%
        assert!(validate_quote(&high_rate, &config).is_err());
    }

    #[test]
    fn test_relaxed_config() {
        let config = QuoteValidationConfig::relaxed();

        // Even extreme rates pass in relaxed mode
        let extreme_rate = MarketQuote::rate(0.50); // 50%
        assert!(validate_quote(&extreme_rate, &config).is_ok());
    }
}
