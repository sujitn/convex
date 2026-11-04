//! Cash position representation.

use convex_core::types::Currency;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// A cash position with currency and FX rate.
///
/// Represents cash held in a portfolio, potentially in a different currency
/// than the portfolio's base currency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashPosition {
    /// Amount in the cash currency.
    pub amount: Decimal,

    /// Currency of the cash.
    pub currency: Currency,

    /// FX rate to convert to portfolio base currency.
    /// Rate is expressed as: 1 unit of cash currency = fx_rate units of base currency.
    pub fx_rate: Decimal,
}

impl CashPosition {
    /// Creates a new cash position.
    ///
    /// # Arguments
    ///
    /// * `amount` - Amount of cash
    /// * `currency` - Currency of the cash
    #[must_use]
    pub fn new(amount: Decimal, currency: Currency) -> Self {
        Self {
            amount,
            currency,
            fx_rate: Decimal::ONE,
        }
    }

    /// Creates a cash position with an FX rate.
    ///
    /// # Arguments
    ///
    /// * `amount` - Amount of cash
    /// * `currency` - Currency of the cash
    /// * `fx_rate` - Rate to convert to base currency
    #[must_use]
    pub fn with_fx_rate(amount: Decimal, currency: Currency, fx_rate: Decimal) -> Self {
        Self {
            amount,
            currency,
            fx_rate,
        }
    }

    /// Sets the FX rate.
    #[must_use]
    pub fn fx_rate(mut self, rate: Decimal) -> Self {
        self.fx_rate = rate;
        self
    }

    /// Checked value at the supplied positive reporting FX; rounds only final output.
    pub fn value_in_base(&self) -> crate::PortfolioResult<Decimal> {
        self.exact_value()?.finish().map_err(Into::into)
    }
    pub(crate) fn exact_value(
        &self,
    ) -> crate::PortfolioResult<convex_math::decimal::BoundedDecimal> {
        if self.fx_rate <= Decimal::ZERO {
            return Err(crate::PortfolioError::InvalidFxRate {
                currency: self.currency.to_string(),
                rate: self.fx_rate.to_string(),
            });
        }
        convex_math::decimal::BoundedDecimal::decimal(self.amount)
            .multiply(self.fx_rate)
            .map_err(Into::into)
    }
    /// Compare actual currency identity with the explicit reporting currency.
    #[must_use]
    pub fn is_base_currency(&self, reporting_currency: Currency) -> bool {
        self.currency == reporting_currency
    }
}

impl Default for CashPosition {
    fn default() -> Self {
        Self {
            amount: Decimal::ZERO,
            currency: Currency::USD,
            fx_rate: Decimal::ONE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_new() {
        let cash = CashPosition::new(dec!(1_000_000), Currency::USD);
        assert_eq!(cash.amount, dec!(1_000_000));
        assert_eq!(cash.currency, Currency::USD);
        assert_eq!(cash.fx_rate, Decimal::ONE);
    }

    #[test]
    fn test_with_fx_rate() {
        let cash = CashPosition::with_fx_rate(dec!(1_000_000), Currency::EUR, dec!(1.10));

        assert_eq!(cash.amount, dec!(1_000_000));
        assert_eq!(cash.currency, Currency::EUR);
        assert_eq!(cash.fx_rate, dec!(1.10));
    }

    #[test]
    fn test_value_in_base() {
        // USD cash in USD portfolio (no conversion)
        let usd_cash = CashPosition::new(dec!(1_000_000), Currency::USD);
        assert_eq!(usd_cash.value_in_base().unwrap(), dec!(1_000_000));

        // EUR cash in USD portfolio (1 EUR = 1.10 USD)
        let eur_cash = CashPosition::with_fx_rate(dec!(1_000_000), Currency::EUR, dec!(1.10));
        assert_eq!(eur_cash.value_in_base().unwrap(), dec!(1_100_000));

        // GBP cash in USD portfolio (1 GBP = 1.25 USD)
        let gbp_cash = CashPosition::with_fx_rate(dec!(500_000), Currency::GBP, dec!(1.25));
        assert_eq!(gbp_cash.value_in_base().unwrap(), dec!(625_000));
    }

    #[test]
    fn test_is_base_currency() {
        let usd_cash = CashPosition::new(dec!(1_000_000), Currency::USD);
        assert!(usd_cash.is_base_currency(Currency::USD));

        let eur_cash = CashPosition::with_fx_rate(dec!(1_000_000), Currency::EUR, dec!(1.10));
        assert!(!eur_cash.is_base_currency(Currency::USD));
    }

    #[test]
    fn test_serde() {
        let cash = CashPosition::with_fx_rate(dec!(1_000_000), Currency::EUR, dec!(1.10));

        let json = serde_json::to_string(&cash).unwrap();
        let parsed: CashPosition = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.amount, cash.amount);
        assert_eq!(parsed.currency, cash.currency);
        assert_eq!(parsed.fx_rate, cash.fx_rate);
    }
}
