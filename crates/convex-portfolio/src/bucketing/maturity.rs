//! Maturity-based portfolio bucketing.
//!
//! Provides distribution analysis by time to maturity.

use super::sector::{aggregate_bucket_metrics, BucketMetrics};
use crate::types::{AnalyticsConfig, Holding, MaturityBucket};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Distribution of holdings by maturity bucket.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MaturityDistribution {
    /// Metrics by maturity bucket.
    pub by_bucket: HashMap<MaturityBucket, BucketMetrics>,

    /// Total portfolio market value.
    pub total_market_value: Decimal,

    /// Holdings without maturity information.
    pub unknown: BucketMetrics,

    /// Weighted average years to maturity.
    pub weighted_avg_maturity: Option<f64>,
}

impl MaturityDistribution {
    /// Returns the weight of short-term holdings (0-3 years).
    #[must_use]
    pub fn short_term_weight(&self) -> f64 {
        [MaturityBucket::ZeroToOne, MaturityBucket::OneToThree]
            .iter()
            .filter_map(|b| self.by_bucket.get(b))
            .map(|m| m.weight_pct)
            .sum()
    }

    /// Returns the weight of intermediate holdings (3-10 years).
    #[must_use]
    pub fn intermediate_weight(&self) -> f64 {
        [
            MaturityBucket::ThreeToFive,
            MaturityBucket::FiveToSeven,
            MaturityBucket::SevenToTen,
        ]
        .iter()
        .filter_map(|b| self.by_bucket.get(b))
        .map(|m| m.weight_pct)
        .sum()
    }

    /// Returns the weight of long-term holdings (10+ years).
    #[must_use]
    pub fn long_term_weight(&self) -> f64 {
        [
            MaturityBucket::TenToTwenty,
            MaturityBucket::TwentyToThirty,
            MaturityBucket::ThirtyPlus,
        ]
        .iter()
        .filter_map(|b| self.by_bucket.get(b))
        .map(|m| m.weight_pct)
        .sum()
    }

    /// Returns metrics for a specific bucket.
    #[must_use]
    pub fn get(&self, bucket: MaturityBucket) -> Option<&BucketMetrics> {
        self.by_bucket.get(&bucket)
    }

    /// Returns all buckets with their metrics, sorted by maturity (shortest to longest).
    #[must_use]
    pub fn sorted_by_maturity(&self) -> Vec<(MaturityBucket, &BucketMetrics)> {
        let mut result: Vec<_> = self.by_bucket.iter().map(|(b, m)| (*b, m)).collect();
        result.sort_by_key(|(b, _)| *b);
        result
    }
}

/// Buckets holdings by maturity.
///
/// Holdings are grouped by their years to maturity using standard maturity buckets.
/// Holdings without maturity information (years_to_maturity = None) are placed
/// in the "unknown" bucket.
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to bucket
/// * `config` - Analytics configuration (controls weighting and parallelism)
///
/// # Returns
///
/// Distribution of holdings by maturity bucket with aggregated metrics.
#[must_use]
pub fn bucket_by_maturity(holdings: &[Holding], config: &AnalyticsConfig) -> MaturityDistribution {
    if holdings.is_empty() {
        return MaturityDistribution::default();
    }

    // Calculate total market value for weight percentages
    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();

    if total_mv.is_zero() {
        return MaturityDistribution {
            total_market_value: Decimal::ZERO,
            ..Default::default()
        };
    }

    // Calculate weighted average maturity
    let (maturity_sum, maturity_weight) = holdings.iter().fold((0.0, 0.0), |(sum, w), h| {
        if let Some(ytm) = h.analytics.years_to_maturity {
            let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
            (sum + ytm * mv, w + mv)
        } else {
            (sum, w)
        }
    });

    let weighted_avg_maturity = if maturity_weight > 0.0 {
        Some(maturity_sum / maturity_weight)
    } else {
        None
    };

    // Group holdings by maturity bucket (using indices to avoid lifetime issues)
    let mut grouped: HashMap<Option<MaturityBucket>, Vec<usize>> = HashMap::new();
    for (i, h) in holdings.iter().enumerate() {
        let bucket = h
            .analytics
            .years_to_maturity
            .map(MaturityBucket::from_years);
        grouped.entry(bucket).or_default().push(i);
    }

    // Aggregate metrics for each group
    let mut by_bucket = HashMap::new();
    let mut unknown = BucketMetrics::default();

    for (bucket_opt, indices) in grouped {
        let group: Vec<&Holding> = indices.iter().map(|&i| &holdings[i]).collect();
        let metrics = aggregate_bucket_metrics(&group, total_mv, config);

        match bucket_opt {
            Some(bucket) => {
                by_bucket.insert(bucket, metrics);
            }
            None => {
                unknown = metrics;
            }
        }
    }

    MaturityDistribution {
        by_bucket,
        total_market_value: total_mv,
        unknown,
        weighted_avg_maturity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Classification, HoldingAnalytics, HoldingBuilder};
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal_macros::dec;

    fn create_test_holding(id: &str, mv: Decimal, years_to_maturity: Option<f64>) -> Holding {
        let mut analytics = HoldingAnalytics::new()
            .with_ytm(0.05)
            .with_modified_duration(5.0)
            .with_dv01(0.05);

        if let Some(ytm) = years_to_maturity {
            analytics = analytics.with_years_to_maturity(ytm);
        }

        HoldingBuilder::new()
            .id(id)
            .identifiers(BondIdentifiers::from_isin_str("US912828Z229").unwrap())
            .par_amount(dec!(1_000_000))
            .market_price(mv)
            .classification(Classification::new())
            .analytics(analytics)
            .build()
            .unwrap()
    }

    #[test]
    fn test_bucket_by_maturity_empty() {
        let holdings: Vec<Holding> = vec![];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_maturity(&holdings, &config);

        assert!(dist.by_bucket.is_empty());
        assert!(dist.total_market_value.is_zero());
    }

    #[test]
    fn test_bucket_by_maturity_single() {
        let holdings = vec![create_test_holding("H1", dec!(100), Some(2.5))]; // 1-3Y bucket
        let config = AnalyticsConfig::default();

        let dist = bucket_by_maturity(&holdings, &config);

        assert_eq!(dist.by_bucket.len(), 1);
        let bucket = dist.get(MaturityBucket::OneToThree).unwrap();
        assert_eq!(bucket.count, 1);
        assert!((bucket.weight_pct - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_bucket_by_maturity_multiple() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(0.5)), // 0-1Y
            create_test_holding("H2", dec!(100), Some(2.0)), // 1-3Y
            create_test_holding("H3", dec!(100), Some(15.0)), // 10-20Y
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_maturity(&holdings, &config);

        assert_eq!(dist.by_bucket.len(), 3);
        assert!(dist.get(MaturityBucket::ZeroToOne).is_some());
        assert!(dist.get(MaturityBucket::OneToThree).is_some());
        assert!(dist.get(MaturityBucket::TenToTwenty).is_some());
    }

    #[test]
    fn test_short_intermediate_long_weights() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(0.5)), // Short (0-1Y)
            create_test_holding("H2", dec!(100), Some(2.0)), // Short (1-3Y)
            create_test_holding("H3", dec!(100), Some(5.0)), // Intermediate (3-5Y)
            create_test_holding("H4", dec!(100), Some(8.0)), // Intermediate (7-10Y)
            create_test_holding("H5", dec!(100), Some(15.0)), // Long (10-20Y)
            create_test_holding("H6", dec!(100), Some(25.0)), // Long (20-30Y)
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_maturity(&holdings, &config);

        // 2 short, 2 intermediate, 2 long = each 33.33%
        assert!((dist.short_term_weight() - 33.33).abs() < 0.1);
        assert!((dist.intermediate_weight() - 33.33).abs() < 0.1);
        assert!((dist.long_term_weight() - 33.33).abs() < 0.1);
    }

    #[test]
    fn test_weighted_avg_maturity() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(2.0)), // 2Y @ 50%
            create_test_holding("H2", dec!(100), Some(10.0)), // 10Y @ 50%
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_maturity(&holdings, &config);

        // Average should be (2 + 10) / 2 = 6 years
        let avg = dist.weighted_avg_maturity.unwrap();
        assert!((avg - 6.0).abs() < 0.01);
    }

    #[test]
    fn test_unknown_maturity() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(5.0)),
            create_test_holding("H2", dec!(100), None), // No maturity
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_maturity(&holdings, &config);

        assert_eq!(dist.by_bucket.len(), 1);
        assert_eq!(dist.unknown.count, 1);
        assert!((dist.unknown.weight_pct - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_sorted_by_maturity() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(25.0)), // 20-30Y
            create_test_holding("H2", dec!(100), Some(0.5)),  // 0-1Y
            create_test_holding("H3", dec!(100), Some(5.0)),  // 3-5Y
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_maturity(&holdings, &config);
        let sorted = dist.sorted_by_maturity();

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].0, MaturityBucket::ZeroToOne); // Shortest
        assert_eq!(sorted[1].0, MaturityBucket::ThreeToFive);
        assert_eq!(sorted[2].0, MaturityBucket::TwentyToThirty); // Longest
    }

    #[test]
    fn test_all_buckets() {
        // Test each bucket boundary
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(0.5)), // 0-1Y
            create_test_holding("H2", dec!(100), Some(2.0)), // 1-3Y
            create_test_holding("H3", dec!(100), Some(4.0)), // 3-5Y
            create_test_holding("H4", dec!(100), Some(6.0)), // 5-7Y
            create_test_holding("H5", dec!(100), Some(8.0)), // 7-10Y
            create_test_holding("H6", dec!(100), Some(15.0)), // 10-20Y
            create_test_holding("H7", dec!(100), Some(25.0)), // 20-30Y
            create_test_holding("H8", dec!(100), Some(40.0)), // 30+Y
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_maturity(&holdings, &config);

        assert_eq!(dist.by_bucket.len(), 8);
        for bucket in MaturityBucket::all() {
            assert!(dist.get(*bucket).is_some(), "Missing bucket: {:?}", bucket);
        }
    }
}
