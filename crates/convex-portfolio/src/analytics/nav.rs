//! NAV (Net Asset Value) analytics.
//!
//! Provides detailed NAV breakdown and component analysis for portfolios.

use crate::Portfolio;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Detailed NAV breakdown showing all components.
///
/// NAV = Securities Market Value + Accrued Interest + Cash - Liabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavBreakdown {
    /// Total market value of securities (clean price × par × FX rate).
    pub securities_market_value: Decimal,

    /// Total accrued interest on all holdings.
    pub accrued_interest: Decimal,

    /// Total cash across all currencies (converted to base).
    pub total_cash: Decimal,

    /// Total liabilities.
    pub liabilities: Decimal,

    /// Net Asset Value = MV + Accrued + Cash - Liabilities.
    pub nav: Decimal,

    /// Shares outstanding (if applicable).
    pub shares_outstanding: Option<Decimal>,

    /// NAV per share (if shares_outstanding is set).
    pub nav_per_share: Option<Decimal>,
}

impl NavBreakdown {
    /// Creates a NAV breakdown from a portfolio.
    #[must_use]
    pub fn from_portfolio(portfolio: &Portfolio) -> Self {
        let securities_market_value = portfolio.securities_market_value();
        let accrued_interest = portfolio.total_accrued_interest();
        let total_cash = portfolio.total_cash();
        let liabilities = portfolio.total_liabilities();

        let nav = securities_market_value + accrued_interest + total_cash - liabilities;

        let nav_per_share = portfolio.shares_outstanding.and_then(|shares| {
            if shares > Decimal::ZERO {
                Some(nav / shares)
            } else {
                None
            }
        });

        Self {
            securities_market_value,
            accrued_interest,
            total_cash,
            liabilities,
            nav,
            shares_outstanding: portfolio.shares_outstanding,
            nav_per_share,
        }
    }

    /// Returns the securities portion as a percentage of NAV.
    #[must_use]
    pub fn securities_pct(&self) -> Decimal {
        if self.nav > Decimal::ZERO {
            self.securities_market_value / self.nav * Decimal::ONE_HUNDRED
        } else {
            Decimal::ZERO
        }
    }

    /// Returns the cash portion as a percentage of NAV.
    #[must_use]
    pub fn cash_pct(&self) -> Decimal {
        if self.nav > Decimal::ZERO {
            self.total_cash / self.nav * Decimal::ONE_HUNDRED
        } else {
            Decimal::ZERO
        }
    }

    /// Returns the accrued interest as a percentage of NAV.
    #[must_use]
    pub fn accrued_pct(&self) -> Decimal {
        if self.nav > Decimal::ZERO {
            self.accrued_interest / self.nav * Decimal::ONE_HUNDRED
        } else {
            Decimal::ZERO
        }
    }
}

/// Calculates detailed NAV breakdown for a portfolio.
///
/// # Example
///
/// ```ignore
/// let breakdown = calculate_nav_breakdown(&portfolio);
/// println!("NAV: {}", breakdown.nav);
/// println!("Securities: {}%", breakdown.securities_pct());
/// ```
#[must_use]
pub fn calculate_nav_breakdown(portfolio: &Portfolio) -> NavBreakdown {
    NavBreakdown::from_portfolio(portfolio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CashPosition, Holding, HoldingAnalytics};
    use convex_bonds::types::BondIdentifiers;
    use convex_core::types::Currency;
    use rust_decimal_macros::dec;

    fn create_test_portfolio() -> Portfolio {
        let holding1 = Holding::builder()
            .id("BOND1")
            .identifiers(BondIdentifiers::new().with_ticker("TEST001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .accrued_interest(dec!(1.0))
            .analytics(HoldingAnalytics::new())
            .build()
            .unwrap();

        let holding2 = Holding::builder()
            .id("BOND2")
            .identifiers(BondIdentifiers::new().with_ticker("TEST002"))
            .par_amount(dec!(500_000))
            .market_price(dec!(98))
            .accrued_interest(dec!(0.5))
            .analytics(HoldingAnalytics::new())
            .build()
            .unwrap();

        Portfolio::builder("Test Portfolio")
            .id("TEST001")
            .as_of_date(convex_core::types::Date::from_ymd(2025, 1, 15).unwrap())
            .add_holding(holding1)
            .add_holding(holding2)
            .add_cash(CashPosition::new(dec!(100_000), Currency::USD))
            .shares_outstanding(dec!(1_000_000))
            .build()
            .unwrap()
    }

    #[test]
    fn test_nav_breakdown() {
        let portfolio = create_test_portfolio();
        let breakdown = calculate_nav_breakdown(&portfolio);

        // Securities: 1,000,000 + 490,000 = 1,490,000
        assert_eq!(breakdown.securities_market_value, dec!(1_490_000));

        // Accrued: 10,000 + 2,500 = 12,500
        assert_eq!(breakdown.accrued_interest, dec!(12_500));

        // Cash: 100,000
        assert_eq!(breakdown.total_cash, dec!(100_000));

        // NAV: 1,490,000 + 12,500 + 100,000 = 1,602,500
        assert_eq!(breakdown.nav, dec!(1_602_500));

        // NAV per share: 1,602,500 / 1,000,000 = 1.6025
        assert_eq!(breakdown.nav_per_share, Some(dec!(1.6025)));
    }

    #[test]
    fn test_nav_percentages() {
        let portfolio = create_test_portfolio();
        let breakdown = calculate_nav_breakdown(&portfolio);

        // Securities pct: 1,490,000 / 1,602,500 ≈ 92.98%
        let sec_pct = breakdown.securities_pct();
        assert!((sec_pct - dec!(92.98)).abs() < dec!(0.1));

        // Cash pct: 100,000 / 1,602,500 ≈ 6.24%
        let cash_pct = breakdown.cash_pct();
        assert!((cash_pct - dec!(6.24)).abs() < dec!(0.1));
    }

    #[test]
    fn test_nav_without_shares() {
        let portfolio = Portfolio::builder("No Shares")
            .id("NS001")
            .as_of_date(convex_core::types::Date::from_ymd(2025, 1, 15).unwrap())
            .add_cash(CashPosition::new(dec!(100_000), Currency::USD))
            .build()
            .unwrap();

        let breakdown = calculate_nav_breakdown(&portfolio);
        assert_eq!(breakdown.nav_per_share, None);
        assert_eq!(breakdown.shares_outstanding, None);
    }

    #[test]
    fn test_nav_with_liabilities() {
        let holding = Holding::builder()
            .id("BOND1")
            .identifiers(BondIdentifiers::new().with_ticker("TEST001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new())
            .build()
            .unwrap();

        let portfolio = Portfolio::builder("With Liabilities")
            .id("WL001")
            .as_of_date(convex_core::types::Date::from_ymd(2025, 1, 15).unwrap())
            .add_holding(holding)
            .add_cash(CashPosition::new(dec!(100_000), Currency::USD))
            .liabilities(dec!(50_000))
            .build()
            .unwrap();

        let breakdown = calculate_nav_breakdown(&portfolio);

        // NAV = 1,000,000 + 0 + 100,000 - 50,000 = 1,050,000
        assert_eq!(breakdown.nav, dec!(1_050_000));
        assert_eq!(breakdown.liabilities, dec!(50_000));
    }
}
