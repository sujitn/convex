//! Credit rating-based portfolio bucketing.
//!
//! Provides distribution analysis by credit rating.

use super::sector::{aggregate_bucket_metrics, BucketMetrics};
use crate::types::{AnalyticsConfig, CreditRating, Holding, RatingBucket};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Distribution of holdings by credit rating.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RatingDistribution {
    /// Metrics by individual rating notch (AAA, AA+, AA, etc.).
    pub by_rating: HashMap<CreditRating, BucketMetrics>,

    /// Metrics by rating bucket (AAA, AA, A, BBB, etc.).
    pub by_bucket: HashMap<RatingBucket, BucketMetrics>,

    /// Total portfolio market value.
    pub total_market_value: Decimal,

    /// Holdings without rating classification.
    pub unrated: BucketMetrics,
}

impl RatingDistribution {
    /// Returns the weight of investment grade holdings (BBB- or better).
    #[must_use]
    pub fn investment_grade_weight(&self) -> f64 {
        self.by_rating
            .iter()
            .filter(|(r, _)| r.is_investment_grade())
            .map(|(_, m)| m.weight_pct)
            .sum()
    }

    /// Returns the weight of high yield holdings (BB+ or worse, excluding D and NR).
    #[must_use]
    pub fn high_yield_weight(&self) -> f64 {
        self.by_rating
            .iter()
            .filter(|(r, _)| r.is_high_yield())
            .map(|(_, m)| m.weight_pct)
            .sum()
    }

    /// Returns the weight of defaulted holdings (D rating).
    #[must_use]
    pub fn default_weight(&self) -> f64 {
        self.by_rating
            .get(&CreditRating::D)
            .map(|m| m.weight_pct)
            .unwrap_or(0.0)
    }

    /// Returns the weight of unrated holdings.
    #[must_use]
    pub fn unrated_weight(&self) -> f64 {
        let explicit_nr = self
            .by_rating
            .get(&CreditRating::NotRated)
            .map(|m| m.weight_pct)
            .unwrap_or(0.0);
        explicit_nr + self.unrated.weight_pct
    }

    /// Returns the weighted average rating score (1=AAA, 22=D).
    ///
    /// Lower scores indicate higher credit quality.
    #[must_use]
    pub fn average_rating_score(&self) -> Option<f64> {
        let (sum, weight) = self
            .by_rating
            .iter()
            .filter(|(r, _)| **r != CreditRating::NotRated)
            .fold((0.0, 0.0), |(s, w), (rating, metrics)| {
                let score = rating.score() as f64;
                let mv: f64 = metrics.market_value.try_into().unwrap_or(0.0);
                (s + score * mv, w + mv)
            });

        if weight > 0.0 {
            Some(sum / weight)
        } else {
            None
        }
    }

    /// Returns the implied average rating based on average score.
    #[must_use]
    pub fn average_rating(&self) -> Option<CreditRating> {
        self.average_rating_score().and_then(|score| {
            // Round to nearest rating
            let rounded = score.round() as u8;
            match rounded {
                1 => Some(CreditRating::AAA),
                2 => Some(CreditRating::AAPlus),
                3 => Some(CreditRating::AA),
                4 => Some(CreditRating::AAMinus),
                5 => Some(CreditRating::APlus),
                6 => Some(CreditRating::A),
                7 => Some(CreditRating::AMinus),
                8 => Some(CreditRating::BBBPlus),
                9 => Some(CreditRating::BBB),
                10 => Some(CreditRating::BBBMinus),
                11 => Some(CreditRating::BBPlus),
                12 => Some(CreditRating::BB),
                13 => Some(CreditRating::BBMinus),
                14 => Some(CreditRating::BPlus),
                15 => Some(CreditRating::B),
                16 => Some(CreditRating::BMinus),
                17 => Some(CreditRating::CCCPlus),
                18 => Some(CreditRating::CCC),
                19 => Some(CreditRating::CCCMinus),
                20 => Some(CreditRating::CC),
                21 => Some(CreditRating::C),
                22 => Some(CreditRating::D),
                _ => None,
            }
        })
    }

    /// Returns metrics for a specific rating.
    #[must_use]
    pub fn get(&self, rating: CreditRating) -> Option<&BucketMetrics> {
        self.by_rating.get(&rating)
    }

    /// Returns metrics for a specific rating bucket.
    #[must_use]
    pub fn get_bucket(&self, bucket: RatingBucket) -> Option<&BucketMetrics> {
        self.by_bucket.get(&bucket)
    }

    /// Returns all ratings with their metrics, sorted by rating (best to worst).
    #[must_use]
    pub fn sorted_by_rating(&self) -> Vec<(CreditRating, &BucketMetrics)> {
        let mut result: Vec<_> = self.by_rating.iter().map(|(r, m)| (*r, m)).collect();
        result.sort_by_key(|(r, _)| *r);
        result
    }

    /// Returns all buckets with their metrics, sorted by bucket (best to worst).
    #[must_use]
    pub fn sorted_by_bucket(&self) -> Vec<(RatingBucket, &BucketMetrics)> {
        let mut result: Vec<_> = self.by_bucket.iter().map(|(b, m)| (*b, m)).collect();
        result.sort_by_key(|(b, _)| *b);
        result
    }
}

/// Buckets holdings by credit rating.
///
/// Holdings are grouped by their composite credit rating. Holdings without
/// a rating classification are placed in the "unrated" bucket.
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to bucket
/// * `config` - Analytics configuration (controls weighting and parallelism)
///
/// # Returns
///
/// Distribution of holdings by credit rating with aggregated metrics.
#[must_use]
pub fn bucket_by_rating(holdings: &[Holding], config: &AnalyticsConfig) -> RatingDistribution {
    if holdings.is_empty() {
        return RatingDistribution::default();
    }

    // Calculate total market value for weight percentages
    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();

    if total_mv.is_zero() {
        return RatingDistribution {
            total_market_value: Decimal::ZERO,
            ..Default::default()
        };
    }

    // Group holdings by rating (using indices to avoid lifetime issues)
    let mut grouped: HashMap<Option<CreditRating>, Vec<usize>> = HashMap::new();
    for (i, h) in holdings.iter().enumerate() {
        let rating = h.classification.rating.composite;
        grouped.entry(rating).or_default().push(i);
    }

    // Aggregate metrics for each group
    let mut by_rating = HashMap::new();
    let mut unrated = BucketMetrics::default();

    for (rating_opt, indices) in grouped {
        let group: Vec<&Holding> = indices.iter().map(|&i| &holdings[i]).collect();
        let metrics = aggregate_bucket_metrics(&group, total_mv, config);

        match rating_opt {
            Some(rating) => {
                by_rating.insert(rating, metrics);
            }
            None => {
                unrated = metrics;
            }
        }
    }

    // Also aggregate by bucket
    let mut by_bucket: HashMap<RatingBucket, BucketMetrics> = HashMap::new();
    for (rating, metrics) in &by_rating {
        let bucket = rating.bucket();
        let entry = by_bucket.entry(bucket).or_default();
        entry.count += metrics.count;
        entry.market_value += metrics.market_value;
        entry.par_value += metrics.par_value;
        // Weight will be recalculated below
    }

    // Calculate bucket weights
    for metrics in by_bucket.values_mut() {
        if !total_mv.is_zero() {
            let weight = metrics.market_value / total_mv * Decimal::ONE_HUNDRED;
            metrics.weight_pct = weight.try_into().unwrap_or(0.0);
        }
    }

    RatingDistribution {
        by_rating,
        by_bucket,
        total_market_value: total_mv,
        unrated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Classification, HoldingAnalytics, HoldingBuilder, RatingInfo};
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal_macros::dec;

    fn create_test_holding(id: &str, mv: Decimal, rating: Option<CreditRating>) -> Holding {
        HoldingBuilder::new()
            .id(id)
            .identifiers(BondIdentifiers::from_isin_str("US912828Z229").unwrap())
            .par_amount(dec!(1_000_000))
            .market_price(mv)
            .classification(Classification::new().with_rating(match rating {
                Some(r) => RatingInfo::from_composite(r),
                None => RatingInfo::new(),
            }))
            .analytics(
                HoldingAnalytics::new()
                    .with_ytm(0.05)
                    .with_modified_duration(5.0)
                    .with_dv01(0.05),
            )
            .build()
            .unwrap()
    }

    #[test]
    fn test_bucket_by_rating_empty() {
        let holdings: Vec<Holding> = vec![];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_rating(&holdings, &config);

        assert!(dist.by_rating.is_empty());
        assert!(dist.total_market_value.is_zero());
    }

    #[test]
    fn test_bucket_by_rating_single() {
        let holdings = vec![create_test_holding(
            "H1",
            dec!(100),
            Some(CreditRating::AAA),
        )];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_rating(&holdings, &config);

        assert_eq!(dist.by_rating.len(), 1);
        let aaa = dist.get(CreditRating::AAA).unwrap();
        assert_eq!(aaa.count, 1);
        assert!((aaa.weight_pct - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_investment_grade_weight() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(CreditRating::AAA)),
            create_test_holding("H2", dec!(100), Some(CreditRating::BBB)),
            create_test_holding("H3", dec!(100), Some(CreditRating::BB)), // HY
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_rating(&holdings, &config);

        // AAA + BBB = 66.67%
        assert!((dist.investment_grade_weight() - 66.67).abs() < 0.1);
        // BB = 33.33%
        assert!((dist.high_yield_weight() - 33.33).abs() < 0.1);
    }

    #[test]
    fn test_average_rating_score() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(CreditRating::AAA)), // Score 1
            create_test_holding("H2", dec!(100), Some(CreditRating::BBB)), // Score 9
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_rating(&holdings, &config);

        // Average should be (1 + 9) / 2 = 5 (A+)
        let avg = dist.average_rating_score().unwrap();
        assert!((avg - 5.0).abs() < 0.01);

        let avg_rating = dist.average_rating().unwrap();
        assert_eq!(avg_rating, CreditRating::APlus);
    }

    #[test]
    fn test_bucket_aggregation() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(CreditRating::AAPlus)),
            create_test_holding("H2", dec!(100), Some(CreditRating::AA)),
            create_test_holding("H3", dec!(100), Some(CreditRating::AAMinus)),
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_rating(&holdings, &config);

        // All three should be in the AA bucket
        assert_eq!(dist.by_rating.len(), 3);
        assert_eq!(dist.by_bucket.len(), 1);

        let aa_bucket = dist.get_bucket(RatingBucket::AA).unwrap();
        assert_eq!(aa_bucket.count, 3);
        assert!((aa_bucket.weight_pct - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_unrated_holdings() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(CreditRating::AAA)),
            create_test_holding("H2", dec!(100), None), // No rating
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_rating(&holdings, &config);

        assert_eq!(dist.by_rating.len(), 1);
        assert_eq!(dist.unrated.count, 1);
        assert!((dist.unrated_weight() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_sorted_by_rating() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(CreditRating::BB)),
            create_test_holding("H2", dec!(100), Some(CreditRating::AAA)),
            create_test_holding("H3", dec!(100), Some(CreditRating::A)),
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_rating(&holdings, &config);
        let sorted = dist.sorted_by_rating();

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].0, CreditRating::AAA); // Best
        assert_eq!(sorted[1].0, CreditRating::A);
        assert_eq!(sorted[2].0, CreditRating::BB); // Worst
    }
}
