//! Active weights and tracking error analysis.
//!
//! Provides tools for analyzing portfolio positioning relative to a benchmark.

use crate::bucketing::{bucket_by_rating, bucket_by_sector};
use crate::types::{AnalyticsConfig, Holding, RatingBucket, Sector};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Active weight for a single holding or bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveWeight {
    /// Portfolio weight (0-100%).
    pub portfolio_weight: f64,

    /// Benchmark weight (0-100%).
    pub benchmark_weight: f64,

    /// Active weight = portfolio - benchmark (can be negative).
    pub active_weight: f64,

    /// Relative weight = portfolio / benchmark (ratio).
    pub relative_weight: Option<f64>,
}

impl ActiveWeight {
    /// Creates a new active weight.
    #[must_use]
    pub fn new(portfolio_weight: f64, benchmark_weight: f64) -> Self {
        let active_weight = portfolio_weight - benchmark_weight;
        let relative_weight = if benchmark_weight > 0.0 {
            Some(portfolio_weight / benchmark_weight)
        } else {
            None
        };

        Self {
            portfolio_weight,
            benchmark_weight,
            active_weight,
            relative_weight,
        }
    }

    /// Returns true if this is an overweight position.
    #[must_use]
    pub fn is_overweight(&self) -> bool {
        self.active_weight > 0.0
    }

    /// Returns true if this is an underweight position.
    #[must_use]
    pub fn is_underweight(&self) -> bool {
        self.active_weight < 0.0
    }
}

/// Active weights breakdown by various dimensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveWeights {
    /// Active weights by sector.
    pub by_sector: HashMap<Sector, ActiveWeight>,

    /// Active weights by rating bucket.
    pub by_rating: HashMap<RatingBucket, ActiveWeight>,

    /// Active weights by holding ID (for holdings in either portfolio or benchmark).
    pub by_holding: HashMap<String, ActiveWeight>,

    /// Sum of absolute active weights (measure of active risk).
    pub total_active_weight: f64,

    /// Number of overweight positions.
    pub overweight_count: usize,

    /// Number of underweight positions.
    pub underweight_count: usize,
}

impl ActiveWeights {
    /// Returns sectors where portfolio is overweight.
    #[must_use]
    pub fn overweight_sectors(&self) -> Vec<(Sector, f64)> {
        self.by_sector
            .iter()
            .filter(|(_, w)| w.is_overweight())
            .map(|(s, w)| (*s, w.active_weight))
            .collect()
    }

    /// Returns sectors where portfolio is underweight.
    #[must_use]
    pub fn underweight_sectors(&self) -> Vec<(Sector, f64)> {
        self.by_sector
            .iter()
            .filter(|(_, w)| w.is_underweight())
            .map(|(s, w)| (*s, w.active_weight))
            .collect()
    }

    /// Returns the largest active positions by absolute weight.
    #[must_use]
    pub fn largest_active_positions(&self, n: usize) -> Vec<(&str, f64)> {
        let mut positions: Vec<_> = self
            .by_holding
            .iter()
            .map(|(id, w)| (id.as_str(), w.active_weight))
            .collect();

        positions.sort_by(|a, b| {
            b.1.abs()
                .partial_cmp(&a.1.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        positions.into_iter().take(n).collect()
    }
}

/// Tracking error estimation using factor-based approach.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackingErrorEstimate {
    /// Estimated annualized tracking error (%).
    pub tracking_error: f64,

    /// Contribution from duration differences.
    pub duration_contribution: f64,

    /// Contribution from spread/credit differences.
    pub spread_contribution: f64,

    /// Contribution from sector allocation.
    pub sector_contribution: f64,

    /// Contribution from security selection (residual).
    pub selection_contribution: f64,

    /// Active duration exposure.
    pub active_duration: f64,

    /// Active spread exposure (bps).
    pub active_spread: f64,
}

/// Calculates active weights between portfolio and benchmark.
///
/// # Arguments
///
/// * `portfolio` - Portfolio holdings
/// * `benchmark` - Benchmark holdings
/// * `config` - Analytics configuration
///
/// # Returns
///
/// Active weights breakdown by sector, rating, and holding.
///
/// # Example
///
/// ```rust,ignore
/// use convex_portfolio::benchmark::active_weights;
///
/// let weights = active_weights(&portfolio.holdings, &benchmark.holdings, &config);
///
/// for (sector, weight) in weights.overweight_sectors() {
///     println!("{:?}: +{:.2}%", sector, weight);
/// }
/// ```
#[must_use]
pub fn active_weights(
    portfolio: &[Holding],
    benchmark: &[Holding],
    config: &AnalyticsConfig,
) -> ActiveWeights {
    // Calculate sector weights
    let port_sector = bucket_by_sector(portfolio, config);
    let bench_sector = bucket_by_sector(benchmark, config);

    let mut by_sector = HashMap::new();
    let mut all_sectors: std::collections::HashSet<Sector> =
        port_sector.by_sector.keys().copied().collect();
    all_sectors.extend(bench_sector.by_sector.keys().copied());

    for sector in all_sectors {
        let port_wt = port_sector.get(sector).map(|m| m.weight_pct).unwrap_or(0.0);
        let bench_wt = bench_sector
            .get(sector)
            .map(|m| m.weight_pct)
            .unwrap_or(0.0);

        by_sector.insert(sector, ActiveWeight::new(port_wt, bench_wt));
    }

    // Calculate rating weights
    let port_rating = bucket_by_rating(portfolio, config);
    let bench_rating = bucket_by_rating(benchmark, config);

    let mut by_rating = HashMap::new();
    let mut all_ratings: std::collections::HashSet<RatingBucket> =
        port_rating.by_bucket.keys().copied().collect();
    all_ratings.extend(bench_rating.by_bucket.keys().copied());

    for rating in all_ratings {
        let port_wt = port_rating
            .get_bucket(rating)
            .map(|m| m.weight_pct)
            .unwrap_or(0.0);
        let bench_wt = bench_rating
            .get_bucket(rating)
            .map(|m| m.weight_pct)
            .unwrap_or(0.0);

        by_rating.insert(rating, ActiveWeight::new(port_wt, bench_wt));
    }

    // Calculate holding weights
    let port_mv: Decimal = portfolio.iter().map(|h| h.market_value()).sum();
    let bench_mv: Decimal = benchmark.iter().map(|h| h.market_value()).sum();

    let port_mv_f: f64 = port_mv.try_into().unwrap_or(1.0);
    let bench_mv_f: f64 = bench_mv.try_into().unwrap_or(1.0);

    let mut by_holding: HashMap<String, ActiveWeight> = HashMap::new();

    // Add portfolio holdings
    for h in portfolio {
        let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
        let port_wt = (mv / port_mv_f) * 100.0;

        by_holding.insert(h.id.clone(), ActiveWeight::new(port_wt, 0.0));
    }

    // Update with benchmark holdings
    for h in benchmark {
        let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
        let bench_wt = (mv / bench_mv_f) * 100.0;

        let entry = by_holding
            .entry(h.id.clone())
            .or_insert_with(|| ActiveWeight::new(0.0, 0.0));

        // Update benchmark weight and recalculate
        *entry = ActiveWeight::new(entry.portfolio_weight, bench_wt);
    }

    // Calculate summary statistics
    let total_active_weight: f64 = by_holding.values().map(|w| w.active_weight.abs()).sum();

    let overweight_count = by_holding.values().filter(|w| w.is_overweight()).count();
    let underweight_count = by_holding.values().filter(|w| w.is_underweight()).count();

    ActiveWeights {
        by_sector,
        by_rating,
        by_holding,
        total_active_weight,
        overweight_count,
        underweight_count,
    }
}

/// Estimates tracking error using a simplified factor model.
///
/// Uses duration, spread, and sector exposures to estimate tracking error.
/// This is a simplified model; actual tracking error would require
/// historical return data and covariance matrices.
///
/// # Arguments
///
/// * `portfolio` - Portfolio holdings
/// * `benchmark` - Benchmark holdings
/// * `config` - Analytics configuration
/// * `rate_vol` - Assumed interest rate volatility (annualized, e.g., 0.01 for 100bp)
/// * `spread_vol` - Assumed spread volatility (annualized, e.g., 0.002 for 20bp)
///
/// # Returns
///
/// Estimated tracking error breakdown.
///
/// # Example
///
/// ```rust,ignore
/// use convex_portfolio::benchmark::estimate_tracking_error;
///
/// // Assume 100bp rate vol and 20bp spread vol
/// let te = estimate_tracking_error(
///     &portfolio.holdings,
///     &benchmark.holdings,
///     &config,
///     0.01,  // 100bp rate vol
///     0.002, // 20bp spread vol
/// );
///
/// println!("Estimated tracking error: {:.2}%", te.tracking_error);
/// ```
#[must_use]
pub fn estimate_tracking_error(
    portfolio: &[Holding],
    benchmark: &[Holding],
    config: &AnalyticsConfig,
    rate_vol: f64,
    spread_vol: f64,
) -> TrackingErrorEstimate {
    // Calculate weighted average durations
    let port_duration = calculate_weighted_duration(portfolio, config);
    let bench_duration = calculate_weighted_duration(benchmark, config);
    let active_duration = port_duration - bench_duration;

    // Calculate weighted average spreads
    let port_spread = calculate_weighted_spread(portfolio, config);
    let bench_spread = calculate_weighted_spread(benchmark, config);
    let active_spread = port_spread - bench_spread;

    // Duration contribution to TE: |active_duration| × rate_vol
    let duration_contribution = active_duration.abs() * rate_vol * 100.0;

    // Spread contribution to TE: |active_spread / 10000| × spread_vol / spread_vol
    // Simplified: assume spread duration similar to duration
    let spread_contribution =
        (active_spread.abs() / 10000.0) * port_duration.abs() * spread_vol * 100.0;

    // Sector contribution: simplified based on active sector weights
    let weights = active_weights(portfolio, benchmark, config);
    let sector_active: f64 = weights
        .by_sector
        .values()
        .map(|w| w.active_weight.abs())
        .sum();

    // Assume each 1% active sector weight contributes ~10bp to TE
    let sector_contribution = sector_active * 0.001;

    // Selection contribution (residual)
    let selection_contribution = weights.total_active_weight * 0.0005;

    // Total TE (simplified sum-of-squares)
    let tracking_error = (duration_contribution.powi(2)
        + spread_contribution.powi(2)
        + sector_contribution.powi(2)
        + selection_contribution.powi(2))
    .sqrt();

    TrackingErrorEstimate {
        tracking_error,
        duration_contribution,
        spread_contribution,
        sector_contribution,
        selection_contribution,
        active_duration,
        active_spread,
    }
}

/// Helper to calculate weighted average duration.
fn calculate_weighted_duration(holdings: &[Holding], _config: &AnalyticsConfig) -> f64 {
    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();
    let total_mv_f: f64 = total_mv.try_into().unwrap_or(1.0);

    if total_mv_f <= 0.0 {
        return 0.0;
    }

    let mut sum = 0.0;
    for h in holdings {
        if let Some(dur) = h.analytics.best_duration() {
            let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
            sum += dur * (mv / total_mv_f);
        }
    }

    sum
}

/// Helper to calculate weighted average spread.
fn calculate_weighted_spread(holdings: &[Holding], _config: &AnalyticsConfig) -> f64 {
    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();
    let total_mv_f: f64 = total_mv.try_into().unwrap_or(1.0);

    if total_mv_f <= 0.0 {
        return 0.0;
    }

    let mut sum = 0.0;
    for h in holdings {
        if let Some(spread) = h.analytics.best_spread() {
            let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
            sum += spread * (mv / total_mv_f);
        }
    }

    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        Classification, CreditRating, HoldingAnalytics, HoldingBuilder, RatingInfo, SectorInfo,
    };
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal_macros::dec;

    fn create_test_holding(
        id: &str,
        mv: Decimal,
        duration: f64,
        spread: f64,
        sector: Option<Sector>,
        rating: Option<CreditRating>,
    ) -> Holding {
        let mut classification = Classification::new();
        if let Some(s) = sector {
            classification = classification.with_sector(SectorInfo::from_composite(s));
        }
        if let Some(r) = rating {
            classification = classification.with_rating(RatingInfo::from_composite(r));
        }

        HoldingBuilder::new()
            .id(id)
            .identifiers(BondIdentifiers::from_isin_str("US912828Z229").unwrap())
            .par_amount(dec!(1_000_000))
            .market_price(mv)
            .classification(classification)
            .analytics(
                HoldingAnalytics::new()
                    .with_modified_duration(duration)
                    .with_z_spread(spread),
            )
            .build()
            .unwrap()
    }

    #[test]
    fn test_active_weight() {
        let aw = ActiveWeight::new(10.0, 8.0);

        assert_eq!(aw.portfolio_weight, 10.0);
        assert_eq!(aw.benchmark_weight, 8.0);
        assert_eq!(aw.active_weight, 2.0);
        assert!(aw.is_overweight());
        assert!(!aw.is_underweight());
        assert!((aw.relative_weight.unwrap() - 1.25).abs() < 0.01);
    }

    #[test]
    fn test_active_weight_underweight() {
        let aw = ActiveWeight::new(5.0, 10.0);

        assert_eq!(aw.active_weight, -5.0);
        assert!(!aw.is_overweight());
        assert!(aw.is_underweight());
    }

    #[test]
    fn test_active_weights_by_sector() {
        let portfolio = vec![
            create_test_holding("P1", dec!(100), 5.0, 100.0, Some(Sector::Government), None),
            create_test_holding("P2", dec!(100), 5.0, 100.0, Some(Sector::Corporate), None),
        ];

        let benchmark = vec![
            create_test_holding("B1", dec!(150), 5.0, 100.0, Some(Sector::Government), None),
            create_test_holding("B2", dec!(50), 5.0, 100.0, Some(Sector::Corporate), None),
        ];

        let config = AnalyticsConfig::default();
        let weights = active_weights(&portfolio, &benchmark, &config);

        // Portfolio: 50% Govt, 50% Corp
        // Benchmark: 75% Govt, 25% Corp
        // Active: -25% Govt, +25% Corp
        let govt = weights.by_sector.get(&Sector::Government).unwrap();
        assert!((govt.active_weight - (-25.0)).abs() < 0.1);

        let corp = weights.by_sector.get(&Sector::Corporate).unwrap();
        assert!((corp.active_weight - 25.0).abs() < 0.1);
    }

    #[test]
    fn test_active_weights_by_rating() {
        let portfolio = vec![
            create_test_holding("P1", dec!(100), 5.0, 100.0, None, Some(CreditRating::AAA)),
            create_test_holding("P2", dec!(100), 5.0, 100.0, None, Some(CreditRating::BBB)),
        ];

        let benchmark = vec![
            create_test_holding("B1", dec!(100), 5.0, 100.0, None, Some(CreditRating::AAA)),
            create_test_holding("B2", dec!(100), 5.0, 100.0, None, Some(CreditRating::A)),
        ];

        let config = AnalyticsConfig::default();
        let weights = active_weights(&portfolio, &benchmark, &config);

        // Portfolio: 50% AAA, 50% BBB
        // Benchmark: 50% AAA, 50% A
        // Active: 0% AAA, -50% A, +50% BBB
        let aaa = weights.by_rating.get(&RatingBucket::AAA).unwrap();
        assert!((aaa.active_weight).abs() < 0.1);

        let a = weights.by_rating.get(&RatingBucket::A).unwrap();
        assert!((a.active_weight - (-50.0)).abs() < 0.1);

        let bbb = weights.by_rating.get(&RatingBucket::BBB).unwrap();
        assert!((bbb.active_weight - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_active_weights_by_holding() {
        let portfolio = vec![
            create_test_holding("H1", dec!(100), 5.0, 100.0, None, None),
            create_test_holding("H2", dec!(100), 5.0, 100.0, None, None),
        ];

        let benchmark = vec![
            create_test_holding("H1", dec!(50), 5.0, 100.0, None, None),
            create_test_holding("H3", dec!(150), 5.0, 100.0, None, None),
        ];

        let config = AnalyticsConfig::default();
        let weights = active_weights(&portfolio, &benchmark, &config);

        // H1: Portfolio 50%, Benchmark 25% → Active +25%
        let h1 = weights.by_holding.get("H1").unwrap();
        assert!((h1.active_weight - 25.0).abs() < 0.1);

        // H2: Portfolio 50%, Benchmark 0% → Active +50%
        let h2 = weights.by_holding.get("H2").unwrap();
        assert!((h2.active_weight - 50.0).abs() < 0.1);

        // H3: Portfolio 0%, Benchmark 75% → Active -75%
        let h3 = weights.by_holding.get("H3").unwrap();
        assert!((h3.active_weight - (-75.0)).abs() < 0.1);
    }

    #[test]
    fn test_overweight_underweight_counts() {
        let portfolio = vec![
            create_test_holding("H1", dec!(100), 5.0, 100.0, None, None),
            create_test_holding("H2", dec!(100), 5.0, 100.0, None, None),
        ];

        let benchmark = vec![
            create_test_holding("H1", dec!(50), 5.0, 100.0, None, None),
            create_test_holding("H3", dec!(150), 5.0, 100.0, None, None),
        ];

        let config = AnalyticsConfig::default();
        let weights = active_weights(&portfolio, &benchmark, &config);

        // H1: overweight, H2: overweight (not in benchmark), H3: underweight (not in portfolio)
        assert_eq!(weights.overweight_count, 2);
        assert_eq!(weights.underweight_count, 1);
    }

    #[test]
    fn test_largest_active_positions() {
        let portfolio = vec![
            create_test_holding("H1", dec!(100), 5.0, 100.0, None, None),
            create_test_holding("H2", dec!(50), 5.0, 100.0, None, None),
        ];

        let benchmark = vec![
            create_test_holding("H1", dec!(50), 5.0, 100.0, None, None),
            create_test_holding("H3", dec!(100), 5.0, 100.0, None, None),
        ];

        let config = AnalyticsConfig::default();
        let weights = active_weights(&portfolio, &benchmark, &config);
        let largest = weights.largest_active_positions(2);

        assert_eq!(largest.len(), 2);
        // H3 has largest active weight (-66.67%)
        assert_eq!(largest[0].0, "H3");
    }

    #[test]
    fn test_estimate_tracking_error() {
        let portfolio = vec![create_test_holding(
            "P1",
            dec!(100),
            6.0,
            150.0,
            Some(Sector::Corporate),
            None,
        )];

        let benchmark = vec![create_test_holding(
            "B1",
            dec!(100),
            5.0,
            100.0,
            Some(Sector::Government),
            None,
        )];

        let config = AnalyticsConfig::default();
        let te = estimate_tracking_error(&portfolio, &benchmark, &config, 0.01, 0.002);

        // Active duration = 6 - 5 = 1
        assert!((te.active_duration - 1.0).abs() < 0.01);

        // Active spread = 150 - 100 = 50bp
        assert!((te.active_spread - 50.0).abs() < 0.01);

        // TE should be positive
        assert!(te.tracking_error > 0.0);
        assert!(te.duration_contribution > 0.0);
    }

    #[test]
    fn test_tracking_error_identical() {
        let holdings = vec![create_test_holding(
            "H1",
            dec!(100),
            5.0,
            100.0,
            Some(Sector::Government),
            None,
        )];

        let config = AnalyticsConfig::default();
        let te = estimate_tracking_error(&holdings, &holdings, &config, 0.01, 0.002);

        // Identical portfolios should have zero active exposure
        assert!((te.active_duration).abs() < 0.01);
        assert!((te.active_spread).abs() < 0.01);
    }

    #[test]
    fn test_overweight_underweight_sectors() {
        let portfolio = vec![
            create_test_holding("P1", dec!(100), 5.0, 100.0, Some(Sector::Government), None),
            create_test_holding("P2", dec!(100), 5.0, 100.0, Some(Sector::Corporate), None),
        ];

        let benchmark = vec![
            create_test_holding("B1", dec!(150), 5.0, 100.0, Some(Sector::Government), None),
            create_test_holding("B2", dec!(50), 5.0, 100.0, Some(Sector::Corporate), None),
        ];

        let config = AnalyticsConfig::default();
        let weights = active_weights(&portfolio, &benchmark, &config);

        let overweight = weights.overweight_sectors();
        let underweight = weights.underweight_sectors();

        assert_eq!(overweight.len(), 1);
        assert_eq!(overweight[0].0, Sector::Corporate);

        assert_eq!(underweight.len(), 1);
        assert_eq!(underweight[0].0, Sector::Government);
    }
}
