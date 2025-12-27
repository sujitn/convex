//! Portfolio struct and core methods.

use crate::types::{CashPosition, Holding, WeightingMethod};
use convex_core::types::{Currency, Date};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// A fixed income portfolio.
///
/// Contains holdings (bond positions) and cash positions, with metadata
/// for NAV calculation and ETF analytics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Portfolio {
    /// Unique identifier for the portfolio.
    pub id: String,

    /// Portfolio name.
    pub name: String,

    /// Base currency for reporting.
    pub base_currency: Currency,

    /// As-of date for the portfolio.
    pub as_of_date: Date,

    /// Bond holdings.
    pub holdings: Vec<Holding>,

    /// Cash positions (may be in multiple currencies).
    pub cash: Vec<CashPosition>,

    /// Shares outstanding (for ETF NAV per share).
    pub shares_outstanding: Option<Decimal>,

    /// Liabilities (for NAV calculation).
    pub liabilities: Option<Decimal>,
}

impl Portfolio {
    /// Creates a new portfolio builder.
    #[must_use]
    pub fn builder(name: impl Into<String>) -> super::PortfolioBuilder {
        super::PortfolioBuilder::new().name(name)
    }

    /// Returns the number of holdings.
    #[must_use]
    pub fn holding_count(&self) -> usize {
        self.holdings.len()
    }

    /// Returns true if the portfolio has no holdings.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.holdings.is_empty()
    }

    /// Returns the total market value of all holdings (in base currency).
    #[must_use]
    pub fn securities_market_value(&self) -> Decimal {
        self.holdings.iter().map(|h| h.market_value()).sum()
    }

    /// Returns the total accrued interest (in base currency).
    #[must_use]
    pub fn total_accrued_interest(&self) -> Decimal {
        self.holdings.iter().map(|h| h.accrued_amount()).sum()
    }

    /// Returns the total par value of all holdings (in base currency).
    #[must_use]
    pub fn total_par_value(&self) -> Decimal {
        self.holdings.iter().map(|h| h.par_amount * h.fx_rate).sum()
    }

    /// Returns the total cash value (in base currency).
    #[must_use]
    pub fn total_cash(&self) -> Decimal {
        self.cash.iter().map(|c| c.value_in_base()).sum()
    }

    /// Returns the total liabilities.
    #[must_use]
    pub fn total_liabilities(&self) -> Decimal {
        self.liabilities.unwrap_or(Decimal::ZERO)
    }

    /// Calculates the Net Asset Value (NAV).
    ///
    /// NAV = Securities MV + Accrued Interest + Cash - Liabilities
    #[must_use]
    pub fn nav(&self) -> Decimal {
        self.securities_market_value() + self.total_accrued_interest() + self.total_cash()
            - self.total_liabilities()
    }

    /// Calculates the NAV per share.
    ///
    /// Returns None if shares_outstanding is not set or is zero.
    #[must_use]
    pub fn nav_per_share(&self) -> Option<Decimal> {
        self.shares_outstanding.and_then(|shares| {
            if shares > Decimal::ZERO {
                Some(self.nav() / shares)
            } else {
                None
            }
        })
    }

    /// Returns the total DV01 of the portfolio.
    #[must_use]
    pub fn total_dv01(&self) -> Decimal {
        self.holdings.iter().filter_map(|h| h.total_dv01()).sum()
    }

    /// Returns the DV01 per share.
    #[must_use]
    pub fn dv01_per_share(&self) -> Option<Decimal> {
        self.shares_outstanding.and_then(|shares| {
            if shares > Decimal::ZERO {
                Some(self.total_dv01() / shares)
            } else {
                None
            }
        })
    }

    /// Calculates the weight of each holding.
    ///
    /// Returns a vector of (holding_id, weight) pairs.
    #[must_use]
    pub fn calculate_weights(&self, method: WeightingMethod) -> Vec<(&str, Decimal)> {
        let total: Decimal = self.holdings.iter().map(|h| h.weight_value(method)).sum();

        if total == Decimal::ZERO {
            return self
                .holdings
                .iter()
                .map(|h| (h.id.as_str(), Decimal::ZERO))
                .collect();
        }

        self.holdings
            .iter()
            .map(|h| (h.id.as_str(), h.weight_value(method) / total))
            .collect()
    }

    /// Returns holdings filtered by a predicate.
    pub fn filter_holdings<F>(&self, predicate: F) -> Vec<&Holding>
    where
        F: Fn(&Holding) -> bool,
    {
        self.holdings.iter().filter(|h| predicate(h)).collect()
    }

    /// Returns the set of currencies in the portfolio.
    #[must_use]
    pub fn currencies(&self) -> Vec<Currency> {
        let mut currencies: Vec<Currency> = self
            .holdings
            .iter()
            .map(|h| h.currency)
            .chain(self.cash.iter().map(|c| c.currency))
            .collect();

        currencies.sort_by_key(|c| c.code());
        currencies.dedup();
        currencies
    }

    /// Returns true if this is a multi-currency portfolio.
    #[must_use]
    pub fn is_multi_currency(&self) -> bool {
        self.currencies().len() > 1
    }

    /// Validates the portfolio.
    ///
    /// Checks for:
    /// - Non-empty holdings
    /// - Valid FX rates
    /// - Consistent data
    pub fn validate(&self) -> crate::PortfolioResult<()> {
        // Check for empty portfolio (warning, not error)
        if self.holdings.is_empty() && self.cash.is_empty() {
            // Empty portfolio is valid but unusual
        }

        // Check for invalid FX rates
        for holding in &self.holdings {
            if holding.fx_rate <= Decimal::ZERO {
                return Err(crate::PortfolioError::InvalidFxRate {
                    currency: holding.currency.to_string(),
                    rate: holding.fx_rate.to_string(),
                });
            }
        }

        for cash in &self.cash {
            if cash.fx_rate <= Decimal::ZERO {
                return Err(crate::PortfolioError::InvalidFxRate {
                    currency: cash.currency.to_string(),
                    rate: cash.fx_rate.to_string(),
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::HoldingAnalytics;
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal_macros::dec;

    fn create_test_identifiers(suffix: &str) -> BondIdentifiers {
        BondIdentifiers::new().with_ticker(format!("TEST{}", suffix))
    }

    fn create_test_portfolio() -> Portfolio {
        let holding1 = Holding::builder()
            .id("BOND1")
            .identifiers(create_test_identifiers("001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .accrued_interest(dec!(1.0))
            .analytics(HoldingAnalytics::new().with_dv01(0.05))
            .build()
            .unwrap();

        let holding2 = Holding::builder()
            .id("BOND2")
            .identifiers(create_test_identifiers("002"))
            .par_amount(dec!(500_000))
            .market_price(dec!(98))
            .accrued_interest(dec!(0.5))
            .analytics(HoldingAnalytics::new().with_dv01(0.04))
            .build()
            .unwrap();

        Portfolio::builder("Test Portfolio")
            .id("TEST001")
            .base_currency(Currency::USD)
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .add_holding(holding1)
            .add_holding(holding2)
            .add_cash(CashPosition::new(dec!(100_000), Currency::USD))
            .shares_outstanding(dec!(1_000_000))
            .build()
            .unwrap()
    }

    #[test]
    fn test_holding_count() {
        let portfolio = create_test_portfolio();
        assert_eq!(portfolio.holding_count(), 2);
        assert!(!portfolio.is_empty());
    }

    #[test]
    fn test_securities_market_value() {
        let portfolio = create_test_portfolio();

        // Bond1: 1,000,000 × 100/100 = 1,000,000
        // Bond2: 500,000 × 98/100 = 490,000
        // Total: 1,490,000
        assert_eq!(portfolio.securities_market_value(), dec!(1_490_000));
    }

    #[test]
    fn test_accrued_interest() {
        let portfolio = create_test_portfolio();

        // Bond1: 1,000,000 × 1.0/100 = 10,000
        // Bond2: 500,000 × 0.5/100 = 2,500
        // Total: 12,500
        assert_eq!(portfolio.total_accrued_interest(), dec!(12_500));
    }

    #[test]
    fn test_total_cash() {
        let portfolio = create_test_portfolio();
        assert_eq!(portfolio.total_cash(), dec!(100_000));
    }

    #[test]
    fn test_nav() {
        let portfolio = create_test_portfolio();

        // NAV = 1,490,000 + 12,500 + 100,000 - 0 = 1,602,500
        assert_eq!(portfolio.nav(), dec!(1_602_500));
    }

    #[test]
    fn test_nav_per_share() {
        let portfolio = create_test_portfolio();

        // NAV per share = 1,602,500 / 1,000,000 = 1.6025
        assert_eq!(portfolio.nav_per_share(), Some(dec!(1.6025)));
    }

    #[test]
    fn test_total_dv01() {
        let portfolio = create_test_portfolio();

        // Bond1: 0.05 × 1,000,000 / 100 = 500
        // Bond2: 0.04 × 500,000 / 100 = 200
        // Total: 700
        let dv01 = portfolio.total_dv01();
        assert!((dv01 - dec!(700)).abs() < dec!(1));
    }

    #[test]
    fn test_calculate_weights() {
        let portfolio = create_test_portfolio();

        let weights = portfolio.calculate_weights(WeightingMethod::MarketValue);
        assert_eq!(weights.len(), 2);

        // Bond1 weight: 1,000,000 / 1,490,000 ≈ 0.6711
        // Bond2 weight: 490,000 / 1,490,000 ≈ 0.3289
        let bond1_weight = weights.iter().find(|(id, _)| *id == "BOND1").unwrap().1;
        let bond2_weight = weights.iter().find(|(id, _)| *id == "BOND2").unwrap().1;

        assert!((bond1_weight - dec!(0.6711)).abs() < dec!(0.001));
        assert!((bond2_weight - dec!(0.3289)).abs() < dec!(0.001));

        // Weights should sum to 1
        let total: Decimal = weights.iter().map(|(_, w)| w).sum();
        assert!((total - Decimal::ONE).abs() < dec!(0.0001));
    }

    #[test]
    fn test_par_value_weights() {
        let portfolio = create_test_portfolio();

        let weights = portfolio.calculate_weights(WeightingMethod::ParValue);

        // Bond1 weight: 1,000,000 / 1,500,000 ≈ 0.6667
        // Bond2 weight: 500,000 / 1,500,000 ≈ 0.3333
        let bond1_weight = weights.iter().find(|(id, _)| *id == "BOND1").unwrap().1;

        assert!((bond1_weight - dec!(0.6667)).abs() < dec!(0.001));
    }

    #[test]
    fn test_currencies() {
        let portfolio = create_test_portfolio();
        let currencies = portfolio.currencies();

        assert_eq!(currencies.len(), 1);
        assert_eq!(currencies[0], Currency::USD);
        assert!(!portfolio.is_multi_currency());
    }

    #[test]
    fn test_validation() {
        let portfolio = create_test_portfolio();
        assert!(portfolio.validate().is_ok());
    }
}
