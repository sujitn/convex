//! Custom user-defined portfolio bucketing.
//!
//! Provides flexible bucketing by user-defined classification schemes.

use super::sector::{aggregate_bucket_metrics, BucketMetrics};
use crate::types::{AnalyticsConfig, Holding};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Distribution of holdings by a custom classification.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CustomDistribution {
    /// Metrics by bucket key.
    pub by_bucket: HashMap<String, BucketMetrics>,

    /// Total portfolio market value.
    pub total_market_value: Decimal,

    /// Holdings that didn't match any bucket.
    pub unclassified: BucketMetrics,
}

impl CustomDistribution {
    /// Returns metrics for a specific bucket.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&BucketMetrics> {
        self.by_bucket.get(key)
    }

    /// Returns all buckets with their metrics, sorted by weight descending.
    #[must_use]
    pub fn sorted_by_weight(&self) -> Vec<(&str, &BucketMetrics)> {
        let mut result: Vec<_> = self
            .by_bucket
            .iter()
            .map(|(k, m)| (k.as_str(), m))
            .collect();
        result.sort_by(|a, b| {
            b.1.weight_pct
                .partial_cmp(&a.1.weight_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        result
    }

    /// Returns all buckets with their metrics, sorted by key alphabetically.
    #[must_use]
    pub fn sorted_by_key(&self) -> Vec<(&str, &BucketMetrics)> {
        let mut result: Vec<_> = self
            .by_bucket
            .iter()
            .map(|(k, m)| (k.as_str(), m))
            .collect();
        result.sort_by_key(|(k, _)| *k);
        result
    }

    /// Returns the number of distinct buckets.
    #[must_use]
    pub fn bucket_count(&self) -> usize {
        self.by_bucket.len()
    }
}

/// Buckets holdings by a custom field in their classification.
///
/// Holdings are grouped by the value of a custom field in their classification.
/// Holdings without the specified field are placed in the "unclassified" bucket.
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to bucket
/// * `field_name` - Name of the custom field to bucket by
/// * `config` - Analytics configuration (controls weighting and parallelism)
///
/// # Returns
///
/// Distribution of holdings by the custom field values.
///
/// # Example
///
/// ```rust,ignore
/// use convex_portfolio::bucketing::bucket_by_custom_field;
///
/// // Holdings have classification.custom["strategy"] = "Core" or "Satellite"
/// let dist = bucket_by_custom_field(&holdings, "strategy", &config);
/// let core = dist.get("Core");
/// ```
#[must_use]
pub fn bucket_by_custom_field(
    holdings: &[Holding],
    field_name: &str,
    config: &AnalyticsConfig,
) -> CustomDistribution {
    bucket_by_classifier(
        holdings,
        |h| h.classification.custom.get(field_name).cloned(),
        config,
    )
}

/// Buckets holdings by country.
///
/// Holdings are grouped by their country classification.
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to bucket
/// * `config` - Analytics configuration (controls weighting and parallelism)
#[must_use]
pub fn bucket_by_country(holdings: &[Holding], config: &AnalyticsConfig) -> CustomDistribution {
    bucket_by_classifier(holdings, |h| h.classification.country.clone(), config)
}

/// Buckets holdings by region.
///
/// Holdings are grouped by their region classification.
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to bucket
/// * `config` - Analytics configuration (controls weighting and parallelism)
#[must_use]
pub fn bucket_by_region(holdings: &[Holding], config: &AnalyticsConfig) -> CustomDistribution {
    bucket_by_classifier(holdings, |h| h.classification.region.clone(), config)
}

/// Buckets holdings by issuer.
///
/// Holdings are grouped by their issuer name.
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to bucket
/// * `config` - Analytics configuration (controls weighting and parallelism)
#[must_use]
pub fn bucket_by_issuer(holdings: &[Holding], config: &AnalyticsConfig) -> CustomDistribution {
    bucket_by_classifier(holdings, |h| h.classification.issuer.clone(), config)
}

/// Buckets holdings by currency.
///
/// Holdings are grouped by their currency.
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to bucket
/// * `config` - Analytics configuration (controls weighting and parallelism)
#[must_use]
pub fn bucket_by_currency(holdings: &[Holding], config: &AnalyticsConfig) -> CustomDistribution {
    bucket_by_classifier(holdings, |h| Some(h.currency.to_string()), config)
}

/// Generic bucketing by a classifier function.
///
/// This is the core implementation used by all custom bucketing functions.
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to bucket
/// * `classifier` - Function that extracts the bucket key from a holding
/// * `config` - Analytics configuration
///
/// # Example
///
/// ```rust,ignore
/// // Bucket by first letter of issuer name
/// let dist = bucket_by_classifier(&holdings, |h| {
///     h.classification.issuer.as_ref().and_then(|s| s.chars().next().map(|c| c.to_string()))
/// }, &config);
/// ```
#[must_use]
pub fn bucket_by_classifier<F>(
    holdings: &[Holding],
    classifier: F,
    config: &AnalyticsConfig,
) -> CustomDistribution
where
    F: Fn(&Holding) -> Option<String> + Sync,
{
    if holdings.is_empty() {
        return CustomDistribution::default();
    }

    // Calculate total market value for weight percentages
    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();

    if total_mv.is_zero() {
        return CustomDistribution {
            total_market_value: Decimal::ZERO,
            ..Default::default()
        };
    }

    // Group holdings by classifier result (using indices to avoid lifetime issues)
    let mut grouped: HashMap<Option<String>, Vec<usize>> = HashMap::new();
    for (i, h) in holdings.iter().enumerate() {
        let key = classifier(h);
        grouped.entry(key).or_default().push(i);
    }

    // Aggregate metrics for each group
    let mut by_bucket = HashMap::new();
    let mut unclassified = BucketMetrics::default();

    for (key_opt, indices) in grouped {
        let group: Vec<&Holding> = indices.iter().map(|&i| &holdings[i]).collect();
        let metrics = aggregate_bucket_metrics(&group, total_mv, config);

        match key_opt {
            Some(key) => {
                by_bucket.insert(key, metrics);
            }
            None => {
                unclassified = metrics;
            }
        }
    }

    CustomDistribution {
        by_bucket,
        total_market_value: total_mv,
        unclassified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Classification, HoldingAnalytics, HoldingBuilder};
    use convex_bonds::types::BondIdentifiers;
    use convex_core::types::Currency;
    use rust_decimal_macros::dec;

    fn create_test_holding(
        id: &str,
        mv: Decimal,
        country: Option<&str>,
        region: Option<&str>,
        issuer: Option<&str>,
        currency: Currency,
    ) -> Holding {
        let mut classification = Classification::new();
        if let Some(c) = country {
            classification = classification.with_country(c);
        }
        if let Some(r) = region {
            classification = classification.with_region(r);
        }
        if let Some(i) = issuer {
            classification = classification.with_issuer(i);
        }

        HoldingBuilder::new()
            .id(id)
            .identifiers(BondIdentifiers::from_isin_str("US912828Z229").unwrap())
            .par_amount(dec!(1_000_000))
            .market_price(mv)
            .currency(currency)
            .classification(classification)
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
    fn test_bucket_by_country() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some("US"), None, None, Currency::USD),
            create_test_holding("H2", dec!(100), Some("US"), None, None, Currency::USD),
            create_test_holding("H3", dec!(100), Some("GB"), None, None, Currency::GBP),
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_country(&holdings, &config);

        assert_eq!(dist.by_bucket.len(), 2);
        let us = dist.get("US").unwrap();
        assert_eq!(us.count, 2);
        assert!((us.weight_pct - 66.67).abs() < 0.1);

        let gb = dist.get("GB").unwrap();
        assert_eq!(gb.count, 1);
        assert!((gb.weight_pct - 33.33).abs() < 0.1);
    }

    #[test]
    fn test_bucket_by_region() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), None, Some("Americas"), None, Currency::USD),
            create_test_holding("H2", dec!(100), None, Some("EMEA"), None, Currency::EUR),
            create_test_holding("H3", dec!(100), None, Some("EMEA"), None, Currency::GBP),
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_region(&holdings, &config);

        assert_eq!(dist.by_bucket.len(), 2);
        let emea = dist.get("EMEA").unwrap();
        assert_eq!(emea.count, 2);
    }

    #[test]
    fn test_bucket_by_issuer() {
        let holdings = vec![
            create_test_holding(
                "H1",
                dec!(100),
                None,
                None,
                Some("Apple Inc"),
                Currency::USD,
            ),
            create_test_holding(
                "H2",
                dec!(200),
                None,
                None,
                Some("Microsoft"),
                Currency::USD,
            ),
            create_test_holding(
                "H3",
                dec!(100),
                None,
                None,
                Some("Apple Inc"),
                Currency::USD,
            ),
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_issuer(&holdings, &config);

        assert_eq!(dist.by_bucket.len(), 2);

        let apple = dist.get("Apple Inc").unwrap();
        assert_eq!(apple.count, 2);
        assert!((apple.weight_pct - 50.0).abs() < 0.1);

        let msft = dist.get("Microsoft").unwrap();
        assert_eq!(msft.count, 1);
        assert!((msft.weight_pct - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_bucket_by_currency() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), None, None, None, Currency::USD),
            create_test_holding("H2", dec!(100), None, None, None, Currency::EUR),
            create_test_holding("H3", dec!(100), None, None, None, Currency::USD),
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_currency(&holdings, &config);

        assert_eq!(dist.by_bucket.len(), 2);
        let usd = dist.get("USD").unwrap();
        assert_eq!(usd.count, 2);
        assert!((usd.weight_pct - 66.67).abs() < 0.1);
    }

    #[test]
    fn test_bucket_by_custom_field() {
        let holdings = vec![
            {
                let mut h = create_test_holding("H1", dec!(100), None, None, None, Currency::USD);
                h.classification = h.classification.with_custom("strategy", "Core");
                h
            },
            {
                let mut h = create_test_holding("H2", dec!(100), None, None, None, Currency::USD);
                h.classification = h.classification.with_custom("strategy", "Satellite");
                h
            },
            {
                let mut h = create_test_holding("H3", dec!(100), None, None, None, Currency::USD);
                h.classification = h.classification.with_custom("strategy", "Core");
                h
            },
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_custom_field(&holdings, "strategy", &config);

        assert_eq!(dist.by_bucket.len(), 2);
        let core = dist.get("Core").unwrap();
        assert_eq!(core.count, 2);
        assert!((core.weight_pct - 66.67).abs() < 0.1);
    }

    #[test]
    fn test_bucket_by_classifier_custom() {
        let holdings = vec![
            create_test_holding(
                "H1",
                dec!(100),
                None,
                None,
                Some("Apple Inc"),
                Currency::USD,
            ),
            create_test_holding("H2", dec!(100), None, None, Some("Amazon"), Currency::USD),
            create_test_holding(
                "H3",
                dec!(100),
                None,
                None,
                Some("Microsoft"),
                Currency::USD,
            ),
        ];
        let config = AnalyticsConfig::default();

        // Bucket by first letter of issuer
        let dist = bucket_by_classifier(
            &holdings,
            |h| {
                h.classification
                    .issuer
                    .as_ref()
                    .and_then(|s| s.chars().next().map(|c| c.to_string()))
            },
            &config,
        );

        assert_eq!(dist.by_bucket.len(), 2); // A (Apple, Amazon) and M (Microsoft)
        let a = dist.get("A").unwrap();
        assert_eq!(a.count, 2);
        let m = dist.get("M").unwrap();
        assert_eq!(m.count, 1);
    }

    #[test]
    fn test_unclassified() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some("US"), None, None, Currency::USD),
            create_test_holding("H2", dec!(100), None, None, None, Currency::USD), // No country
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_country(&holdings, &config);

        assert_eq!(dist.by_bucket.len(), 1);
        assert_eq!(dist.unclassified.count, 1);
        assert!((dist.unclassified.weight_pct - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_sorted_by_weight() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some("US"), None, None, Currency::USD),
            create_test_holding("H2", dec!(200), Some("GB"), None, None, Currency::GBP),
            create_test_holding("H3", dec!(50), Some("DE"), None, None, Currency::EUR),
        ];
        let config = AnalyticsConfig::default();

        let dist = bucket_by_country(&holdings, &config);
        let sorted = dist.sorted_by_weight();

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].0, "GB"); // Highest weight
        assert_eq!(sorted[1].0, "US");
        assert_eq!(sorted[2].0, "DE"); // Lowest weight
    }
}
