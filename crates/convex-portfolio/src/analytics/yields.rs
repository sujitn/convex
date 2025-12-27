//! Yield analytics for portfolios.
//!
//! Provides weighted average yield calculations including YTM, YTW, YTC,
//! and current yield. Supports configurable weighting methods and
//! optional parallel processing.

use crate::analytics::parallel::maybe_parallel_fold;
use crate::types::{AnalyticsConfig, Holding, WeightingMethod};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Aggregated yield metrics for a portfolio.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct YieldMetrics {
    /// Weighted average yield to maturity.
    pub ytm: Option<f64>,

    /// Weighted average yield to worst.
    pub ytw: Option<f64>,

    /// Weighted average yield to call.
    pub ytc: Option<f64>,

    /// Weighted average current yield.
    pub current_yield: Option<f64>,

    /// Best available yield (prefers YTW over YTM).
    pub best_yield: Option<f64>,

    /// Number of holdings with YTM data.
    pub ytm_coverage: usize,

    /// Number of holdings with YTW data.
    pub ytw_coverage: usize,

    /// Total number of holdings.
    pub total_holdings: usize,
}

impl YieldMetrics {
    /// Returns the YTM coverage as a percentage.
    #[must_use]
    pub fn ytm_coverage_pct(&self) -> f64 {
        if self.total_holdings > 0 {
            self.ytm_coverage as f64 / self.total_holdings as f64 * 100.0
        } else {
            0.0
        }
    }

    /// Returns the YTW coverage as a percentage.
    #[must_use]
    pub fn ytw_coverage_pct(&self) -> f64 {
        if self.total_holdings > 0 {
            self.ytw_coverage as f64 / self.total_holdings as f64 * 100.0
        } else {
            0.0
        }
    }
}

/// Calculates weighted average yield to maturity.
///
/// ## Formula
///
/// ```text
/// YTM_portfolio = Σ(w_i × YTM_i) / Σ(w_i)
/// ```
///
/// Where:
/// - `w_i` = weight of holding i (based on weighting method)
/// - `YTM_i` = yield to maturity of holding i
///
/// Holdings without YTM are excluded from the calculation.
///
/// # Returns
///
/// Returns `None` if no holdings have YTM data.
#[must_use]
pub fn weighted_ytm(holdings: &[Holding], config: &AnalyticsConfig) -> Option<f64> {
    weighted_metric(holdings, config, |h| h.analytics.ytm)
}

/// Calculates weighted average yield to worst.
///
/// Yield to worst is the lower of YTM and yield to all call dates,
/// used for callable bonds.
///
/// # Returns
///
/// Returns `None` if no holdings have YTW data.
#[must_use]
pub fn weighted_ytw(holdings: &[Holding], config: &AnalyticsConfig) -> Option<f64> {
    weighted_metric(holdings, config, |h| h.analytics.ytw)
}

/// Calculates weighted average yield to call.
///
/// # Returns
///
/// Returns `None` if no holdings have YTC data.
#[must_use]
pub fn weighted_ytc(holdings: &[Holding], config: &AnalyticsConfig) -> Option<f64> {
    weighted_metric(holdings, config, |h| h.analytics.ytc)
}

/// Calculates weighted average current yield.
///
/// Current yield = Annual Coupon / Clean Price
///
/// # Returns
///
/// Returns `None` if no holdings have current yield data.
#[must_use]
pub fn weighted_current_yield(holdings: &[Holding], config: &AnalyticsConfig) -> Option<f64> {
    weighted_metric(holdings, config, |h| h.analytics.current_yield)
}

/// Calculates weighted best yield (prefers YTW over YTM).
///
/// For each holding, uses YTW if available, otherwise YTM.
///
/// # Returns
///
/// Returns `None` if no holdings have yield data.
#[must_use]
pub fn weighted_best_yield(holdings: &[Holding], config: &AnalyticsConfig) -> Option<f64> {
    weighted_metric(holdings, config, |h| h.analytics.best_yield())
}

/// Calculates all yield metrics for a portfolio.
///
/// # Example
///
/// ```ignore
/// let metrics = calculate_yield_metrics(&portfolio.holdings, &config);
/// println!("Portfolio YTM: {:?}", metrics.ytm);
/// println!("YTM coverage: {:.1}%", metrics.ytm_coverage_pct());
/// ```
#[must_use]
pub fn calculate_yield_metrics(holdings: &[Holding], config: &AnalyticsConfig) -> YieldMetrics {
    let ytm_coverage = holdings
        .iter()
        .filter(|h| h.analytics.ytm.is_some())
        .count();
    let ytw_coverage = holdings
        .iter()
        .filter(|h| h.analytics.ytw.is_some())
        .count();

    YieldMetrics {
        ytm: weighted_ytm(holdings, config),
        ytw: weighted_ytw(holdings, config),
        ytc: weighted_ytc(holdings, config),
        current_yield: weighted_current_yield(holdings, config),
        best_yield: weighted_best_yield(holdings, config),
        ytm_coverage,
        ytw_coverage,
        total_holdings: holdings.len(),
    }
}

/// Internal helper to calculate weighted average of any metric.
fn weighted_metric<F>(holdings: &[Holding], config: &AnalyticsConfig, get_value: F) -> Option<f64>
where
    F: Fn(&Holding) -> Option<f64> + Sync,
{
    let (sum_weighted, sum_weights) = maybe_parallel_fold(
        holdings,
        config,
        (0.0_f64, 0.0_f64),
        |(sum_w, sum_wt), h| {
            if let Some(value) = get_value(h) {
                let weight = weight_for_holding(h, config.weighting);
                (sum_w + value * weight, sum_wt + weight)
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

/// Returns the weight for a holding as f64.
fn weight_for_holding(holding: &Holding, method: WeightingMethod) -> f64 {
    let weight_dec = holding.weight_value(method);
    decimal_to_f64(weight_dec)
}

/// Converts Decimal to f64.
fn decimal_to_f64(d: Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::HoldingAnalytics;
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal_macros::dec;

    fn create_holding(
        id: &str,
        par: Decimal,
        price: Decimal,
        ytm: f64,
        ytw: Option<f64>,
    ) -> Holding {
        let mut analytics = HoldingAnalytics::new().with_ytm(ytm);
        if let Some(ytw_val) = ytw {
            analytics = analytics.with_ytw(ytw_val);
        }

        Holding::builder()
            .id(id)
            .identifiers(BondIdentifiers::new().with_ticker(format!("TST{}", id)))
            .par_amount(par)
            .market_price(price)
            .analytics(analytics)
            .build()
            .unwrap()
    }

    #[test]
    fn test_weighted_ytm() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(100), 0.05, None),
            create_holding("BOND2", dec!(500_000), dec!(100), 0.06, None),
        ];

        let config = AnalyticsConfig::default();
        let ytm = weighted_ytm(&holdings, &config).unwrap();

        // Weighted by MV: (1M × 5% + 0.5M × 6%) / 1.5M = 5.33%
        assert!((ytm - 0.0533).abs() < 0.001);
    }

    #[test]
    fn test_weighted_ytm_with_prices() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(98), 0.05, None), // MV = 980,000
            create_holding("BOND2", dec!(500_000), dec!(102), 0.06, None),  // MV = 510,000
        ];

        let config = AnalyticsConfig::default();
        let ytm = weighted_ytm(&holdings, &config).unwrap();

        // Weighted by MV: (980k × 5% + 510k × 6%) / 1.49M
        let expected = (980_000.0 * 0.05 + 510_000.0 * 0.06) / 1_490_000.0;
        assert!((ytm - expected).abs() < 0.0001);
    }

    #[test]
    fn test_weighted_ytw() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(100), 0.05, Some(0.04)),
            create_holding("BOND2", dec!(1_000_000), dec!(100), 0.06, Some(0.055)),
        ];

        let config = AnalyticsConfig::default();
        let ytw = weighted_ytw(&holdings, &config).unwrap();

        // Equal MV, so average of 4% and 5.5% = 4.75%
        assert!((ytw - 0.0475).abs() < 0.001);
    }

    #[test]
    fn test_weighted_best_yield() {
        let holdings = vec![
            // This one has YTW, so uses 4%
            create_holding("BOND1", dec!(1_000_000), dec!(100), 0.05, Some(0.04)),
            // This one has no YTW, so uses YTM 6%
            create_holding("BOND2", dec!(1_000_000), dec!(100), 0.06, None),
        ];

        let config = AnalyticsConfig::default();
        let best = weighted_best_yield(&holdings, &config).unwrap();

        // Equal MV: (4% + 6%) / 2 = 5%
        assert!((best - 0.05).abs() < 0.001);
    }

    #[test]
    fn test_no_yield_data() {
        let holdings = vec![Holding::builder()
            .id("BOND1")
            .identifiers(BondIdentifiers::new().with_ticker("TEST"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new()) // No yield data
            .build()
            .unwrap()];

        let config = AnalyticsConfig::default();
        assert!(weighted_ytm(&holdings, &config).is_none());
    }

    #[test]
    fn test_empty_portfolio() {
        let holdings: Vec<Holding> = vec![];
        let config = AnalyticsConfig::default();
        assert!(weighted_ytm(&holdings, &config).is_none());
    }

    #[test]
    fn test_par_value_weighting() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(90), 0.05, None), // Par 1M, MV 900k
            create_holding("BOND2", dec!(500_000), dec!(110), 0.06, None),  // Par 500k, MV 550k
        ];

        let config = AnalyticsConfig::default().with_weighting(WeightingMethod::ParValue);
        let ytm = weighted_ytm(&holdings, &config).unwrap();

        // Par weighted: (1M × 5% + 0.5M × 6%) / 1.5M = 5.33%
        assert!((ytm - 0.0533).abs() < 0.001);
    }

    #[test]
    fn test_equal_weighting() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(100), 0.05, None),
            create_holding("BOND2", dec!(100_000), dec!(100), 0.07, None),
        ];

        let config = AnalyticsConfig::default().with_weighting(WeightingMethod::EqualWeight);
        let ytm = weighted_ytm(&holdings, &config).unwrap();

        // Equal weight: (5% + 7%) / 2 = 6%
        assert!((ytm - 0.06).abs() < 0.001);
    }

    #[test]
    fn test_yield_metrics() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(100), 0.05, Some(0.04)),
            create_holding("BOND2", dec!(1_000_000), dec!(100), 0.06, None),
        ];

        let config = AnalyticsConfig::default();
        let metrics = calculate_yield_metrics(&holdings, &config);

        assert!(metrics.ytm.is_some());
        assert!(metrics.ytw.is_some()); // Only one has YTW
        assert!(metrics.best_yield.is_some());

        assert_eq!(metrics.ytm_coverage, 2);
        assert_eq!(metrics.ytw_coverage, 1);
        assert_eq!(metrics.total_holdings, 2);
        assert!((metrics.ytm_coverage_pct() - 100.0).abs() < 0.01);
        assert!((metrics.ytw_coverage_pct() - 50.0).abs() < 0.01);
    }
}
