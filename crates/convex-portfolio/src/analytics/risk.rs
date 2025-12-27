//! Risk analytics for portfolios.
//!
//! Provides aggregated risk metrics including:
//! - Weighted duration (modified, effective, Macaulay, spread)
//! - Total DV01 (dollar value of 1bp)
//! - Weighted convexity
//!
//! Follows Bloomberg PORT methodology for aggregation.

use crate::analytics::parallel::maybe_parallel_fold;
use crate::types::{AnalyticsConfig, Holding, WeightingMethod};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Aggregated risk metrics for a portfolio.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RiskMetrics {
    /// Weighted average modified duration.
    pub modified_duration: Option<f64>,

    /// Weighted average effective duration.
    pub effective_duration: Option<f64>,

    /// Weighted average Macaulay duration.
    pub macaulay_duration: Option<f64>,

    /// Weighted average spread duration.
    pub spread_duration: Option<f64>,

    /// Best available duration (prefers effective over modified).
    pub best_duration: Option<f64>,

    /// Weighted average convexity.
    pub convexity: Option<f64>,

    /// Weighted average effective convexity.
    pub effective_convexity: Option<f64>,

    /// Total DV01 (sum of all position DV01s).
    pub total_dv01: f64,

    /// DV01 per share (if shares outstanding is set).
    pub dv01_per_share: Option<f64>,

    /// Number of holdings with duration data.
    pub duration_coverage: usize,

    /// Number of holdings with DV01 data.
    pub dv01_coverage: usize,

    /// Total number of holdings.
    pub total_holdings: usize,
}

impl RiskMetrics {
    /// Returns the duration coverage as a percentage.
    #[must_use]
    pub fn duration_coverage_pct(&self) -> f64 {
        if self.total_holdings > 0 {
            self.duration_coverage as f64 / self.total_holdings as f64 * 100.0
        } else {
            0.0
        }
    }

    /// Returns the DV01 coverage as a percentage.
    #[must_use]
    pub fn dv01_coverage_pct(&self) -> f64 {
        if self.total_holdings > 0 {
            self.dv01_coverage as f64 / self.total_holdings as f64 * 100.0
        } else {
            0.0
        }
    }
}

/// Calculates weighted average modified duration.
///
/// ## Formula
///
/// ```text
/// Duration_portfolio = Σ(w_i × Duration_i) / Σ(w_i)
/// ```
///
/// Where weights are based on the configured weighting method
/// (typically market value).
///
/// # Returns
///
/// Returns `None` if no holdings have modified duration data.
#[must_use]
pub fn weighted_modified_duration(holdings: &[Holding], config: &AnalyticsConfig) -> Option<f64> {
    weighted_metric(holdings, config, |h| h.analytics.modified_duration)
}

/// Calculates weighted average effective duration.
///
/// Effective duration accounts for embedded options and is preferred
/// for callable bonds.
///
/// # Returns
///
/// Returns `None` if no holdings have effective duration data.
#[must_use]
pub fn weighted_effective_duration(holdings: &[Holding], config: &AnalyticsConfig) -> Option<f64> {
    weighted_metric(holdings, config, |h| h.analytics.effective_duration)
}

/// Calculates weighted average Macaulay duration.
///
/// # Returns
///
/// Returns `None` if no holdings have Macaulay duration data.
#[must_use]
pub fn weighted_macaulay_duration(holdings: &[Holding], config: &AnalyticsConfig) -> Option<f64> {
    weighted_metric(holdings, config, |h| h.analytics.macaulay_duration)
}

/// Calculates weighted average spread duration.
///
/// Spread duration measures sensitivity to credit spread changes.
///
/// # Returns
///
/// Returns `None` if no holdings have spread duration data.
#[must_use]
pub fn weighted_spread_duration(holdings: &[Holding], config: &AnalyticsConfig) -> Option<f64> {
    weighted_metric(holdings, config, |h| h.analytics.spread_duration)
}

/// Calculates weighted best duration (prefers effective over modified).
///
/// For each holding, uses effective duration if available, otherwise modified.
///
/// # Returns
///
/// Returns `None` if no holdings have duration data.
#[must_use]
pub fn weighted_best_duration(holdings: &[Holding], config: &AnalyticsConfig) -> Option<f64> {
    weighted_metric(holdings, config, |h| h.analytics.best_duration())
}

/// Calculates weighted average convexity.
///
/// # Returns
///
/// Returns `None` if no holdings have convexity data.
#[must_use]
pub fn weighted_convexity(holdings: &[Holding], config: &AnalyticsConfig) -> Option<f64> {
    weighted_metric(holdings, config, |h| h.analytics.convexity)
}

/// Calculates weighted average effective convexity.
///
/// # Returns
///
/// Returns `None` if no holdings have effective convexity data.
#[must_use]
pub fn weighted_effective_convexity(holdings: &[Holding], config: &AnalyticsConfig) -> Option<f64> {
    weighted_metric(holdings, config, |h| h.analytics.effective_convexity)
}

/// Calculates total portfolio DV01.
///
/// ## Formula
///
/// ```text
/// DV01_portfolio = Σ(DV01_i × par_i / 100 × fx_rate_i)
/// ```
///
/// DV01 is additive, so we sum individual position DV01s.
/// This represents the dollar change in portfolio value for a 1bp
/// parallel shift in rates.
#[must_use]
pub fn total_dv01(holdings: &[Holding], config: &AnalyticsConfig) -> f64 {
    maybe_parallel_fold(
        holdings,
        config,
        0.0_f64,
        |acc, h| {
            if let Some(dv01_per_par) = h.analytics.dv01 {
                let par = h.par_amount.to_f64().unwrap_or(0.0);
                let fx = h.fx_rate.to_f64().unwrap_or(1.0);
                // DV01 per 100 par × par / 100 × FX = total DV01 in base currency
                acc + dv01_per_par * par / 100.0 * fx
            } else {
                acc
            }
        },
        |a, b| a + b,
    )
}

/// Calculates DV01 per share.
///
/// # Returns
///
/// Returns `None` if shares outstanding is not set or is zero.
#[must_use]
pub fn dv01_per_share(
    holdings: &[Holding],
    shares: Option<Decimal>,
    config: &AnalyticsConfig,
) -> Option<f64> {
    let total = total_dv01(holdings, config);
    shares.and_then(|s| {
        let shares_f64 = s.to_f64().unwrap_or(0.0);
        if shares_f64 > 0.0 {
            Some(total / shares_f64)
        } else {
            None
        }
    })
}

/// Calculates all risk metrics for a portfolio.
///
/// # Example
///
/// ```ignore
/// let metrics = calculate_risk_metrics(&portfolio.holdings, portfolio.shares_outstanding, &config);
/// println!("Portfolio Duration: {:?}", metrics.best_duration);
/// println!("Total DV01: ${:.2}", metrics.total_dv01);
/// ```
#[must_use]
pub fn calculate_risk_metrics(
    holdings: &[Holding],
    shares_outstanding: Option<Decimal>,
    config: &AnalyticsConfig,
) -> RiskMetrics {
    let duration_coverage = holdings
        .iter()
        .filter(|h| h.analytics.best_duration().is_some())
        .count();
    let dv01_coverage = holdings
        .iter()
        .filter(|h| h.analytics.dv01.is_some())
        .count();

    let total_dv01_val = total_dv01(holdings, config);

    RiskMetrics {
        modified_duration: weighted_modified_duration(holdings, config),
        effective_duration: weighted_effective_duration(holdings, config),
        macaulay_duration: weighted_macaulay_duration(holdings, config),
        spread_duration: weighted_spread_duration(holdings, config),
        best_duration: weighted_best_duration(holdings, config),
        convexity: weighted_convexity(holdings, config),
        effective_convexity: weighted_effective_convexity(holdings, config),
        total_dv01: total_dv01_val,
        dv01_per_share: dv01_per_share(holdings, shares_outstanding, config),
        duration_coverage,
        dv01_coverage,
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
    weight_dec.to_f64().unwrap_or(0.0)
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
        duration: f64,
        dv01: f64,
        convexity: f64,
    ) -> Holding {
        Holding::builder()
            .id(id)
            .identifiers(BondIdentifiers::new().with_ticker(format!("TST{}", id)))
            .par_amount(par)
            .market_price(price)
            .analytics(
                HoldingAnalytics::new()
                    .with_modified_duration(duration)
                    .with_dv01(dv01)
                    .with_convexity(convexity),
            )
            .build()
            .unwrap()
    }

    #[test]
    fn test_weighted_duration() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(100), 5.0, 0.05, 50.0),
            create_holding("BOND2", dec!(500_000), dec!(100), 7.0, 0.07, 70.0),
        ];

        let config = AnalyticsConfig::default();
        let duration = weighted_modified_duration(&holdings, &config).unwrap();

        // MV weighted: (1M × 5 + 0.5M × 7) / 1.5M = 5.67
        assert!((duration - 5.67).abs() < 0.01);
    }

    #[test]
    fn test_weighted_duration_with_prices() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(98), 5.0, 0.05, 50.0), // MV = 980k
            create_holding("BOND2", dec!(500_000), dec!(102), 7.0, 0.07, 70.0),  // MV = 510k
        ];

        let config = AnalyticsConfig::default();
        let duration = weighted_modified_duration(&holdings, &config).unwrap();

        // MV weighted: (980k × 5 + 510k × 7) / 1.49M
        let expected = (980_000.0 * 5.0 + 510_000.0 * 7.0) / 1_490_000.0;
        assert!((duration - expected).abs() < 0.01);
    }

    #[test]
    fn test_total_dv01() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(100), 5.0, 0.05, 50.0),
            create_holding("BOND2", dec!(500_000), dec!(100), 7.0, 0.07, 70.0),
        ];

        let config = AnalyticsConfig::default();
        let dv01 = total_dv01(&holdings, &config);

        // Bond1: 0.05 × 1M / 100 = 500
        // Bond2: 0.07 × 500k / 100 = 350
        // Total: 850
        assert!((dv01 - 850.0).abs() < 0.1);
    }

    #[test]
    fn test_dv01_with_fx() {
        let eur_holding = Holding::builder()
            .id("EUR_BOND")
            .identifiers(BondIdentifiers::new().with_ticker("EUR001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .currency(convex_core::types::Currency::EUR)
            .fx_rate(dec!(1.10)) // 1 EUR = 1.10 USD
            .analytics(HoldingAnalytics::new().with_dv01(0.05))
            .build()
            .unwrap();

        let holdings = vec![eur_holding];
        let config = AnalyticsConfig::default();
        let dv01 = total_dv01(&holdings, &config);

        // DV01 = 0.05 × 1M / 100 × 1.10 = 550
        assert!((dv01 - 550.0).abs() < 0.1);
    }

    #[test]
    fn test_dv01_per_share() {
        let holdings = vec![create_holding(
            "BOND1",
            dec!(1_000_000),
            dec!(100),
            5.0,
            0.05,
            50.0,
        )];

        let config = AnalyticsConfig::default();
        let per_share = dv01_per_share(&holdings, Some(dec!(1_000_000)), &config).unwrap();

        // DV01 = 500, shares = 1M, per share = 0.0005
        assert!((per_share - 0.0005).abs() < 0.0001);
    }

    #[test]
    fn test_weighted_convexity() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(100), 5.0, 0.05, 50.0),
            create_holding("BOND2", dec!(1_000_000), dec!(100), 7.0, 0.07, 70.0),
        ];

        let config = AnalyticsConfig::default();
        let convexity = weighted_convexity(&holdings, &config).unwrap();

        // Equal MV: (50 + 70) / 2 = 60
        assert!((convexity - 60.0).abs() < 0.1);
    }

    #[test]
    fn test_best_duration() {
        let holding_with_effective = Holding::builder()
            .id("CALLABLE")
            .identifiers(BondIdentifiers::new().with_ticker("CALL001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(
                HoldingAnalytics::new()
                    .with_modified_duration(5.0)
                    .with_effective_duration(4.5),
            )
            .build()
            .unwrap();

        let holding_without = Holding::builder()
            .id("BULLET")
            .identifiers(BondIdentifiers::new().with_ticker("BULL001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new().with_modified_duration(6.0))
            .build()
            .unwrap();

        let holdings = vec![holding_with_effective, holding_without];
        let config = AnalyticsConfig::default();
        let best = weighted_best_duration(&holdings, &config).unwrap();

        // Equal MV: (4.5 + 6.0) / 2 = 5.25
        assert!((best - 5.25).abs() < 0.01);
    }

    #[test]
    fn test_risk_metrics() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(100), 5.0, 0.05, 50.0),
            create_holding("BOND2", dec!(1_000_000), dec!(100), 7.0, 0.07, 70.0),
        ];

        let config = AnalyticsConfig::default();
        let metrics = calculate_risk_metrics(&holdings, Some(dec!(1_000_000)), &config);

        assert!(metrics.modified_duration.is_some());
        assert!(metrics.convexity.is_some());
        // Bond1: 0.05 × 1M / 100 = 500, Bond2: 0.07 × 1M / 100 = 700, Total: 1200
        assert!((metrics.total_dv01 - 1200.0).abs() < 0.1);
        assert_eq!(metrics.duration_coverage, 2);
        assert_eq!(metrics.dv01_coverage, 2);
        assert!((metrics.duration_coverage_pct() - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_empty_portfolio() {
        let holdings: Vec<Holding> = vec![];
        let config = AnalyticsConfig::default();

        assert!(weighted_modified_duration(&holdings, &config).is_none());
        assert!((total_dv01(&holdings, &config)).abs() < 0.001);
    }

    #[test]
    fn test_no_duration_data() {
        let holding = Holding::builder()
            .id("NODUR")
            .identifiers(BondIdentifiers::new().with_ticker("NODUR001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new()) // No duration
            .build()
            .unwrap();

        let holdings = vec![holding];
        let config = AnalyticsConfig::default();

        assert!(weighted_modified_duration(&holdings, &config).is_none());
    }

    #[test]
    fn test_par_value_weighting() {
        let holdings = vec![
            create_holding("BOND1", dec!(1_000_000), dec!(90), 5.0, 0.05, 50.0), // Par 1M, MV 900k
            create_holding("BOND2", dec!(500_000), dec!(110), 7.0, 0.07, 70.0), // Par 500k, MV 550k
        ];

        let config = AnalyticsConfig::default().with_weighting(WeightingMethod::ParValue);
        let duration = weighted_modified_duration(&holdings, &config).unwrap();

        // Par weighted: (1M × 5 + 0.5M × 7) / 1.5M = 5.67
        assert!((duration - 5.67).abs() < 0.01);
    }
}
