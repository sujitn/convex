//! Key rate duration analytics for portfolios.
//!
//! Aggregates key rate duration profiles across holdings to understand
//! interest rate sensitivity at different points on the curve.
//!
//! Key rate durations are additive across the portfolio when weighted
//! by market value or DV01.

use crate::analytics::parallel::maybe_parallel_fold;
use crate::types::{AnalyticsConfig, Holding, WeightingMethod};
use convex_analytics::risk::{
    Duration, KeyRateDuration, KeyRateDurations, STANDARD_KEY_RATE_TENORS,
};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Aggregated key rate duration profile for a portfolio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRateProfile {
    /// Key rate durations at each tenor point.
    pub durations: Vec<KeyRateDuration>,

    /// Total duration (sum of all key rate durations).
    pub total_duration: f64,

    /// Number of holdings with KRD data.
    pub coverage: usize,

    /// Total number of holdings.
    pub total_holdings: usize,

    /// Tenor points used.
    pub tenors: Vec<f64>,
}

impl KeyRateProfile {
    /// Returns the coverage as a percentage.
    #[must_use]
    pub fn coverage_pct(&self) -> f64 {
        if self.total_holdings > 0 {
            self.coverage as f64 / self.total_holdings as f64 * 100.0
        } else {
            0.0
        }
    }

    /// Gets the duration at a specific tenor.
    #[must_use]
    pub fn at_tenor(&self, tenor: f64) -> Option<f64> {
        self.durations
            .iter()
            .find(|krd| (krd.tenor - tenor).abs() < 0.001)
            .map(|krd| krd.duration.as_f64())
    }

    /// Gets the duration for tenors in a range.
    #[must_use]
    pub fn in_range(&self, min_tenor: f64, max_tenor: f64) -> Vec<&KeyRateDuration> {
        self.durations
            .iter()
            .filter(|krd| krd.tenor >= min_tenor && krd.tenor <= max_tenor)
            .collect()
    }

    /// Converts to a KeyRateDurations type for compatibility.
    #[must_use]
    pub fn to_key_rate_durations(&self) -> KeyRateDurations {
        KeyRateDurations::new(self.durations.clone())
    }

    /// Returns the short-end duration (< 2 years).
    #[must_use]
    pub fn short_duration(&self) -> f64 {
        self.durations
            .iter()
            .filter(|krd| krd.tenor < 2.0)
            .map(|krd| krd.duration.as_f64())
            .sum()
    }

    /// Returns the intermediate duration (2-10 years).
    #[must_use]
    pub fn intermediate_duration(&self) -> f64 {
        self.durations
            .iter()
            .filter(|krd| krd.tenor >= 2.0 && krd.tenor <= 10.0)
            .map(|krd| krd.duration.as_f64())
            .sum()
    }

    /// Returns the long-end duration (> 10 years).
    #[must_use]
    pub fn long_duration(&self) -> f64 {
        self.durations
            .iter()
            .filter(|krd| krd.tenor > 10.0)
            .map(|krd| krd.duration.as_f64())
            .sum()
    }
}

/// Aggregates key rate durations across a portfolio.
///
/// ## Formula
///
/// For each tenor point:
/// ```text
/// KRD_portfolio(t) = Σ(w_i × KRD_i(t)) / Σ(w_i)
/// ```
///
/// Where weights are based on market value (default) or other weighting method.
///
/// # Arguments
///
/// * `holdings` - Portfolio holdings
/// * `config` - Analytics configuration
/// * `tenors` - Optional custom tenors; uses standard tenors if None
///
/// # Returns
///
/// Returns `None` if no holdings have KRD data.
#[must_use]
pub fn aggregate_key_rate_profile(
    holdings: &[Holding],
    config: &AnalyticsConfig,
    tenors: Option<&[f64]>,
) -> Option<KeyRateProfile> {
    let tenor_points = tenors.unwrap_or(STANDARD_KEY_RATE_TENORS);

    // Count coverage
    let coverage = holdings
        .iter()
        .filter(|h| h.analytics.key_rate_durations.is_some())
        .count();

    if coverage == 0 {
        return None;
    }

    // For each tenor, calculate weighted average KRD
    let mut aggregated: BTreeMap<i64, (f64, f64)> = BTreeMap::new(); // tenor_key -> (weighted_sum, weight_sum)

    // Initialize all tenors
    for &tenor in tenor_points {
        let key = (tenor * 1000.0) as i64; // Convert to integer key for BTreeMap
        aggregated.insert(key, (0.0, 0.0));
    }

    // Aggregate using parallel fold if configured
    let tenor_sums = maybe_parallel_fold(
        holdings,
        config,
        aggregated,
        |mut acc, h| {
            if let Some(krd) = &h.analytics.key_rate_durations {
                let weight = weight_for_holding(h, config.weighting);

                for &tenor in tenor_points {
                    let key = (tenor * 1000.0) as i64;
                    if let Some(krd_at_tenor) = krd.at_tenor(tenor) {
                        let duration_val = krd_at_tenor.duration.as_f64();
                        if let Some((sum, wt)) = acc.get_mut(&key) {
                            *sum += duration_val * weight;
                            *wt += weight;
                        }
                    }
                }
            }
            acc
        },
        |mut a, b| {
            for (k, (sum, wt)) in b {
                if let Some((a_sum, a_wt)) = a.get_mut(&k) {
                    *a_sum += sum;
                    *a_wt += wt;
                }
            }
            a
        },
    );

    // Convert to KeyRateDuration vector
    let durations: Vec<KeyRateDuration> = tenor_points
        .iter()
        .map(|&tenor| {
            let key = (tenor * 1000.0) as i64;
            let (sum, wt) = tenor_sums.get(&key).copied().unwrap_or((0.0, 0.0));
            let avg_duration = if wt > 0.0 { sum / wt } else { 0.0 };
            KeyRateDuration {
                tenor,
                duration: Duration::from(avg_duration),
            }
        })
        .collect();

    let total_duration: f64 = durations.iter().map(|krd| krd.duration.as_f64()).sum();

    Some(KeyRateProfile {
        durations,
        total_duration,
        coverage,
        total_holdings: holdings.len(),
        tenors: tenor_points.to_vec(),
    })
}

/// Calculates partial DV01s at each key rate tenor.
///
/// Partial DV01 = Market Value × Key Rate Duration × 0.0001
///
/// This gives the dollar change for a 1bp shift at each tenor.
#[must_use]
pub fn partial_dv01s(
    holdings: &[Holding],
    config: &AnalyticsConfig,
    tenors: Option<&[f64]>,
) -> Option<Vec<(f64, f64)>> {
    let profile = aggregate_key_rate_profile(holdings, config, tenors)?;

    // Calculate total market value
    let total_mv: f64 = holdings
        .iter()
        .map(|h| h.market_value().to_f64().unwrap_or(0.0))
        .sum();

    Some(
        profile
            .durations
            .iter()
            .map(|krd| {
                let partial_dv01 = total_mv * krd.duration.as_f64() * 0.0001;
                (krd.tenor, partial_dv01)
            })
            .collect(),
    )
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
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn create_krd(tenors: &[(f64, f64)]) -> KeyRateDurations {
        let durations = tenors
            .iter()
            .map(|(tenor, dur)| KeyRateDuration {
                tenor: *tenor,
                duration: Duration::from(*dur),
            })
            .collect();
        KeyRateDurations::new(durations)
    }

    fn create_holding_with_krd(
        id: &str,
        par: Decimal,
        price: Decimal,
        krd: KeyRateDurations,
    ) -> Holding {
        let mut analytics = HoldingAnalytics::new();
        analytics.key_rate_durations = Some(krd);

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
    fn test_aggregate_key_rate_profile() {
        // Two bonds with equal market value
        let krd1 = create_krd(&[(2.0, 0.5), (5.0, 1.5), (10.0, 2.0)]);
        let krd2 = create_krd(&[(2.0, 0.3), (5.0, 1.0), (10.0, 1.5)]);

        let holdings = vec![
            create_holding_with_krd("BOND1", dec!(1_000_000), dec!(100), krd1),
            create_holding_with_krd("BOND2", dec!(1_000_000), dec!(100), krd2),
        ];

        let config = AnalyticsConfig::default();
        let tenors = &[2.0, 5.0, 10.0];
        let profile = aggregate_key_rate_profile(&holdings, &config, Some(tenors)).unwrap();

        assert_eq!(profile.coverage, 2);
        assert_eq!(profile.total_holdings, 2);
        assert!((profile.coverage_pct() - 100.0).abs() < 0.01);

        // Equal MV: average KRDs
        // 2Y: (0.5 + 0.3) / 2 = 0.4
        // 5Y: (1.5 + 1.0) / 2 = 1.25
        // 10Y: (2.0 + 1.5) / 2 = 1.75
        let dur_2y = profile.at_tenor(2.0).unwrap();
        assert!((dur_2y - 0.4).abs() < 0.01);

        let dur_5y = profile.at_tenor(5.0).unwrap();
        assert!((dur_5y - 1.25).abs() < 0.01);

        let dur_10y = profile.at_tenor(10.0).unwrap();
        assert!((dur_10y - 1.75).abs() < 0.01);

        // Total: 0.4 + 1.25 + 1.75 = 3.4
        assert!((profile.total_duration - 3.4).abs() < 0.01);
    }

    #[test]
    fn test_weighted_aggregation() {
        // Bond1 has 2x the market value of Bond2
        let krd1 = create_krd(&[(5.0, 3.0)]);
        let krd2 = create_krd(&[(5.0, 6.0)]);

        let holdings = vec![
            create_holding_with_krd("BOND1", dec!(2_000_000), dec!(100), krd1), // MV = 2M
            create_holding_with_krd("BOND2", dec!(1_000_000), dec!(100), krd2), // MV = 1M
        ];

        let config = AnalyticsConfig::default();
        let profile = aggregate_key_rate_profile(&holdings, &config, Some(&[5.0])).unwrap();

        // MV weighted: (2M × 3.0 + 1M × 6.0) / 3M = 12M / 3M = 4.0
        let dur_5y = profile.at_tenor(5.0).unwrap();
        assert!((dur_5y - 4.0).abs() < 0.01);
    }

    #[test]
    fn test_duration_buckets() {
        let krd = create_krd(&[
            (0.5, 0.1),  // short
            (1.0, 0.2),  // short
            (2.0, 0.3),  // intermediate
            (5.0, 1.0),  // intermediate
            (10.0, 1.5), // intermediate
            (20.0, 2.0), // long
            (30.0, 1.0), // long
        ]);

        let holdings = vec![create_holding_with_krd(
            "BOND1",
            dec!(1_000_000),
            dec!(100),
            krd,
        )];

        let config = AnalyticsConfig::default();
        let tenors = &[0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0];
        let profile = aggregate_key_rate_profile(&holdings, &config, Some(tenors)).unwrap();

        // Short: 0.1 + 0.2 = 0.3
        assert!((profile.short_duration() - 0.3).abs() < 0.01);

        // Intermediate: 0.3 + 1.0 + 1.5 = 2.8
        assert!((profile.intermediate_duration() - 2.8).abs() < 0.01);

        // Long: 2.0 + 1.0 = 3.0
        assert!((profile.long_duration() - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_partial_dv01s() {
        let krd = create_krd(&[(5.0, 5.0), (10.0, 3.0)]);

        let holdings = vec![create_holding_with_krd(
            "BOND1",
            dec!(1_000_000),
            dec!(100),
            krd,
        )];

        let config = AnalyticsConfig::default();
        let partials = partial_dv01s(&holdings, &config, Some(&[5.0, 10.0])).unwrap();

        // MV = 1,000,000
        // 5Y partial DV01 = 1,000,000 × 5.0 × 0.0001 = 500
        // 10Y partial DV01 = 1,000,000 × 3.0 × 0.0001 = 300
        let (tenor_5y, dv01_5y) = partials
            .iter()
            .find(|(t, _)| (*t - 5.0).abs() < 0.01)
            .unwrap();
        assert!((*tenor_5y - 5.0).abs() < 0.01);
        assert!((*dv01_5y - 500.0).abs() < 1.0);

        let (tenor_10y, dv01_10y) = partials
            .iter()
            .find(|(t, _)| (*t - 10.0).abs() < 0.01)
            .unwrap();
        assert!((*tenor_10y - 10.0).abs() < 0.01);
        assert!((*dv01_10y - 300.0).abs() < 1.0);
    }

    #[test]
    fn test_no_krd_data() {
        let holding = Holding::builder()
            .id("BOND1")
            .identifiers(BondIdentifiers::new().with_ticker("TEST001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new()) // No KRD data
            .build()
            .unwrap();

        let holdings = vec![holding];
        let config = AnalyticsConfig::default();

        assert!(aggregate_key_rate_profile(&holdings, &config, None).is_none());
    }

    #[test]
    fn test_partial_coverage() {
        let krd = create_krd(&[(5.0, 2.0)]);

        let holdings = vec![
            create_holding_with_krd("BOND1", dec!(1_000_000), dec!(100), krd),
            Holding::builder()
                .id("BOND2")
                .identifiers(BondIdentifiers::new().with_ticker("TEST002"))
                .par_amount(dec!(1_000_000))
                .market_price(dec!(100))
                .analytics(HoldingAnalytics::new()) // No KRD
                .build()
                .unwrap(),
        ];

        let config = AnalyticsConfig::default();
        let profile = aggregate_key_rate_profile(&holdings, &config, Some(&[5.0])).unwrap();

        assert_eq!(profile.coverage, 1);
        assert_eq!(profile.total_holdings, 2);
        assert!((profile.coverage_pct() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_in_range() {
        let krd = create_krd(&[(1.0, 0.5), (2.0, 1.0), (5.0, 2.0), (10.0, 3.0), (30.0, 2.0)]);

        let holdings = vec![create_holding_with_krd(
            "BOND1",
            dec!(1_000_000),
            dec!(100),
            krd,
        )];

        let config = AnalyticsConfig::default();
        let tenors = &[1.0, 2.0, 5.0, 10.0, 30.0];
        let profile = aggregate_key_rate_profile(&holdings, &config, Some(tenors)).unwrap();

        // Get 2-10 year range
        let mid_range = profile.in_range(2.0, 10.0);
        assert_eq!(mid_range.len(), 3); // 2Y, 5Y, 10Y
    }
}
