//! Portfolio analytics summary.
//!
//! Provides a comprehensive analytics summary combining all metrics.

use super::{NavBreakdown, RiskMetrics, SpreadMetrics, YieldMetrics};
use crate::types::AnalyticsConfig;
use crate::Portfolio;
use convex_core::types::{Currency, Date};
use serde::{Deserialize, Serialize};

/// Comprehensive portfolio analytics summary.
///
/// Contains all aggregated metrics for a portfolio in a single struct.
/// This is the primary output for portfolio-level analytics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioAnalytics {
    /// Portfolio identifier.
    pub portfolio_id: String,

    /// Portfolio name.
    pub portfolio_name: String,

    /// As-of date for the analytics.
    pub as_of_date: Date,

    /// Base currency for all values.
    pub base_currency: Currency,

    /// Number of holdings.
    pub holding_count: usize,

    /// NAV breakdown.
    pub nav: NavBreakdown,

    /// Yield metrics.
    pub yields: YieldMetrics,

    /// Risk metrics (duration, DV01, convexity).
    pub risk: RiskMetrics,

    /// Spread metrics.
    pub spreads: SpreadMetrics,

    /// Weighted average years to maturity.
    pub weighted_avg_maturity: Option<f64>,

    /// Weighted average coupon rate.
    pub weighted_avg_coupon: Option<f64>,
}

impl PortfolioAnalytics {
    /// Calculates complete analytics for a portfolio.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use convex_portfolio::prelude::*;
    ///
    /// let config = AnalyticsConfig::default();
    /// let analytics = PortfolioAnalytics::calculate(&portfolio, &config);
    ///
    /// println!("NAV: ${}", analytics.nav.nav);
    /// println!("Duration: {:?}", analytics.risk.best_duration);
    /// println!("Z-Spread: {:?} bps", analytics.spreads.z_spread);
    /// ```
    #[must_use]
    pub fn calculate(portfolio: &Portfolio, config: &AnalyticsConfig) -> Self {
        let nav = super::calculate_nav_breakdown(portfolio);
        let yields = super::calculate_yield_metrics(&portfolio.holdings, config);
        let risk = super::calculate_risk_metrics(
            &portfolio.holdings,
            portfolio.shares_outstanding,
            config,
        );
        let spreads = super::calculate_spread_metrics(
            &portfolio.holdings,
            portfolio.shares_outstanding,
            config,
        );

        let weighted_avg_maturity = weighted_maturity(&portfolio.holdings, config);
        let weighted_avg_coupon = weighted_coupon(&portfolio.holdings, config);

        Self {
            portfolio_id: portfolio.id.clone(),
            portfolio_name: portfolio.name.clone(),
            as_of_date: portfolio.as_of_date,
            base_currency: portfolio.base_currency,
            holding_count: portfolio.holding_count(),
            nav,
            yields,
            risk,
            spreads,
            weighted_avg_maturity,
            weighted_avg_coupon,
        }
    }

    /// Returns whether the portfolio has complete analytics data.
    ///
    /// Complete means all major metrics (YTM, duration, spread) are available.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.yields.ytm.is_some()
            && self.risk.best_duration.is_some()
            && self.spreads.best_spread.is_some()
    }

    /// Returns the overall data coverage as a percentage.
    ///
    /// Average of YTM, duration, and spread coverage.
    #[must_use]
    pub fn data_coverage_pct(&self) -> f64 {
        let ytm_cov = self.yields.ytm_coverage_pct();
        let dur_cov = self.risk.duration_coverage_pct();
        let spread_cov = self.spreads.z_spread_coverage_pct();

        (ytm_cov + dur_cov + spread_cov) / 3.0
    }
}

/// Calculates weighted average years to maturity.
fn weighted_maturity(holdings: &[crate::types::Holding], config: &AnalyticsConfig) -> Option<f64> {
    use crate::analytics::parallel::maybe_parallel_fold;
    use rust_decimal::prelude::ToPrimitive;

    let (sum_weighted, sum_weights) = maybe_parallel_fold(
        holdings,
        config,
        (0.0_f64, 0.0_f64),
        |(sum_w, sum_wt), h| {
            if let Some(maturity) = h.analytics.years_to_maturity {
                let weight = h.weight_value(config.weighting).to_f64().unwrap_or(0.0);
                (sum_w + maturity * weight, sum_wt + weight)
            } else {
                (sum_w, sum_wt)
            }
        },
        |(a, b), (c, d)| (a + c, b + d),
    );

    if sum_weights > 0.0 {
        Some(sum_weighted / sum_weights)
    } else {
        None
    }
}

fn weighted_coupon(holdings: &[crate::types::Holding], config: &AnalyticsConfig) -> Option<f64> {
    use crate::analytics::parallel::maybe_parallel_fold;
    use rust_decimal::prelude::ToPrimitive;

    let (sum_weighted, sum_weights) = maybe_parallel_fold(
        holdings,
        config,
        (0.0_f64, 0.0_f64),
        |(sum_w, sum_wt), h| {
            if let Some(coupon) = h.analytics.coupon_rate {
                let weight = h.weight_value(config.weighting).to_f64().unwrap_or(0.0);
                (sum_w + coupon * weight, sum_wt + weight)
            } else {
                (sum_w, sum_wt)
            }
        },
        |(a, b), (c, d)| (a + c, b + d),
    );

    if sum_weights > 0.0 {
        Some(sum_weighted / sum_weights)
    } else {
        None
    }
}

/// Convenience function to calculate portfolio analytics.
///
/// # Example
///
/// ```ignore
/// let analytics = calculate_portfolio_analytics(&portfolio, &config);
/// ```
#[must_use]
pub fn calculate_portfolio_analytics(
    portfolio: &Portfolio,
    config: &AnalyticsConfig,
) -> PortfolioAnalytics {
    PortfolioAnalytics::calculate(portfolio, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CashPosition, Holding, HoldingAnalytics};
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal_macros::dec;

    fn create_complete_holding(
        id: &str,
        par: rust_decimal::Decimal,
        price: rust_decimal::Decimal,
    ) -> Holding {
        Holding::builder()
            .id(id)
            .identifiers(BondIdentifiers::new().with_ticker(format!("TST{}", id)))
            .par_amount(par)
            .market_price(price)
            .accrued_interest(dec!(0.5))
            .analytics(
                HoldingAnalytics::new()
                    .with_ytm(0.05)
                    .with_ytw(0.045)
                    .with_modified_duration(5.0)
                    .with_convexity(50.0)
                    .with_dv01(0.05)
                    .with_z_spread(100.0)
                    .with_oas(95.0)
                    .with_years_to_maturity(5.5),
            )
            .build()
            .unwrap()
    }

    fn create_test_portfolio() -> Portfolio {
        Portfolio::builder("Test Portfolio")
            .id("TEST001")
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .add_holding(create_complete_holding("BOND1", dec!(1_000_000), dec!(100)))
            .add_holding(create_complete_holding("BOND2", dec!(500_000), dec!(98)))
            .add_cash(CashPosition::new(dec!(100_000), Currency::USD))
            .shares_outstanding(dec!(1_000_000))
            .build()
            .unwrap()
    }

    #[test]
    fn test_portfolio_analytics() {
        let portfolio = create_test_portfolio();
        let config = AnalyticsConfig::default();
        let analytics = PortfolioAnalytics::calculate(&portfolio, &config);

        assert_eq!(analytics.portfolio_id, "TEST001");
        assert_eq!(analytics.portfolio_name, "Test Portfolio");
        assert_eq!(analytics.holding_count, 2);
        assert_eq!(analytics.base_currency, Currency::USD);

        // NAV should be populated
        assert!(analytics.nav.nav > rust_decimal::Decimal::ZERO);
        assert!(analytics.nav.nav_per_share.is_some());

        // Yields should be populated
        assert!(analytics.yields.ytm.is_some());
        assert!(analytics.yields.ytw.is_some());

        // Risk should be populated
        assert!(analytics.risk.modified_duration.is_some());
        assert!(analytics.risk.total_dv01 > 0.0);

        // Spreads should be populated
        assert!(analytics.spreads.z_spread.is_some());
        assert!(analytics.spreads.oas.is_some());

        // Maturity should be populated
        assert!(analytics.weighted_avg_maturity.is_some());
    }

    #[test]
    fn test_is_complete() {
        let portfolio = create_test_portfolio();
        let config = AnalyticsConfig::default();
        let analytics = PortfolioAnalytics::calculate(&portfolio, &config);

        assert!(analytics.is_complete());
    }

    #[test]
    fn test_incomplete_analytics() {
        let holding = Holding::builder()
            .id("BOND1")
            .identifiers(BondIdentifiers::new().with_ticker("TEST001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new()) // No analytics
            .build()
            .unwrap();

        let portfolio = Portfolio::builder("Incomplete")
            .id("INC001")
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .add_holding(holding)
            .build()
            .unwrap();

        let config = AnalyticsConfig::default();
        let analytics = PortfolioAnalytics::calculate(&portfolio, &config);

        assert!(!analytics.is_complete());
        assert!(analytics.yields.ytm.is_none());
        assert!(analytics.risk.modified_duration.is_none());
    }

    #[test]
    fn test_data_coverage() {
        let portfolio = create_test_portfolio();
        let config = AnalyticsConfig::default();
        let analytics = PortfolioAnalytics::calculate(&portfolio, &config);

        // All holdings have all data, so coverage should be 100%
        assert!((analytics.data_coverage_pct() - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_weighted_maturity() {
        let portfolio = create_test_portfolio();
        let config = AnalyticsConfig::default();
        let analytics = PortfolioAnalytics::calculate(&portfolio, &config);

        // Both have 5.5 years, so weighted average is 5.5
        let wam = analytics.weighted_avg_maturity.unwrap();
        assert!((wam - 5.5).abs() < 0.01);
    }

    #[test]
    fn test_calculate_convenience_function() {
        let portfolio = create_test_portfolio();
        let config = AnalyticsConfig::default();

        let analytics = calculate_portfolio_analytics(&portfolio, &config);
        assert!(analytics.is_complete());
    }
}
