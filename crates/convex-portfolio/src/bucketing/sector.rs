//! Sector-based portfolio bucketing.
//!
//! Provides distribution analysis by issuer sector.

use crate::types::{AnalyticsConfig, Holding, Sector, WeightingMethod};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Aggregated metrics for a bucket of holdings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BucketMetrics {
    /// Number of holdings in this bucket.
    pub count: usize,

    /// Total market value in base currency.
    pub market_value: Decimal,

    /// Weight as percentage of total (0-100).
    pub weight_pct: f64,

    /// Par value in base currency.
    pub par_value: Decimal,

    /// Weighted average YTM (if available).
    pub avg_ytm: Option<f64>,

    /// Weighted average modified duration (if available).
    pub avg_duration: Option<f64>,

    /// Total DV01 for this bucket.
    pub total_dv01: Option<Decimal>,

    /// Weighted average spread (Z-spread or OAS).
    pub avg_spread: Option<f64>,
}

impl BucketMetrics {
    /// Creates new empty metrics.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if this bucket is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// Distribution of holdings by sector.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SectorDistribution {
    /// Metrics by sector.
    pub by_sector: HashMap<Sector, BucketMetrics>,

    /// Total portfolio market value.
    pub total_market_value: Decimal,

    /// Holdings without sector classification.
    pub unclassified: BucketMetrics,
}

impl SectorDistribution {
    /// Returns the weight of government-related sectors (Government + Agency + Supranational).
    #[must_use]
    pub fn government_weight(&self) -> f64 {
        [Sector::Government, Sector::Agency, Sector::Supranational]
            .iter()
            .filter_map(|s| self.by_sector.get(s))
            .map(|m| m.weight_pct)
            .sum()
    }

    /// Returns the weight of securitized sectors (ABS + MBS + Covered).
    #[must_use]
    pub fn securitized_weight(&self) -> f64 {
        [
            Sector::AssetBacked,
            Sector::MortgageBacked,
            Sector::CoveredBond,
        ]
        .iter()
        .filter_map(|s| self.by_sector.get(s))
        .map(|m| m.weight_pct)
        .sum()
    }

    /// Returns the weight of credit sectors (Corporate + Financial + Utility).
    #[must_use]
    pub fn credit_weight(&self) -> f64 {
        [Sector::Corporate, Sector::Financial, Sector::Utility]
            .iter()
            .filter_map(|s| self.by_sector.get(s))
            .map(|m| m.weight_pct)
            .sum()
    }

    /// Returns metrics for a specific sector.
    #[must_use]
    pub fn get(&self, sector: Sector) -> Option<&BucketMetrics> {
        self.by_sector.get(&sector)
    }

    /// Returns all sectors with their metrics, sorted by weight descending.
    #[must_use]
    pub fn sorted_by_weight(&self) -> Vec<(Sector, &BucketMetrics)> {
        let mut result: Vec<_> = self.by_sector.iter().map(|(s, m)| (*s, m)).collect();
        result.sort_by(|a, b| {
            b.1.weight_pct
                .partial_cmp(&a.1.weight_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        result
    }
}

/// Buckets holdings by sector classification.
///
/// Holdings are grouped by their composite sector. Holdings without
/// a sector classification are placed in the "unclassified" bucket.
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to bucket
/// * `config` - Analytics configuration (controls weighting and parallelism)
///
/// # Returns
///
/// Distribution of holdings by sector with aggregated metrics.
#[must_use]
pub fn bucket_by_sector(holdings: &[Holding], config: &AnalyticsConfig) -> SectorDistribution {
    if holdings.is_empty() {
        return SectorDistribution::default();
    }

    // Calculate total market value for weight percentages
    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();

    if total_mv.is_zero() {
        return SectorDistribution {
            total_market_value: Decimal::ZERO,
            ..Default::default()
        };
    }

    // Group holdings by sector (using indices to avoid lifetime issues)
    let mut grouped: HashMap<Option<Sector>, Vec<usize>> = HashMap::new();
    for (i, h) in holdings.iter().enumerate() {
        let sector = h.classification.sector.composite;
        grouped.entry(sector).or_default().push(i);
    }

    // Aggregate metrics for each group
    let mut by_sector = HashMap::new();
    let mut unclassified = BucketMetrics::default();

    for (sector_opt, indices) in grouped {
        let group: Vec<&Holding> = indices.iter().map(|&i| &holdings[i]).collect();
        let metrics = aggregate_bucket_metrics(&group, total_mv, config);

        match sector_opt {
            Some(sector) => {
                by_sector.insert(sector, metrics);
            }
            None => {
                unclassified = metrics;
            }
        }
    }

    SectorDistribution {
        by_sector,
        total_market_value: total_mv,
        unclassified,
    }
}

/// Aggregates metrics for a bucket of holdings.
pub(crate) fn aggregate_bucket_metrics(
    holdings: &[&Holding],
    total_mv: Decimal,
    config: &AnalyticsConfig,
) -> BucketMetrics {
    if holdings.is_empty() {
        return BucketMetrics::default();
    }

    let count = holdings.len();
    let market_value: Decimal = holdings.iter().map(|h| h.market_value()).sum();
    let par_value: Decimal = holdings.iter().map(|h| h.par_amount * h.fx_rate).sum();

    let weight_pct = if total_mv.is_zero() {
        0.0
    } else {
        let weight = market_value / total_mv * Decimal::ONE_HUNDRED;
        weight.try_into().unwrap_or(0.0)
    };

    // Calculate weighted averages based on weighting method
    let (avg_ytm, avg_duration, avg_spread) =
        calculate_weighted_averages(holdings, config.weighting);

    // Calculate total DV01
    let total_dv01 = calculate_total_dv01(holdings);

    BucketMetrics {
        count,
        market_value,
        weight_pct,
        par_value,
        avg_ytm,
        avg_duration,
        total_dv01,
        avg_spread,
    }
}

/// Calculates weighted averages for YTM, duration, and spread.
fn calculate_weighted_averages(
    holdings: &[&Holding],
    method: WeightingMethod,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    let mut ytm_sum = 0.0;
    let mut ytm_weight = 0.0;
    let mut dur_sum = 0.0;
    let mut dur_weight = 0.0;
    let mut spread_sum = 0.0;
    let mut spread_weight = 0.0;

    for h in holdings {
        let w: f64 = h.weight_value(method).try_into().unwrap_or(0.0);

        if let Some(ytm) = h.analytics.ytm {
            ytm_sum += ytm * w;
            ytm_weight += w;
        }

        if let Some(dur) = h.analytics.best_duration() {
            dur_sum += dur * w;
            dur_weight += w;
        }

        if let Some(spread) = h.analytics.best_spread() {
            spread_sum += spread * w;
            spread_weight += w;
        }
    }

    let avg_ytm = if ytm_weight > 0.0 {
        Some(ytm_sum / ytm_weight)
    } else {
        None
    };

    let avg_duration = if dur_weight > 0.0 {
        Some(dur_sum / dur_weight)
    } else {
        None
    };

    let avg_spread = if spread_weight > 0.0 {
        Some(spread_sum / spread_weight)
    } else {
        None
    };

    (avg_ytm, avg_duration, avg_spread)
}

/// Calculates total DV01 for a bucket.
fn calculate_total_dv01(holdings: &[&Holding]) -> Option<Decimal> {
    let mut total = Decimal::ZERO;
    let mut has_any = false;

    for h in holdings {
        if let Some(dv01) = h.total_dv01() {
            total += dv01;
            has_any = true;
        }
    }

    if has_any {
        Some(total)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Classification, HoldingAnalytics, HoldingBuilder, SectorInfo};
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal_macros::dec;

    fn create_test_holding(id: &str, mv: Decimal, sector: Option<Sector>) -> Holding {
        HoldingBuilder::new()
            .id(id)
            .identifiers(BondIdentifiers::from_isin_str("US912828Z229").unwrap())
            .par_amount(dec!(1_000_000))
            .market_price(mv)
            .classification(Classification::new().with_sector(match sector {
                Some(s) => SectorInfo::from_composite(s),
                None => SectorInfo::new(),
            }))
            .analytics(
                HoldingAnalytics::new()
                    .with_ytm(0.05)
                    .with_modified_duration(5.0)
                    .with_z_spread(100.0)
                    .with_dv01(0.05),
            )
            .build()
            .unwrap()
    }

    #[test]
    fn test_bucket_by_sector_empty() {
        let holdings: Vec<Holding> = vec![];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_sector(&holdings, &config);

        assert!(dist.by_sector.is_empty());
        assert!(dist.total_market_value.is_zero());
    }

    #[test]
    fn test_bucket_by_sector_single() {
        let holdings = vec![create_test_holding(
            "H1",
            dec!(100),
            Some(Sector::Government),
        )];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_sector(&holdings, &config);

        assert_eq!(dist.by_sector.len(), 1);
        let govt = dist.get(Sector::Government).unwrap();
        assert_eq!(govt.count, 1);
        assert!((govt.weight_pct - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_bucket_by_sector_multiple() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(Sector::Government)),
            create_test_holding("H2", dec!(100), Some(Sector::Government)),
            create_test_holding("H3", dec!(100), Some(Sector::Corporate)),
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_sector(&holdings, &config);

        assert_eq!(dist.by_sector.len(), 2);

        let govt = dist.get(Sector::Government).unwrap();
        assert_eq!(govt.count, 2);
        assert!((govt.weight_pct - 66.67).abs() < 0.1);

        let corp = dist.get(Sector::Corporate).unwrap();
        assert_eq!(corp.count, 1);
        assert!((corp.weight_pct - 33.33).abs() < 0.1);
    }

    #[test]
    fn test_bucket_by_sector_unclassified() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(Sector::Government)),
            create_test_holding("H2", dec!(100), None), // No sector
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_sector(&holdings, &config);

        assert_eq!(dist.by_sector.len(), 1);
        assert_eq!(dist.unclassified.count, 1);
        assert!((dist.unclassified.weight_pct - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_government_weight() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(Sector::Government)),
            create_test_holding("H2", dec!(100), Some(Sector::Agency)),
            create_test_holding("H3", dec!(100), Some(Sector::Corporate)),
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_sector(&holdings, &config);

        // Government + Agency = 66.67%
        assert!((dist.government_weight() - 66.67).abs() < 0.1);
    }

    #[test]
    fn test_credit_weight() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(Sector::Corporate)),
            create_test_holding("H2", dec!(100), Some(Sector::Financial)),
            create_test_holding("H3", dec!(100), Some(Sector::Government)),
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_sector(&holdings, &config);

        // Corporate + Financial = 66.67%
        assert!((dist.credit_weight() - 66.67).abs() < 0.1);
    }

    #[test]
    fn test_sorted_by_weight() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(Sector::Corporate)),
            create_test_holding("H2", dec!(200), Some(Sector::Government)),
            create_test_holding("H3", dec!(50), Some(Sector::Financial)),
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_sector(&holdings, &config);
        let sorted = dist.sorted_by_weight();

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].0, Sector::Government); // Highest weight
        assert_eq!(sorted[1].0, Sector::Corporate);
        assert_eq!(sorted[2].0, Sector::Financial); // Lowest weight
    }

    #[test]
    fn test_bucket_metrics_aggregation() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(Sector::Government)),
            create_test_holding("H2", dec!(100), Some(Sector::Government)),
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_sector(&holdings, &config);
        let govt = dist.get(Sector::Government).unwrap();

        // Should have aggregated metrics
        assert!(govt.avg_ytm.is_some());
        assert!(govt.avg_duration.is_some());
        assert!(govt.avg_spread.is_some());
        assert!(govt.total_dv01.is_some());
    }
}
