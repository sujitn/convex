//! ETF NAV and iNAV calculations.
//!
//! Provides NAV-related metrics specific to ETFs:
//! - NAV per share
//! - Indicative NAV (iNAV) during trading hours
//! - Premium/discount to market price
//! - NAV change analysis

use crate::types::{AnalyticsConfig, Holding};
use crate::Portfolio;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// ETF NAV and pricing metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EtfNavMetrics {
    /// Total NAV (securities + cash - liabilities).
    pub total_nav: Decimal,

    /// Shares outstanding.
    pub shares_outstanding: Decimal,

    /// NAV per share.
    pub nav_per_share: f64,

    /// Securities market value component.
    pub securities_value: Decimal,

    /// Cash and cash equivalents.
    pub cash_value: Decimal,

    /// Total accrued interest.
    pub accrued_interest: Decimal,

    /// Liabilities (if any).
    pub liabilities: Decimal,

    /// iNAV (indicative NAV) if calculable.
    pub inav: Option<f64>,

    /// Current market price of ETF shares (if provided).
    pub market_price: Option<f64>,

    /// Premium (positive) or discount (negative) to NAV in percentage.
    pub premium_discount_pct: Option<f64>,

    /// Premium/discount in dollars per share.
    pub premium_discount_dollars: Option<f64>,
}

impl EtfNavMetrics {
    /// Returns true if ETF is trading at a premium.
    #[must_use]
    pub fn is_premium(&self) -> bool {
        self.premium_discount_pct.map(|p| p > 0.0).unwrap_or(false)
    }

    /// Returns true if ETF is trading at a discount.
    #[must_use]
    pub fn is_discount(&self) -> bool {
        self.premium_discount_pct.map(|p| p < 0.0).unwrap_or(false)
    }

    /// Returns the absolute premium/discount in percentage points.
    #[must_use]
    pub fn abs_premium_discount(&self) -> Option<f64> {
        self.premium_discount_pct.map(|p| p.abs())
    }
}

/// Calculates ETF NAV metrics from a portfolio.
///
/// # Arguments
///
/// * `portfolio` - The ETF portfolio (must have shares_outstanding set)
/// * `market_price` - Optional current market price of ETF shares
///
/// # Returns
///
/// Complete NAV metrics including premium/discount if market price is provided.
///
/// # Example
///
/// ```rust,ignore
/// use convex_portfolio::etf::calculate_etf_nav;
///
/// let nav_metrics = calculate_etf_nav(&portfolio, Some(25.50));
///
/// if nav_metrics.is_premium() {
///     println!("Trading at {:.2}% premium", nav_metrics.premium_discount_pct.unwrap());
/// }
/// ```
#[must_use]
pub fn calculate_etf_nav(portfolio: &Portfolio, market_price: Option<f64>) -> EtfNavMetrics {
    let shares = portfolio.shares_outstanding.unwrap_or(Decimal::ONE);
    let total_nav = portfolio.nav();
    let nav_per_share = total_nav.to_f64().unwrap_or(0.0) / shares.to_f64().unwrap_or(1.0);

    // Calculate premium/discount if market price is provided
    let (premium_discount_pct, premium_discount_dollars) = if let Some(price) = market_price {
        if nav_per_share > 0.0 {
            let diff = price - nav_per_share;
            let pct = (diff / nav_per_share) * 100.0;
            (Some(pct), Some(diff))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    EtfNavMetrics {
        total_nav,
        shares_outstanding: shares,
        nav_per_share,
        securities_value: portfolio.securities_market_value(),
        cash_value: portfolio.total_cash(),
        accrued_interest: portfolio.total_accrued_interest(),
        liabilities: portfolio.liabilities.unwrap_or(Decimal::ZERO),
        inav: None, // Would require real-time pricing data
        market_price,
        premium_discount_pct,
        premium_discount_dollars,
    }
}

/// Calculates indicative NAV (iNAV) with updated prices.
///
/// iNAV is calculated during trading hours using real-time or
/// near-real-time prices for the underlying holdings.
///
/// # Arguments
///
/// * `holdings` - Portfolio holdings with updated prices
/// * `cash` - Total cash position in base currency
/// * `shares_outstanding` - Number of ETF shares outstanding
/// * `liabilities` - Optional liabilities to subtract
/// * `config` - Analytics configuration
///
/// # Returns
///
/// iNAV per share.
///
/// # Example
///
/// ```rust,ignore
/// use convex_portfolio::etf::calculate_inav;
///
/// // Update holdings with real-time prices and recalculate
/// let inav = calculate_inav(&updated_holdings, cash, shares, None, &config);
/// println!("iNAV: ${:.4}", inav);
/// ```
#[must_use]
pub fn calculate_inav(
    holdings: &[Holding],
    cash: Decimal,
    shares_outstanding: Decimal,
    liabilities: Option<Decimal>,
    _config: &AnalyticsConfig,
) -> f64 {
    // Sum market values
    let securities_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();

    // Total accrued
    let accrued: Decimal = holdings.iter().map(|h| h.accrued_amount()).sum();

    // Total NAV
    let total_nav = securities_mv + accrued + cash - liabilities.unwrap_or(Decimal::ZERO);

    // Per share
    total_nav.to_f64().unwrap_or(0.0) / shares_outstanding.to_f64().unwrap_or(1.0)
}

/// Calculates premium/discount given NAV and market price.
///
/// # Formula
///
/// ```text
/// Premium/Discount = (Market Price - NAV) / NAV × 100
/// ```
///
/// Returns percentage (positive = premium, negative = discount).
#[must_use]
pub fn premium_discount(nav_per_share: f64, market_price: f64) -> Option<f64> {
    if nav_per_share > 0.0 {
        Some((market_price - nav_per_share) / nav_per_share * 100.0)
    } else {
        None
    }
}

/// Historical premium/discount tracking point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PremiumDiscountPoint {
    /// Date of observation.
    pub date: convex_core::types::Date,

    /// NAV per share on this date.
    pub nav: f64,

    /// Market price on this date.
    pub market_price: f64,

    /// Premium/discount percentage.
    pub premium_discount_pct: f64,
}

/// Premium/discount statistics over a period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PremiumDiscountStats {
    /// Number of observations.
    pub count: usize,

    /// Average premium/discount (%).
    pub average: f64,

    /// Median premium/discount (%).
    pub median: f64,

    /// Standard deviation of premium/discount.
    pub std_dev: f64,

    /// Maximum premium (%).
    pub max_premium: f64,

    /// Maximum discount (%) - stored as negative.
    pub max_discount: f64,

    /// Days at premium.
    pub days_at_premium: usize,

    /// Days at discount.
    pub days_at_discount: usize,

    /// Days at par (within 0.05% threshold).
    pub days_at_par: usize,
}

impl PremiumDiscountStats {
    /// Percentage of days at premium.
    #[must_use]
    pub fn pct_at_premium(&self) -> f64 {
        if self.count > 0 {
            self.days_at_premium as f64 / self.count as f64 * 100.0
        } else {
            0.0
        }
    }

    /// Percentage of days at discount.
    #[must_use]
    pub fn pct_at_discount(&self) -> f64 {
        if self.count > 0 {
            self.days_at_discount as f64 / self.count as f64 * 100.0
        } else {
            0.0
        }
    }
}

/// Calculates premium/discount statistics from historical data.
///
/// # Arguments
///
/// * `history` - Historical premium/discount observations
///
/// # Returns
///
/// Statistics including average, median, and distribution of premium/discount.
#[must_use]
pub fn calculate_premium_discount_stats(history: &[PremiumDiscountPoint]) -> PremiumDiscountStats {
    if history.is_empty() {
        return PremiumDiscountStats {
            count: 0,
            average: 0.0,
            median: 0.0,
            std_dev: 0.0,
            max_premium: 0.0,
            max_discount: 0.0,
            days_at_premium: 0,
            days_at_discount: 0,
            days_at_par: 0,
        };
    }

    let count = history.len();
    let values: Vec<f64> = history.iter().map(|p| p.premium_discount_pct).collect();

    // Average
    let sum: f64 = values.iter().sum();
    let average = sum / count as f64;

    // Median
    let mut sorted = values.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = if count % 2 == 0 {
        (sorted[count / 2 - 1] + sorted[count / 2]) / 2.0
    } else {
        sorted[count / 2]
    };

    // Standard deviation
    let variance: f64 = values.iter().map(|v| (v - average).powi(2)).sum::<f64>() / count as f64;
    let std_dev = variance.sqrt();

    // Max premium and discount
    let max_premium = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let max_discount = values.iter().cloned().fold(f64::INFINITY, f64::min);

    // Count by category (using 0.05% threshold for "at par")
    const PAR_THRESHOLD: f64 = 0.05;
    let days_at_premium = values.iter().filter(|&&v| v > PAR_THRESHOLD).count();
    let days_at_discount = values.iter().filter(|&&v| v < -PAR_THRESHOLD).count();
    let days_at_par = values.iter().filter(|&&v| v.abs() <= PAR_THRESHOLD).count();

    PremiumDiscountStats {
        count,
        average,
        median,
        std_dev,
        max_premium,
        max_discount,
        days_at_premium,
        days_at_discount,
        days_at_par,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::PortfolioBuilder;
    use crate::types::{CashPosition, HoldingAnalytics, HoldingBuilder};
    use convex_bonds::types::BondIdentifiers;
    use convex_core::types::{Currency, Date};
    use rust_decimal_macros::dec;

    fn create_test_portfolio() -> Portfolio {
        let holding = HoldingBuilder::new()
            .id("H1")
            .identifiers(BondIdentifiers::from_isin_str("US912828Z229").unwrap())
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new())
            .build()
            .unwrap();

        PortfolioBuilder::new()
            .name("TestETF")
            .base_currency(Currency::USD)
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .add_holding(holding)
            .add_cash(CashPosition::new(dec!(50_000), Currency::USD))
            .shares_outstanding(dec!(100_000))
            .build()
            .unwrap()
    }

    #[test]
    fn test_calculate_etf_nav_basic() {
        let portfolio = create_test_portfolio();
        let metrics = calculate_etf_nav(&portfolio, None);

        // NAV = 1,000,000 + 50,000 = 1,050,000
        assert!((metrics.total_nav - dec!(1_050_000)).abs() < dec!(0.01));
        // NAV per share = 1,050,000 / 100,000 = 10.50
        assert!((metrics.nav_per_share - 10.50).abs() < 0.001);
        assert!(metrics.premium_discount_pct.is_none());
    }

    #[test]
    fn test_calculate_etf_nav_with_market_price() {
        let portfolio = create_test_portfolio();

        // Trading at premium
        let metrics_premium = calculate_etf_nav(&portfolio, Some(10.75));
        assert!(metrics_premium.is_premium());
        // (10.75 - 10.50) / 10.50 * 100 ≈ 2.38%
        assert!((metrics_premium.premium_discount_pct.unwrap() - 2.38).abs() < 0.1);

        // Trading at discount
        let metrics_discount = calculate_etf_nav(&portfolio, Some(10.25));
        assert!(metrics_discount.is_discount());
        // (10.25 - 10.50) / 10.50 * 100 ≈ -2.38%
        assert!((metrics_discount.premium_discount_pct.unwrap() - (-2.38)).abs() < 0.1);
    }

    #[test]
    fn test_premium_discount_function() {
        // At premium
        let result = premium_discount(100.0, 102.0);
        assert!((result.unwrap() - 2.0).abs() < 0.01);

        // At discount
        let result = premium_discount(100.0, 98.0);
        assert!((result.unwrap() - (-2.0)).abs() < 0.01);

        // At par
        let result = premium_discount(100.0, 100.0);
        assert!(result.unwrap().abs() < 0.01);

        // Zero NAV
        let result = premium_discount(0.0, 100.0);
        assert!(result.is_none());
    }

    #[test]
    fn test_calculate_inav() {
        let holding = HoldingBuilder::new()
            .id("H1")
            .identifiers(BondIdentifiers::from_isin_str("US912828Z229").unwrap())
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new())
            .build()
            .unwrap();

        let holdings = vec![holding];
        let cash = dec!(50_000);
        let shares = dec!(100_000);
        let config = AnalyticsConfig::default();

        let inav = calculate_inav(&holdings, cash, shares, None, &config);

        // iNAV = (1,000,000 + 50,000) / 100,000 = 10.50
        assert!((inav - 10.50).abs() < 0.001);
    }

    #[test]
    fn test_premium_discount_stats_empty() {
        let stats = calculate_premium_discount_stats(&[]);
        assert_eq!(stats.count, 0);
        assert_eq!(stats.average, 0.0);
    }

    #[test]
    fn test_premium_discount_stats() {
        let history = vec![
            PremiumDiscountPoint {
                date: Date::from_ymd(2025, 1, 1).unwrap(),
                nav: 100.0,
                market_price: 101.0,
                premium_discount_pct: 1.0,
            },
            PremiumDiscountPoint {
                date: Date::from_ymd(2025, 1, 2).unwrap(),
                nav: 100.0,
                market_price: 99.0,
                premium_discount_pct: -1.0,
            },
            PremiumDiscountPoint {
                date: Date::from_ymd(2025, 1, 3).unwrap(),
                nav: 100.0,
                market_price: 100.5,
                premium_discount_pct: 0.5,
            },
            PremiumDiscountPoint {
                date: Date::from_ymd(2025, 1, 4).unwrap(),
                nav: 100.0,
                market_price: 100.0,
                premium_discount_pct: 0.0,
            },
        ];

        let stats = calculate_premium_discount_stats(&history);

        assert_eq!(stats.count, 4);
        // Average = (1.0 - 1.0 + 0.5 + 0.0) / 4 = 0.125
        assert!((stats.average - 0.125).abs() < 0.001);
        // Median of sorted [-1.0, 0.0, 0.5, 1.0] = (0.0 + 0.5) / 2 = 0.25
        assert!((stats.median - 0.25).abs() < 0.001);
        assert!((stats.max_premium - 1.0).abs() < 0.001);
        assert!((stats.max_discount - (-1.0)).abs() < 0.001);
        assert_eq!(stats.days_at_premium, 2); // 1.0% and 0.5%
        assert_eq!(stats.days_at_discount, 1); // -1.0%
        assert_eq!(stats.days_at_par, 1); // 0.0%
    }

    #[test]
    fn test_premium_discount_stats_percentages() {
        let history = vec![
            PremiumDiscountPoint {
                date: Date::from_ymd(2025, 1, 1).unwrap(),
                nav: 100.0,
                market_price: 101.0,
                premium_discount_pct: 1.0,
            },
            PremiumDiscountPoint {
                date: Date::from_ymd(2025, 1, 2).unwrap(),
                nav: 100.0,
                market_price: 99.0,
                premium_discount_pct: -1.0,
            },
        ];

        let stats = calculate_premium_discount_stats(&history);

        assert!((stats.pct_at_premium() - 50.0).abs() < 0.01);
        assert!((stats.pct_at_discount() - 50.0).abs() < 0.01);
    }
}
