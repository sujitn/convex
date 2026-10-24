//! Sector-based portfolio bucketing.
//!
//! Provides distribution analysis by issuer sector.

use crate::types::{AnalyticsConfig, Holding, Sector};
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if this bucket is empty.
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
    pub fn government_weight(&self) -> f64 {
        [Sector::Government, Sector::Agency, Sector::Supranational]
            .iter()
            .filter_map(|s| self.by_sector.get(s))
            .map(|m| m.weight_pct)
            .sum()
    }

    /// Returns the weight of securitized sectors (ABS + MBS + Covered).
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
    pub fn credit_weight(&self) -> f64 {
        [Sector::Corporate, Sector::Financial, Sector::Utility]
            .iter()
            .filter_map(|s| self.by_sector.get(s))
            .map(|m| m.weight_pct)
            .sum()
    }

    /// Returns metrics for a specific sector.
    pub fn get(&self, sector: Sector) -> Option<&BucketMetrics> {
        self.by_sector.get(&sector)
    }

    /// Returns all sectors with their metrics, sorted by weight descending.
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
pub fn bucket_by_sector(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> crate::PortfolioResult<SectorDistribution> {
    Ok({
        if holdings.is_empty() {
            return Ok(SectorDistribution::default());
        }

        // Calculate total market value for weight percentages
        let total_mv: Decimal = crate::analytics::metrics::clean_total(holdings.iter())?;

        if total_mv.is_zero() {
            return Err(crate::PortfolioError::invalid_portfolio(
                "nonempty bucket weights are undefined at zero net market value",
            ));
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
            let metrics = aggregate_bucket_metrics(&group, total_mv, config)?;

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
    })
}

/// Checked complete metrics for one original holding cohort.
pub(crate) fn aggregate_bucket_metrics(
    holdings: &[&Holding],
    total_mv: Decimal,
    config: &AnalyticsConfig,
) -> crate::PortfolioResult<BucketMetrics> {
    use crate::analytics::metrics::{self, MetricBook};
    use convex_math::decimal::BoundedDecimal;
    if holdings.is_empty() {
        return Ok(BucketMetrics::default());
    }
    let market_value = metrics::clean_total(holdings.iter().copied())?;
    let par_value = metrics::par_total(holdings.iter().copied())?;
    let weight = metrics::ratio_as_f64(
        &BoundedDecimal::decimal(market_value).multiply(Decimal::ONE_HUNDRED)?,
        &BoundedDecimal::decimal(total_mv),
    )?
    .ok_or_else(|| {
        crate::PortfolioError::invalid_portfolio("bucket weight is undefined at zero total value")
    })?;
    let book = MetricBook::from_rows(holdings.iter().copied(), config)?;
    Ok(BucketMetrics {
        count: holdings.len(),
        market_value,
        par_value,
        weight_pct: weight,
        avg_ytm: book.weighted("ytm", |h| h.analytics.ytm)?,
        avg_duration: book.weighted("best_duration", |h| h.analytics.best_duration())?,
        avg_spread: book.weighted("best_spread", |h| h.analytics.best_spread())?,
        total_dv01: metrics::currency_rows(holdings.iter().copied(), "dv01", |h| h.analytics.dv01)?
            .map(|v| v.finish().map_err(crate::PortfolioError::from))
            .transpose()?,
    })
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

        let dist = bucket_by_sector(&holdings, &config).unwrap();

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

        let dist = bucket_by_sector(&holdings, &config).unwrap();

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

        let dist = bucket_by_sector(&holdings, &config).unwrap();

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

        let dist = bucket_by_sector(&holdings, &config).unwrap();

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

        let dist = bucket_by_sector(&holdings, &config).unwrap();

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

        let dist = bucket_by_sector(&holdings, &config).unwrap();

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

        let dist = bucket_by_sector(&holdings, &config).unwrap();
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

        let dist = bucket_by_sector(&holdings, &config).unwrap();
        let govt = dist.get(Sector::Government).unwrap();

        // Should have aggregated metrics
        assert!(govt.avg_ytm.is_some());
        assert!(govt.avg_duration.is_some());
        assert!(govt.avg_spread.is_some());
        assert!(govt.total_dv01.is_some());
    }
}
