//! Benchmark comparison analytics.
//!
//! Provides duration, spread, and other metric comparisons between
//! a portfolio and its benchmark.

use super::tracking::{active_weights, ActiveWeights};
use crate::analytics::{calculate_risk_metrics, calculate_spread_metrics, calculate_yield_metrics};
use crate::bucketing::{bucket_by_rating, bucket_by_sector};
use crate::types::{AnalyticsConfig, Holding, RatingBucket, Sector};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Comprehensive benchmark comparison results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkComparison {
    /// Active weights breakdown.
    pub active_weights: ActiveWeights,

    /// Duration comparison.
    pub duration: DurationComparison,

    /// Spread comparison.
    pub spread: SpreadComparison,

    /// Yield comparison.
    pub yield_comparison: YieldComparison,

    /// Risk comparison.
    pub risk: RiskComparison,

    /// Sector-level comparison.
    pub by_sector: HashMap<Sector, SectorComparison>,

    /// Rating-level comparison.
    pub by_rating: HashMap<RatingBucket, RatingComparison>,
}

/// Duration comparison between portfolio and benchmark.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DurationComparison {
    /// Portfolio weighted average duration.
    pub portfolio_duration: Option<f64>,

    /// Benchmark weighted average duration.
    pub benchmark_duration: Option<f64>,

    /// Duration difference (portfolio - benchmark).
    pub difference: Option<f64>,

    /// Duration ratio (portfolio / benchmark).
    pub ratio: Option<f64>,

    /// Portfolio effective duration (if available).
    pub portfolio_effective: Option<f64>,

    /// Benchmark effective duration (if available).
    pub benchmark_effective: Option<f64>,
}

impl DurationComparison {
    /// Returns true if portfolio is longer duration than benchmark.
    #[must_use]
    pub fn is_longer(&self) -> bool {
        self.difference.map(|d| d > 0.0).unwrap_or(false)
    }

    /// Returns true if portfolio is shorter duration than benchmark.
    #[must_use]
    pub fn is_shorter(&self) -> bool {
        self.difference.map(|d| d < 0.0).unwrap_or(false)
    }
}

/// Spread comparison between portfolio and benchmark.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpreadComparison {
    /// Portfolio weighted average spread (bps).
    pub portfolio_spread: Option<f64>,

    /// Benchmark weighted average spread (bps).
    pub benchmark_spread: Option<f64>,

    /// Spread difference in bps (portfolio - benchmark).
    pub difference: Option<f64>,

    /// Spread ratio (portfolio / benchmark).
    pub ratio: Option<f64>,

    /// Portfolio OAS (if available).
    pub portfolio_oas: Option<f64>,

    /// Benchmark OAS (if available).
    pub benchmark_oas: Option<f64>,
}

impl SpreadComparison {
    /// Returns true if portfolio has wider spread than benchmark.
    #[must_use]
    pub fn is_wider(&self) -> bool {
        self.difference.map(|d| d > 0.0).unwrap_or(false)
    }

    /// Returns true if portfolio has tighter spread than benchmark.
    #[must_use]
    pub fn is_tighter(&self) -> bool {
        self.difference.map(|d| d < 0.0).unwrap_or(false)
    }
}

/// Yield comparison between portfolio and benchmark.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YieldComparison {
    /// Portfolio weighted YTM.
    pub portfolio_ytm: Option<f64>,

    /// Benchmark weighted YTM.
    pub benchmark_ytm: Option<f64>,

    /// YTM difference (portfolio - benchmark).
    pub ytm_difference: Option<f64>,

    /// Portfolio weighted YTW.
    pub portfolio_ytw: Option<f64>,

    /// Benchmark weighted YTW.
    pub benchmark_ytw: Option<f64>,

    /// YTW difference (portfolio - benchmark).
    pub ytw_difference: Option<f64>,
}

impl YieldComparison {
    /// Returns true if portfolio has higher yield than benchmark.
    #[must_use]
    pub fn is_higher_yield(&self) -> bool {
        self.ytm_difference.map(|d| d > 0.0).unwrap_or(false)
    }
}

/// Risk comparison between portfolio and benchmark.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskComparison {
    /// Portfolio total DV01.
    pub portfolio_dv01: f64,

    /// Benchmark total DV01.
    pub benchmark_dv01: f64,

    /// DV01 difference.
    pub dv01_difference: f64,

    /// DV01 ratio (portfolio / benchmark).
    pub dv01_ratio: Option<f64>,

    /// Portfolio convexity.
    pub portfolio_convexity: Option<f64>,

    /// Benchmark convexity.
    pub benchmark_convexity: Option<f64>,
}

/// Sector-level comparison.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SectorComparison {
    /// Portfolio weight in this sector (%).
    pub portfolio_weight: f64,

    /// Benchmark weight in this sector (%).
    pub benchmark_weight: f64,

    /// Active weight (portfolio - benchmark).
    pub active_weight: f64,

    /// Portfolio duration in this sector.
    pub portfolio_duration: Option<f64>,

    /// Benchmark duration in this sector.
    pub benchmark_duration: Option<f64>,

    /// Portfolio spread in this sector (bps).
    pub portfolio_spread: Option<f64>,

    /// Benchmark spread in this sector (bps).
    pub benchmark_spread: Option<f64>,
}

/// Rating-level comparison.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RatingComparison {
    /// Portfolio weight in this rating bucket (%).
    pub portfolio_weight: f64,

    /// Benchmark weight in this rating bucket (%).
    pub benchmark_weight: f64,

    /// Active weight (portfolio - benchmark).
    pub active_weight: f64,

    /// Portfolio duration in this bucket.
    pub portfolio_duration: Option<f64>,

    /// Benchmark duration in this bucket.
    pub benchmark_duration: Option<f64>,

    /// Portfolio spread in this bucket (bps).
    pub portfolio_spread: Option<f64>,

    /// Benchmark spread in this bucket (bps).
    pub benchmark_spread: Option<f64>,
}

/// Performs comprehensive benchmark comparison.
///
/// # Arguments
///
/// * `portfolio` - Portfolio holdings
/// * `benchmark` - Benchmark holdings
/// * `config` - Analytics configuration
///
/// # Returns
///
/// Comprehensive comparison including duration, spread, yield, and risk metrics.
///
/// # Example
///
/// ```rust,ignore
/// use convex_portfolio::benchmark::benchmark_comparison;
///
/// let comparison = benchmark_comparison(&portfolio.holdings, &benchmark.holdings, &config);
///
/// if comparison.duration.is_longer() {
///     println!("Portfolio is {:.2} years longer than benchmark",
///         comparison.duration.difference.unwrap());
/// }
///
/// if comparison.spread.is_wider() {
///     println!("Portfolio is {:.0}bp wider than benchmark",
///         comparison.spread.difference.unwrap());
/// }
/// ```
#[must_use]
pub fn benchmark_comparison(
    portfolio: &[Holding],
    benchmark: &[Holding],
    config: &AnalyticsConfig,
) -> BenchmarkComparison {
    // Calculate active weights
    let active_wts = active_weights(portfolio, benchmark, config);

    // Calculate metrics for both (no shares outstanding for benchmark comparison)
    let port_risk = calculate_risk_metrics(portfolio, None, config);
    let bench_risk = calculate_risk_metrics(benchmark, None, config);

    let port_spread = calculate_spread_metrics(portfolio, None, config);
    let bench_spread = calculate_spread_metrics(benchmark, None, config);

    let port_yield = calculate_yield_metrics(portfolio, config);
    let bench_yield = calculate_yield_metrics(benchmark, config);

    // Duration comparison
    let duration = DurationComparison {
        portfolio_duration: port_risk.best_duration,
        benchmark_duration: bench_risk.best_duration,
        difference: match (port_risk.best_duration, bench_risk.best_duration) {
            (Some(p), Some(b)) => Some(p - b),
            _ => None,
        },
        ratio: match (port_risk.best_duration, bench_risk.best_duration) {
            (Some(p), Some(b)) if b > 0.0 => Some(p / b),
            _ => None,
        },
        portfolio_effective: port_risk.effective_duration,
        benchmark_effective: bench_risk.effective_duration,
    };

    // Spread comparison
    let spread = SpreadComparison {
        portfolio_spread: port_spread.z_spread,
        benchmark_spread: bench_spread.z_spread,
        difference: match (port_spread.z_spread, bench_spread.z_spread) {
            (Some(p), Some(b)) => Some(p - b),
            _ => None,
        },
        ratio: match (port_spread.z_spread, bench_spread.z_spread) {
            (Some(p), Some(b)) if b > 0.0 => Some(p / b),
            _ => None,
        },
        portfolio_oas: port_spread.oas,
        benchmark_oas: bench_spread.oas,
    };

    // Yield comparison
    let yield_comparison = YieldComparison {
        portfolio_ytm: port_yield.ytm,
        benchmark_ytm: bench_yield.ytm,
        ytm_difference: match (port_yield.ytm, bench_yield.ytm) {
            (Some(p), Some(b)) => Some(p - b),
            _ => None,
        },
        portfolio_ytw: port_yield.ytw,
        benchmark_ytw: bench_yield.ytw,
        ytw_difference: match (port_yield.ytw, bench_yield.ytw) {
            (Some(p), Some(b)) => Some(p - b),
            _ => None,
        },
    };

    // Risk comparison (total_dv01 is f64, not Option)
    let risk = RiskComparison {
        portfolio_dv01: port_risk.total_dv01,
        benchmark_dv01: bench_risk.total_dv01,
        dv01_difference: port_risk.total_dv01 - bench_risk.total_dv01,
        dv01_ratio: if bench_risk.total_dv01 > 0.0 {
            Some(port_risk.total_dv01 / bench_risk.total_dv01)
        } else {
            None
        },
        portfolio_convexity: port_risk.convexity,
        benchmark_convexity: bench_risk.convexity,
    };

    // Sector-level comparison
    let by_sector = calculate_sector_comparison(portfolio, benchmark, config);

    // Rating-level comparison
    let by_rating = calculate_rating_comparison(portfolio, benchmark, config);

    BenchmarkComparison {
        active_weights: active_wts,
        duration,
        spread,
        yield_comparison,
        risk,
        by_sector,
        by_rating,
    }
}

/// Calculates sector-level comparison.
fn calculate_sector_comparison(
    portfolio: &[Holding],
    benchmark: &[Holding],
    config: &AnalyticsConfig,
) -> HashMap<Sector, SectorComparison> {
    let port_sector = bucket_by_sector(portfolio, config);
    let bench_sector = bucket_by_sector(benchmark, config);

    let mut result = HashMap::new();

    // Collect all sectors
    let mut all_sectors: std::collections::HashSet<Sector> =
        port_sector.by_sector.keys().copied().collect();
    all_sectors.extend(bench_sector.by_sector.keys().copied());

    for sector in all_sectors {
        let port_metrics = port_sector.get(sector);
        let bench_metrics = bench_sector.get(sector);

        let port_weight = port_metrics.map(|m| m.weight_pct).unwrap_or(0.0);
        let bench_weight = bench_metrics.map(|m| m.weight_pct).unwrap_or(0.0);

        result.insert(
            sector,
            SectorComparison {
                portfolio_weight: port_weight,
                benchmark_weight: bench_weight,
                active_weight: port_weight - bench_weight,
                portfolio_duration: port_metrics.and_then(|m| m.avg_duration),
                benchmark_duration: bench_metrics.and_then(|m| m.avg_duration),
                portfolio_spread: port_metrics.and_then(|m| m.avg_spread),
                benchmark_spread: bench_metrics.and_then(|m| m.avg_spread),
            },
        );
    }

    result
}

/// Calculates rating-level comparison.
fn calculate_rating_comparison(
    portfolio: &[Holding],
    benchmark: &[Holding],
    config: &AnalyticsConfig,
) -> HashMap<RatingBucket, RatingComparison> {
    let port_rating = bucket_by_rating(portfolio, config);
    let bench_rating = bucket_by_rating(benchmark, config);

    let mut result = HashMap::new();

    // Collect all rating buckets
    let mut all_ratings: std::collections::HashSet<RatingBucket> =
        port_rating.by_bucket.keys().copied().collect();
    all_ratings.extend(bench_rating.by_bucket.keys().copied());

    for rating in all_ratings {
        let port_metrics = port_rating.get_bucket(rating);
        let bench_metrics = bench_rating.get_bucket(rating);

        let port_weight = port_metrics.map(|m| m.weight_pct).unwrap_or(0.0);
        let bench_weight = bench_metrics.map(|m| m.weight_pct).unwrap_or(0.0);

        result.insert(
            rating,
            RatingComparison {
                portfolio_weight: port_weight,
                benchmark_weight: bench_weight,
                active_weight: port_weight - bench_weight,
                portfolio_duration: port_metrics.and_then(|m| m.avg_duration),
                benchmark_duration: bench_metrics.and_then(|m| m.avg_duration),
                portfolio_spread: port_metrics.and_then(|m| m.avg_spread),
                benchmark_spread: bench_metrics.and_then(|m| m.avg_spread),
            },
        );
    }

    result
}

/// Calculates duration difference by sector.
///
/// Returns the contribution to duration difference from each sector.
#[must_use]
pub fn duration_difference_by_sector(
    portfolio: &[Holding],
    benchmark: &[Holding],
    config: &AnalyticsConfig,
) -> HashMap<Sector, f64> {
    let comparison = calculate_sector_comparison(portfolio, benchmark, config);

    comparison
        .into_iter()
        .filter_map(|(sector, comp)| {
            match (comp.portfolio_duration, comp.benchmark_duration) {
                (Some(p), Some(b)) => {
                    // Contribution = active_weight × duration + weight × duration_diff
                    let allocation = (comp.active_weight / 100.0) * b;
                    let selection = (comp.portfolio_weight / 100.0) * (p - b);
                    Some((sector, allocation + selection))
                }
                _ => None,
            }
        })
        .collect()
}

/// Calculates spread difference by sector.
///
/// Returns the contribution to spread difference from each sector.
#[must_use]
pub fn spread_difference_by_sector(
    portfolio: &[Holding],
    benchmark: &[Holding],
    config: &AnalyticsConfig,
) -> HashMap<Sector, f64> {
    let comparison = calculate_sector_comparison(portfolio, benchmark, config);

    comparison
        .into_iter()
        .filter_map(|(sector, comp)| {
            match (comp.portfolio_spread, comp.benchmark_spread) {
                (Some(p), Some(b)) => {
                    // Contribution = active_weight × spread + weight × spread_diff
                    let allocation = (comp.active_weight / 100.0) * b;
                    let selection = (comp.portfolio_weight / 100.0) * (p - b);
                    Some((sector, allocation + selection))
                }
                _ => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        Classification, CreditRating, HoldingAnalytics, HoldingBuilder, RatingInfo, SectorInfo,
    };
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn create_test_holding(
        id: &str,
        mv: Decimal,
        duration: f64,
        spread: f64,
        ytm: f64,
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
                    .with_z_spread(spread)
                    .with_ytm(ytm)
                    .with_dv01(duration / 100.0),
            )
            .build()
            .unwrap()
    }

    #[test]
    fn test_benchmark_comparison_duration() {
        let portfolio = vec![create_test_holding(
            "P1",
            dec!(100),
            6.0,
            150.0,
            0.05,
            None,
            None,
        )];

        let benchmark = vec![create_test_holding(
            "B1",
            dec!(100),
            5.0,
            100.0,
            0.04,
            None,
            None,
        )];

        let config = AnalyticsConfig::default();
        let comparison = benchmark_comparison(&portfolio, &benchmark, &config);

        // Duration difference = 6 - 5 = 1
        assert!((comparison.duration.difference.unwrap() - 1.0).abs() < 0.01);
        assert!(comparison.duration.is_longer());
    }

    #[test]
    fn test_benchmark_comparison_spread() {
        let portfolio = vec![create_test_holding(
            "P1",
            dec!(100),
            5.0,
            150.0,
            0.05,
            None,
            None,
        )];

        let benchmark = vec![create_test_holding(
            "B1",
            dec!(100),
            5.0,
            100.0,
            0.04,
            None,
            None,
        )];

        let config = AnalyticsConfig::default();
        let comparison = benchmark_comparison(&portfolio, &benchmark, &config);

        // Spread difference = 150 - 100 = 50bp
        assert!((comparison.spread.difference.unwrap() - 50.0).abs() < 0.01);
        assert!(comparison.spread.is_wider());
    }

    #[test]
    fn test_benchmark_comparison_yield() {
        let portfolio = vec![create_test_holding(
            "P1",
            dec!(100),
            5.0,
            100.0,
            0.055,
            None,
            None,
        )];

        let benchmark = vec![create_test_holding(
            "B1",
            dec!(100),
            5.0,
            100.0,
            0.045,
            None,
            None,
        )];

        let config = AnalyticsConfig::default();
        let comparison = benchmark_comparison(&portfolio, &benchmark, &config);

        // YTM difference = 5.5% - 4.5% = 1%
        assert!((comparison.yield_comparison.ytm_difference.unwrap() - 0.01).abs() < 0.001);
        assert!(comparison.yield_comparison.is_higher_yield());
    }

    #[test]
    fn test_benchmark_comparison_by_sector() {
        let portfolio = vec![
            create_test_holding(
                "P1",
                dec!(100),
                5.0,
                100.0,
                0.05,
                Some(Sector::Government),
                None,
            ),
            create_test_holding(
                "P2",
                dec!(100),
                6.0,
                150.0,
                0.06,
                Some(Sector::Corporate),
                None,
            ),
        ];

        let benchmark = vec![
            create_test_holding(
                "B1",
                dec!(150),
                4.0,
                80.0,
                0.04,
                Some(Sector::Government),
                None,
            ),
            create_test_holding(
                "B2",
                dec!(50),
                7.0,
                200.0,
                0.07,
                Some(Sector::Corporate),
                None,
            ),
        ];

        let config = AnalyticsConfig::default();
        let comparison = benchmark_comparison(&portfolio, &benchmark, &config);

        // Government: Portfolio 50%, Benchmark 75% → Active -25%
        let govt = comparison.by_sector.get(&Sector::Government).unwrap();
        assert!((govt.active_weight - (-25.0)).abs() < 0.1);

        // Corporate: Portfolio 50%, Benchmark 25% → Active +25%
        let corp = comparison.by_sector.get(&Sector::Corporate).unwrap();
        assert!((corp.active_weight - 25.0).abs() < 0.1);
    }

    #[test]
    fn test_benchmark_comparison_by_rating() {
        let portfolio = vec![create_test_holding(
            "P1",
            dec!(100),
            5.0,
            100.0,
            0.05,
            None,
            Some(CreditRating::AAA),
        )];

        let benchmark = vec![create_test_holding(
            "B1",
            dec!(100),
            5.0,
            100.0,
            0.05,
            None,
            Some(CreditRating::BBB),
        )];

        let config = AnalyticsConfig::default();
        let comparison = benchmark_comparison(&portfolio, &benchmark, &config);

        // AAA: Portfolio 100%, Benchmark 0% → Active +100%
        let aaa = comparison.by_rating.get(&RatingBucket::AAA).unwrap();
        assert!((aaa.active_weight - 100.0).abs() < 0.1);

        // BBB: Portfolio 0%, Benchmark 100% → Active -100%
        let bbb = comparison.by_rating.get(&RatingBucket::BBB).unwrap();
        assert!((bbb.active_weight - (-100.0)).abs() < 0.1);
    }

    #[test]
    fn test_duration_difference_by_sector() {
        let portfolio = vec![
            create_test_holding(
                "P1",
                dec!(100),
                6.0,
                100.0,
                0.05,
                Some(Sector::Government),
                None,
            ),
            create_test_holding(
                "P2",
                dec!(100),
                4.0,
                100.0,
                0.05,
                Some(Sector::Corporate),
                None,
            ),
        ];

        let benchmark = vec![
            create_test_holding(
                "B1",
                dec!(100),
                5.0,
                100.0,
                0.05,
                Some(Sector::Government),
                None,
            ),
            create_test_holding(
                "B2",
                dec!(100),
                5.0,
                100.0,
                0.05,
                Some(Sector::Corporate),
                None,
            ),
        ];

        let config = AnalyticsConfig::default();
        let dur_diff = duration_difference_by_sector(&portfolio, &benchmark, &config);

        // Both sectors have 50% weight in both
        // Government: selection = 0.5 × (6 - 5) = 0.5
        assert!(dur_diff.contains_key(&Sector::Government));

        // Corporate: selection = 0.5 × (4 - 5) = -0.5
        assert!(dur_diff.contains_key(&Sector::Corporate));
    }

    #[test]
    fn test_identical_portfolios() {
        let holdings = vec![create_test_holding(
            "H1",
            dec!(100),
            5.0,
            100.0,
            0.05,
            Some(Sector::Government),
            Some(CreditRating::AAA),
        )];

        let config = AnalyticsConfig::default();
        let comparison = benchmark_comparison(&holdings, &holdings, &config);

        // Duration difference should be zero
        assert!((comparison.duration.difference.unwrap()).abs() < 0.01);

        // Spread difference should be zero
        assert!((comparison.spread.difference.unwrap()).abs() < 0.01);

        // All active weights should be zero
        assert_eq!(comparison.active_weights.overweight_count, 0);
        assert_eq!(comparison.active_weights.underweight_count, 0);
    }

    #[test]
    fn test_duration_ratio() {
        let portfolio = vec![create_test_holding(
            "P1",
            dec!(100),
            6.0,
            100.0,
            0.05,
            None,
            None,
        )];

        let benchmark = vec![create_test_holding(
            "B1",
            dec!(100),
            4.0,
            100.0,
            0.05,
            None,
            None,
        )];

        let config = AnalyticsConfig::default();
        let comparison = benchmark_comparison(&portfolio, &benchmark, &config);

        // Duration ratio = 6 / 4 = 1.5
        assert!((comparison.duration.ratio.unwrap() - 1.5).abs() < 0.01);
    }
}
