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

    pub(crate) fn exact_securities_value(
        &self,
        accrued: bool,
    ) -> crate::PortfolioResult<convex_math::decimal::BoundedDecimal> {
        self.validate()?;
        crate::analytics::metrics::sum(self.holdings.iter().map(|h| {
            let price = if accrued {
                convex_math::decimal::BoundedDecimal::decimal(h.accrued_interest)
            } else {
                h.exact_clean_price()?
            };
            h.exact_amount(price, true)
        }))
    }
    pub(crate) fn exact_cash(
        &self,
    ) -> crate::PortfolioResult<convex_math::decimal::BoundedDecimal> {
        self.validate()?;
        crate::analytics::metrics::sum(self.cash.iter().map(CashPosition::exact_value))
    }
    pub(crate) fn exact_nav(&self) -> crate::PortfolioResult<convex_math::decimal::BoundedDecimal> {
        self.validate()?;
        let mut nav = crate::analytics::metrics::sum(
            self.holdings
                .iter()
                .map(|h| h.exact_amount(h.exact_dirty_price()?, true)),
        )?;
        nav.add(&self.exact_cash()?)?;
        nav.add(&convex_math::decimal::BoundedDecimal::decimal(
            -self.total_liabilities(),
        ))?;
        Ok(nav)
    }
    /// Checked total clean reporting value, retaining signed cancellation.
    pub fn securities_market_value(&self) -> crate::PortfolioResult<Decimal> {
        self.exact_securities_value(false)?
            .finish()
            .map_err(Into::into)
    }
    /// Checked total reporting accrued; signed ex-coupon accrual stays signed.
    pub fn total_accrued_interest(&self) -> crate::PortfolioResult<Decimal> {
        self.exact_securities_value(true)?
            .finish()
            .map_err(Into::into)
    }
    /// Checked reporting par, with exact products and signed accumulation.
    pub fn total_par_value(&self) -> crate::PortfolioResult<Decimal> {
        self.validate()?;
        crate::analytics::metrics::sum(
            self.holdings
                .iter()
                .map(|h| h.exact_weight(WeightingMethod::ParValue)),
        )?
        .finish()
        .map_err(Into::into)
    }
    /// Checked total supplied cash converted to reporting currency.
    pub fn total_cash(&self) -> crate::PortfolioResult<Decimal> {
        self.exact_cash()?.finish().map_err(Into::into)
    }
    /// Supplied reporting liabilities; absence explicitly means none in this contract.
    #[must_use]
    pub fn total_liabilities(&self) -> Decimal {
        self.liabilities.unwrap_or(Decimal::ZERO)
    }
    /// Checked exact dirty securities plus supplied cash less reporting liabilities.
    pub fn nav(&self) -> crate::PortfolioResult<Decimal> {
        self.exact_nav()?.finish().map_err(Into::into)
    }
    /// Final exact NAV/share ratio; absent or zero shares have no ratio.
    pub fn nav_per_share(&self) -> crate::PortfolioResult<Option<Decimal>> {
        crate::analytics::metrics::per_share_exact(Some(self.exact_nav()?), self.shares_outstanding)
    }
    /// Complete supplied currency DV01, unavailable if any holding lacks it.
    pub fn total_dv01(&self) -> crate::PortfolioResult<Option<Decimal>> {
        self.validate()?;
        crate::analytics::metrics::currency_sum(&self.holdings, "dv01", |h| h.analytics.dv01)
    }
    /// Final exact currency DV01/share ratio, with explicit missing inputs.
    pub fn dv01_per_share(&self) -> crate::PortfolioResult<Option<Decimal>> {
        self.validate()?;
        crate::analytics::metrics::per_share_exact(
            crate::analytics::metrics::currency_exact(&self.holdings, "dv01", |h| {
                h.analytics.dv01
            })?,
            self.shares_outstanding,
        )
    }
    /// Checked signed presentation weights; zero normalization is unavailable.
    pub fn calculate_weights(
        &self,
        method: WeightingMethod,
    ) -> crate::PortfolioResult<Option<Vec<(&str, Decimal)>>> {
        use convex_math::decimal::BoundedDecimal;
        self.validate()?;
        let weights = self
            .holdings
            .iter()
            .map(|h| h.exact_weight(method))
            .collect::<crate::PortfolioResult<Vec<_>>>()?;
        let mut total = BoundedDecimal::zero();
        for weight in &weights {
            total.add(weight)?;
        }
        if total.is_zero() {
            return Ok(None);
        }
        self.holdings
            .iter()
            .zip(weights)
            .map(|(h, weight)| {
                crate::analytics::metrics::signed_ratio(&weight, &total)?
                    .map(|v| (h.id.as_str(), v))
                    .ok_or_else(|| {
                        crate::PortfolioError::invalid_portfolio(
                            "undefined nonzero weight denominator",
                        )
                    })
            })
            .collect::<crate::PortfolioResult<Vec<_>>>()
            .map(Some)
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
        crate::analytics::metrics::identities(&self.holdings)?;
        if self.cash.len() > 65_536
            || self.shares_outstanding.is_some_and(|s| s < Decimal::ZERO)
            || self.liabilities.is_some_and(|l| l < Decimal::ZERO)
        {
            return Err(crate::PortfolioError::invalid_portfolio(
                "bounded cash manifest and nonnegative supplied shares/liabilities required",
            ));
        }

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
        assert_eq!(
            portfolio.securities_market_value().unwrap(),
            dec!(1_490_000)
        );
    }

    #[test]
    fn test_accrued_interest() {
        let portfolio = create_test_portfolio();

        // Bond1: 1,000,000 × 1.0/100 = 10,000
        // Bond2: 500,000 × 0.5/100 = 2,500
        // Total: 12,500
        assert_eq!(portfolio.total_accrued_interest().unwrap(), dec!(12_500));
    }

    #[test]
    fn test_total_cash() {
        let portfolio = create_test_portfolio();
        assert_eq!(portfolio.total_cash().unwrap(), dec!(100_000));
    }

    #[test]
    fn test_nav() {
        let portfolio = create_test_portfolio();

        // NAV = 1,490,000 + 12,500 + 100,000 - 0 = 1,602,500
        assert_eq!(portfolio.nav().unwrap(), dec!(1_602_500));
    }

    #[test]
    fn test_nav_per_share() {
        let portfolio = create_test_portfolio();

        // NAV per share = 1,602,500 / 1,000,000 = 1.6025
        assert_eq!(portfolio.nav_per_share().unwrap(), Some(dec!(1.6025)));
    }

    #[test]
    fn test_total_dv01() {
        let portfolio = create_test_portfolio();

        // Bond1: 0.05 × 1,000,000 / 100 = 500
        // Bond2: 0.04 × 500,000 / 100 = 200
        // Total: 700
        let dv01 = portfolio.total_dv01().unwrap().unwrap();
        assert!((dv01 - dec!(700)).abs() < dec!(1));
    }

    #[test]
    fn test_calculate_weights() {
        let portfolio = create_test_portfolio();

        let weights = portfolio
            .calculate_weights(WeightingMethod::MarketValue)
            .unwrap()
            .unwrap();
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

        let weights = portfolio
            .calculate_weights(WeightingMethod::ParValue)
            .unwrap()
            .unwrap();

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
