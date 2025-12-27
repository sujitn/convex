//! Risk contribution analysis.
//!
//! Calculates how each holding contributes to portfolio-level risk metrics.

use crate::types::{AnalyticsConfig, Holding, RatingBucket, Sector};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Contribution of a single holding to a portfolio metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldingContribution {
    /// Holding identifier.
    pub id: String,

    /// Market value weight (0-1).
    pub weight: f64,

    /// Absolute contribution value.
    pub contribution: f64,

    /// Contribution as percentage of total (0-100).
    pub contribution_pct: f64,
}

/// Aggregated contribution for a bucket (sector, rating, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BucketContribution {
    /// Number of holdings in this bucket.
    pub count: usize,

    /// Total weight of holdings in bucket (0-1).
    pub weight: f64,

    /// Absolute contribution value.
    pub contribution: f64,

    /// Contribution as percentage of total (0-100).
    pub contribution_pct: f64,
}

/// Duration contribution analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DurationContributions {
    /// Contributions by holding, sorted by absolute contribution descending.
    pub by_holding: Vec<HoldingContribution>,

    /// Contributions by sector.
    pub by_sector: HashMap<Sector, BucketContribution>,

    /// Contributions by rating bucket.
    pub by_rating: HashMap<RatingBucket, BucketContribution>,

    /// Portfolio weighted average duration.
    pub portfolio_duration: f64,

    /// Total portfolio market value.
    pub total_market_value: Decimal,
}

impl DurationContributions {
    /// Returns the top N contributors by absolute contribution.
    #[must_use]
    pub fn top_contributors(&self, n: usize) -> Vec<&HoldingContribution> {
        self.by_holding.iter().take(n).collect()
    }

    /// Returns holdings that contribute more than the given percentage.
    #[must_use]
    pub fn large_contributors(&self, threshold_pct: f64) -> Vec<&HoldingContribution> {
        self.by_holding
            .iter()
            .filter(|c| c.contribution_pct.abs() > threshold_pct)
            .collect()
    }
}

/// DV01 contribution analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dv01Contributions {
    /// Contributions by holding, sorted by absolute contribution descending.
    pub by_holding: Vec<HoldingContribution>,

    /// Contributions by sector.
    pub by_sector: HashMap<Sector, BucketContribution>,

    /// Contributions by rating bucket.
    pub by_rating: HashMap<RatingBucket, BucketContribution>,

    /// Total portfolio DV01.
    pub total_dv01: Decimal,

    /// Total portfolio market value.
    pub total_market_value: Decimal,
}

impl Dv01Contributions {
    /// Returns the top N contributors by absolute DV01.
    #[must_use]
    pub fn top_contributors(&self, n: usize) -> Vec<&HoldingContribution> {
        self.by_holding.iter().take(n).collect()
    }
}

/// Spread contribution analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpreadContributions {
    /// Contributions by holding, sorted by absolute contribution descending.
    pub by_holding: Vec<HoldingContribution>,

    /// Contributions by sector.
    pub by_sector: HashMap<Sector, BucketContribution>,

    /// Contributions by rating bucket.
    pub by_rating: HashMap<RatingBucket, BucketContribution>,

    /// Portfolio weighted average spread.
    pub portfolio_spread: f64,

    /// Total portfolio market value.
    pub total_market_value: Decimal,
}

impl SpreadContributions {
    /// Returns the top N contributors by absolute spread contribution.
    #[must_use]
    pub fn top_contributors(&self, n: usize) -> Vec<&HoldingContribution> {
        self.by_holding.iter().take(n).collect()
    }
}

/// CS01 (credit spread sensitivity) contribution analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cs01Contributions {
    /// Contributions by holding, sorted by absolute contribution descending.
    pub by_holding: Vec<HoldingContribution>,

    /// Contributions by sector.
    pub by_sector: HashMap<Sector, BucketContribution>,

    /// Contributions by rating bucket.
    pub by_rating: HashMap<RatingBucket, BucketContribution>,

    /// Total portfolio CS01.
    pub total_cs01: Decimal,

    /// Total portfolio market value.
    pub total_market_value: Decimal,
}

/// Calculates duration contributions for each holding.
///
/// Duration contribution = weight × duration
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to analyze
/// * `config` - Analytics configuration
///
/// # Returns
///
/// Duration contribution breakdown by holding and aggregations.
///
/// # Example
///
/// ```rust,ignore
/// use convex_portfolio::contribution::duration_contributions;
///
/// let contrib = duration_contributions(&portfolio.holdings, &config);
/// println!("Portfolio duration: {:.2}", contrib.portfolio_duration);
///
/// for c in contrib.top_contributors(5) {
///     println!("{}: {:.2}%", c.id, c.contribution_pct);
/// }
/// ```
#[must_use]
pub fn duration_contributions(
    holdings: &[Holding],
    _config: &AnalyticsConfig,
) -> DurationContributions {
    if holdings.is_empty() {
        return DurationContributions {
            by_holding: vec![],
            by_sector: HashMap::new(),
            by_rating: HashMap::new(),
            portfolio_duration: 0.0,
            total_market_value: Decimal::ZERO,
        };
    }

    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();

    if total_mv.is_zero() {
        return DurationContributions {
            by_holding: vec![],
            by_sector: HashMap::new(),
            by_rating: HashMap::new(),
            portfolio_duration: 0.0,
            total_market_value: Decimal::ZERO,
        };
    }

    // Calculate individual contributions
    let mut contributions: Vec<HoldingContribution> = Vec::with_capacity(holdings.len());
    let mut total_contribution = 0.0;

    for h in holdings {
        if let Some(duration) = h.analytics.best_duration() {
            let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
            let total_mv_f: f64 = total_mv.try_into().unwrap_or(1.0);
            let weight = mv / total_mv_f;
            let contribution = weight * duration;

            contributions.push(HoldingContribution {
                id: h.id.clone(),
                weight,
                contribution,
                contribution_pct: 0.0, // Will be calculated after we know total
            });

            total_contribution += contribution;
        }
    }

    // Calculate contribution percentages
    if total_contribution.abs() > f64::EPSILON {
        for c in &mut contributions {
            c.contribution_pct = (c.contribution / total_contribution) * 100.0;
        }
    }

    // Sort by absolute contribution descending
    contributions.sort_by(|a, b| {
        b.contribution
            .abs()
            .partial_cmp(&a.contribution.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Aggregate by sector
    let by_sector = aggregate_by_sector(holdings, total_mv, total_contribution, |h| {
        h.analytics.best_duration()
    });

    // Aggregate by rating
    let by_rating = aggregate_by_rating(holdings, total_mv, total_contribution, |h| {
        h.analytics.best_duration()
    });

    DurationContributions {
        by_holding: contributions,
        by_sector,
        by_rating,
        portfolio_duration: total_contribution,
        total_market_value: total_mv,
    }
}

/// Calculates DV01 contributions for each holding.
///
/// DV01 contribution is the absolute DV01 value for each holding.
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to analyze
/// * `config` - Analytics configuration
///
/// # Returns
///
/// DV01 contribution breakdown by holding and aggregations.
#[must_use]
pub fn dv01_contributions(holdings: &[Holding], _config: &AnalyticsConfig) -> Dv01Contributions {
    if holdings.is_empty() {
        return Dv01Contributions {
            by_holding: vec![],
            by_sector: HashMap::new(),
            by_rating: HashMap::new(),
            total_dv01: Decimal::ZERO,
            total_market_value: Decimal::ZERO,
        };
    }

    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();
    let mut total_dv01 = Decimal::ZERO;

    // Calculate individual contributions
    let mut contributions: Vec<HoldingContribution> = Vec::with_capacity(holdings.len());

    for h in holdings {
        if let Some(dv01) = h.total_dv01() {
            let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
            let total_mv_f: f64 = total_mv.try_into().unwrap_or(1.0);
            let weight = mv / total_mv_f;
            let dv01_f: f64 = dv01.try_into().unwrap_or(0.0);

            contributions.push(HoldingContribution {
                id: h.id.clone(),
                weight,
                contribution: dv01_f,
                contribution_pct: 0.0,
            });

            total_dv01 += dv01;
        }
    }

    // Calculate contribution percentages
    let total_dv01_f: f64 = total_dv01.try_into().unwrap_or(0.0);
    if total_dv01_f.abs() > f64::EPSILON {
        for c in &mut contributions {
            c.contribution_pct = (c.contribution / total_dv01_f) * 100.0;
        }
    }

    // Sort by absolute contribution descending
    contributions.sort_by(|a, b| {
        b.contribution
            .abs()
            .partial_cmp(&a.contribution.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Aggregate by sector
    let by_sector = aggregate_dv01_by_sector(holdings, total_dv01);

    // Aggregate by rating
    let by_rating = aggregate_dv01_by_rating(holdings, total_dv01);

    Dv01Contributions {
        by_holding: contributions,
        by_sector,
        by_rating,
        total_dv01,
        total_market_value: total_mv,
    }
}

/// Calculates spread contributions for each holding.
///
/// Spread contribution = weight × spread (using best available spread).
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to analyze
/// * `config` - Analytics configuration
///
/// # Returns
///
/// Spread contribution breakdown by holding and aggregations.
#[must_use]
pub fn spread_contributions(
    holdings: &[Holding],
    _config: &AnalyticsConfig,
) -> SpreadContributions {
    if holdings.is_empty() {
        return SpreadContributions {
            by_holding: vec![],
            by_sector: HashMap::new(),
            by_rating: HashMap::new(),
            portfolio_spread: 0.0,
            total_market_value: Decimal::ZERO,
        };
    }

    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();

    if total_mv.is_zero() {
        return SpreadContributions {
            by_holding: vec![],
            by_sector: HashMap::new(),
            by_rating: HashMap::new(),
            portfolio_spread: 0.0,
            total_market_value: Decimal::ZERO,
        };
    }

    // Calculate individual contributions
    let mut contributions: Vec<HoldingContribution> = Vec::with_capacity(holdings.len());
    let mut total_contribution = 0.0;

    for h in holdings {
        if let Some(spread) = h.analytics.best_spread() {
            let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
            let total_mv_f: f64 = total_mv.try_into().unwrap_or(1.0);
            let weight = mv / total_mv_f;
            let contribution = weight * spread;

            contributions.push(HoldingContribution {
                id: h.id.clone(),
                weight,
                contribution,
                contribution_pct: 0.0,
            });

            total_contribution += contribution;
        }
    }

    // Calculate contribution percentages
    if total_contribution.abs() > f64::EPSILON {
        for c in &mut contributions {
            c.contribution_pct = (c.contribution / total_contribution) * 100.0;
        }
    }

    // Sort by absolute contribution descending
    contributions.sort_by(|a, b| {
        b.contribution
            .abs()
            .partial_cmp(&a.contribution.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Aggregate by sector
    let by_sector = aggregate_by_sector(holdings, total_mv, total_contribution, |h| {
        h.analytics.best_spread()
    });

    // Aggregate by rating
    let by_rating = aggregate_by_rating(holdings, total_mv, total_contribution, |h| {
        h.analytics.best_spread()
    });

    SpreadContributions {
        by_holding: contributions,
        by_sector,
        by_rating,
        portfolio_spread: total_contribution,
        total_market_value: total_mv,
    }
}

/// Calculates CS01 contributions for each holding.
///
/// CS01 contribution is the absolute credit spread sensitivity for each holding.
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to analyze
/// * `config` - Analytics configuration
///
/// # Returns
///
/// CS01 contribution breakdown by holding and aggregations.
#[must_use]
pub fn cs01_contributions(holdings: &[Holding], _config: &AnalyticsConfig) -> Cs01Contributions {
    if holdings.is_empty() {
        return Cs01Contributions {
            by_holding: vec![],
            by_sector: HashMap::new(),
            by_rating: HashMap::new(),
            total_cs01: Decimal::ZERO,
            total_market_value: Decimal::ZERO,
        };
    }

    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();
    let mut total_cs01 = Decimal::ZERO;

    // Calculate individual contributions
    let mut contributions: Vec<HoldingContribution> = Vec::with_capacity(holdings.len());

    for h in holdings {
        if let Some(cs01) = h.analytics.cs01 {
            // CS01 per par × par amount / 100
            let cs01_total = Decimal::from_f64_retain(cs01).unwrap_or(Decimal::ZERO) * h.par_amount
                / Decimal::ONE_HUNDRED
                * h.fx_rate;

            let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
            let total_mv_f: f64 = total_mv.try_into().unwrap_or(1.0);
            let weight = mv / total_mv_f;
            let cs01_f: f64 = cs01_total.try_into().unwrap_or(0.0);

            contributions.push(HoldingContribution {
                id: h.id.clone(),
                weight,
                contribution: cs01_f,
                contribution_pct: 0.0,
            });

            total_cs01 += cs01_total;
        }
    }

    // Calculate contribution percentages
    let total_cs01_f: f64 = total_cs01.try_into().unwrap_or(0.0);
    if total_cs01_f.abs() > f64::EPSILON {
        for c in &mut contributions {
            c.contribution_pct = (c.contribution / total_cs01_f) * 100.0;
        }
    }

    // Sort by absolute contribution descending
    contributions.sort_by(|a, b| {
        b.contribution
            .abs()
            .partial_cmp(&a.contribution.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Aggregate by sector
    let by_sector = aggregate_cs01_by_sector(holdings, total_cs01);

    // Aggregate by rating
    let by_rating = aggregate_cs01_by_rating(holdings, total_cs01);

    Cs01Contributions {
        by_holding: contributions,
        by_sector,
        by_rating,
        total_cs01,
        total_market_value: total_mv,
    }
}

/// Helper to aggregate contributions by sector.
fn aggregate_by_sector<F>(
    holdings: &[Holding],
    total_mv: Decimal,
    total_contribution: f64,
    metric_fn: F,
) -> HashMap<Sector, BucketContribution>
where
    F: Fn(&Holding) -> Option<f64>,
{
    let mut by_sector: HashMap<Sector, BucketContribution> = HashMap::new();
    let total_mv_f: f64 = total_mv.try_into().unwrap_or(1.0);

    for h in holdings {
        if let Some(sector) = h.classification.sector.composite {
            if let Some(value) = metric_fn(h) {
                let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
                let weight = mv / total_mv_f;
                let contribution = weight * value;

                let entry = by_sector.entry(sector).or_default();
                entry.count += 1;
                entry.weight += weight;
                entry.contribution += contribution;
            }
        }
    }

    // Calculate percentages
    if total_contribution.abs() > f64::EPSILON {
        for contrib in by_sector.values_mut() {
            contrib.contribution_pct = (contrib.contribution / total_contribution) * 100.0;
        }
    }

    by_sector
}

/// Helper to aggregate contributions by rating.
fn aggregate_by_rating<F>(
    holdings: &[Holding],
    total_mv: Decimal,
    total_contribution: f64,
    metric_fn: F,
) -> HashMap<RatingBucket, BucketContribution>
where
    F: Fn(&Holding) -> Option<f64>,
{
    let mut by_rating: HashMap<RatingBucket, BucketContribution> = HashMap::new();
    let total_mv_f: f64 = total_mv.try_into().unwrap_or(1.0);

    for h in holdings {
        if let Some(rating) = h.classification.rating.composite {
            if let Some(value) = metric_fn(h) {
                let bucket = rating.bucket();
                let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
                let weight = mv / total_mv_f;
                let contribution = weight * value;

                let entry = by_rating.entry(bucket).or_default();
                entry.count += 1;
                entry.weight += weight;
                entry.contribution += contribution;
            }
        }
    }

    // Calculate percentages
    if total_contribution.abs() > f64::EPSILON {
        for contrib in by_rating.values_mut() {
            contrib.contribution_pct = (contrib.contribution / total_contribution) * 100.0;
        }
    }

    by_rating
}

/// Helper to aggregate DV01 by sector.
fn aggregate_dv01_by_sector(
    holdings: &[Holding],
    total_dv01: Decimal,
) -> HashMap<Sector, BucketContribution> {
    let mut by_sector: HashMap<Sector, BucketContribution> = HashMap::new();
    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();
    let total_mv_f: f64 = total_mv.try_into().unwrap_or(1.0);
    let total_dv01_f: f64 = total_dv01.try_into().unwrap_or(0.0);

    for h in holdings {
        if let Some(sector) = h.classification.sector.composite {
            if let Some(dv01) = h.total_dv01() {
                let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
                let weight = mv / total_mv_f;
                let dv01_f: f64 = dv01.try_into().unwrap_or(0.0);

                let entry = by_sector.entry(sector).or_default();
                entry.count += 1;
                entry.weight += weight;
                entry.contribution += dv01_f;
            }
        }
    }

    // Calculate percentages
    if total_dv01_f.abs() > f64::EPSILON {
        for contrib in by_sector.values_mut() {
            contrib.contribution_pct = (contrib.contribution / total_dv01_f) * 100.0;
        }
    }

    by_sector
}

/// Helper to aggregate DV01 by rating.
fn aggregate_dv01_by_rating(
    holdings: &[Holding],
    total_dv01: Decimal,
) -> HashMap<RatingBucket, BucketContribution> {
    let mut by_rating: HashMap<RatingBucket, BucketContribution> = HashMap::new();
    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();
    let total_mv_f: f64 = total_mv.try_into().unwrap_or(1.0);
    let total_dv01_f: f64 = total_dv01.try_into().unwrap_or(0.0);

    for h in holdings {
        if let Some(rating) = h.classification.rating.composite {
            if let Some(dv01) = h.total_dv01() {
                let bucket = rating.bucket();
                let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
                let weight = mv / total_mv_f;
                let dv01_f: f64 = dv01.try_into().unwrap_or(0.0);

                let entry = by_rating.entry(bucket).or_default();
                entry.count += 1;
                entry.weight += weight;
                entry.contribution += dv01_f;
            }
        }
    }

    // Calculate percentages
    if total_dv01_f.abs() > f64::EPSILON {
        for contrib in by_rating.values_mut() {
            contrib.contribution_pct = (contrib.contribution / total_dv01_f) * 100.0;
        }
    }

    by_rating
}

/// Helper to aggregate CS01 by sector.
fn aggregate_cs01_by_sector(
    holdings: &[Holding],
    total_cs01: Decimal,
) -> HashMap<Sector, BucketContribution> {
    let mut by_sector: HashMap<Sector, BucketContribution> = HashMap::new();
    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();
    let total_mv_f: f64 = total_mv.try_into().unwrap_or(1.0);
    let total_cs01_f: f64 = total_cs01.try_into().unwrap_or(0.0);

    for h in holdings {
        if let Some(sector) = h.classification.sector.composite {
            if let Some(cs01) = h.analytics.cs01 {
                let cs01_total = Decimal::from_f64_retain(cs01).unwrap_or(Decimal::ZERO)
                    * h.par_amount
                    / Decimal::ONE_HUNDRED
                    * h.fx_rate;

                let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
                let weight = mv / total_mv_f;
                let cs01_f: f64 = cs01_total.try_into().unwrap_or(0.0);

                let entry = by_sector.entry(sector).or_default();
                entry.count += 1;
                entry.weight += weight;
                entry.contribution += cs01_f;
            }
        }
    }

    // Calculate percentages
    if total_cs01_f.abs() > f64::EPSILON {
        for contrib in by_sector.values_mut() {
            contrib.contribution_pct = (contrib.contribution / total_cs01_f) * 100.0;
        }
    }

    by_sector
}

/// Helper to aggregate CS01 by rating.
fn aggregate_cs01_by_rating(
    holdings: &[Holding],
    total_cs01: Decimal,
) -> HashMap<RatingBucket, BucketContribution> {
    let mut by_rating: HashMap<RatingBucket, BucketContribution> = HashMap::new();
    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();
    let total_mv_f: f64 = total_mv.try_into().unwrap_or(1.0);
    let total_cs01_f: f64 = total_cs01.try_into().unwrap_or(0.0);

    for h in holdings {
        if let Some(rating) = h.classification.rating.composite {
            if let Some(cs01) = h.analytics.cs01 {
                let cs01_total = Decimal::from_f64_retain(cs01).unwrap_or(Decimal::ZERO)
                    * h.par_amount
                    / Decimal::ONE_HUNDRED
                    * h.fx_rate;

                let bucket = rating.bucket();
                let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
                let weight = mv / total_mv_f;
                let cs01_f: f64 = cs01_total.try_into().unwrap_or(0.0);

                let entry = by_rating.entry(bucket).or_default();
                entry.count += 1;
                entry.weight += weight;
                entry.contribution += cs01_f;
            }
        }
    }

    // Calculate percentages
    if total_cs01_f.abs() > f64::EPSILON {
        for contrib in by_rating.values_mut() {
            contrib.contribution_pct = (contrib.contribution / total_cs01_f) * 100.0;
        }
    }

    by_rating
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
        dv01: f64,
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
                    .with_dv01(dv01)
                    .with_z_spread(spread),
            )
            .build()
            .unwrap()
    }

    #[test]
    fn test_duration_contributions_empty() {
        let holdings: Vec<Holding> = vec![];
        let config = AnalyticsConfig::default();

        let contrib = duration_contributions(&holdings, &config);

        assert!(contrib.by_holding.is_empty());
        assert_eq!(contrib.portfolio_duration, 0.0);
    }

    #[test]
    fn test_duration_contributions_single() {
        let holdings = vec![create_test_holding(
            "H1",
            dec!(100),
            5.0,
            0.05,
            100.0,
            None,
            None,
        )];
        let config = AnalyticsConfig::default();

        let contrib = duration_contributions(&holdings, &config);

        assert_eq!(contrib.by_holding.len(), 1);
        assert!((contrib.portfolio_duration - 5.0).abs() < 0.01);
        assert!((contrib.by_holding[0].contribution_pct - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_duration_contributions_multiple() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), 4.0, 0.04, 80.0, None, None),
            create_test_holding("H2", dec!(100), 6.0, 0.06, 120.0, None, None),
        ];
        let config = AnalyticsConfig::default();

        let contrib = duration_contributions(&holdings, &config);

        assert_eq!(contrib.by_holding.len(), 2);
        // Portfolio duration = 0.5 * 4 + 0.5 * 6 = 5.0
        assert!((contrib.portfolio_duration - 5.0).abs() < 0.01);
        // H1: 0.5 * 4 = 2 → 40% of portfolio duration
        // H2: 0.5 * 6 = 3 → 60% of portfolio duration
        // After sorting by absolute contribution desc, H2 (60%) comes first
        assert_eq!(contrib.by_holding[0].id, "H2");
        assert!((contrib.by_holding[0].contribution_pct - 60.0).abs() < 1.0);
        assert!((contrib.by_holding[1].contribution_pct - 40.0).abs() < 1.0);
    }

    #[test]
    fn test_duration_by_sector() {
        let holdings = vec![
            create_test_holding(
                "H1",
                dec!(100),
                4.0,
                0.04,
                80.0,
                Some(Sector::Government),
                None,
            ),
            create_test_holding(
                "H2",
                dec!(100),
                6.0,
                0.06,
                120.0,
                Some(Sector::Corporate),
                None,
            ),
        ];
        let config = AnalyticsConfig::default();

        let contrib = duration_contributions(&holdings, &config);

        assert_eq!(contrib.by_sector.len(), 2);
        let govt = contrib.by_sector.get(&Sector::Government).unwrap();
        let corp = contrib.by_sector.get(&Sector::Corporate).unwrap();

        assert_eq!(govt.count, 1);
        assert_eq!(corp.count, 1);
        // Government: 0.5 * 4 = 2 / 5 = 40%
        assert!((govt.contribution_pct - 40.0).abs() < 0.1);
        // Corporate: 0.5 * 6 = 3 / 5 = 60%
        assert!((corp.contribution_pct - 60.0).abs() < 0.1);
    }

    #[test]
    fn test_duration_by_rating() {
        let holdings = vec![
            create_test_holding(
                "H1",
                dec!(100),
                4.0,
                0.04,
                80.0,
                None,
                Some(CreditRating::AAA),
            ),
            create_test_holding(
                "H2",
                dec!(100),
                6.0,
                0.06,
                120.0,
                None,
                Some(CreditRating::BBB),
            ),
        ];
        let config = AnalyticsConfig::default();

        let contrib = duration_contributions(&holdings, &config);

        assert_eq!(contrib.by_rating.len(), 2);
        let aaa = contrib.by_rating.get(&RatingBucket::AAA).unwrap();
        let bbb = contrib.by_rating.get(&RatingBucket::BBB).unwrap();

        assert_eq!(aaa.count, 1);
        assert_eq!(bbb.count, 1);
    }

    #[test]
    fn test_dv01_contributions() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), 4.0, 0.04, 80.0, None, None),
            create_test_holding("H2", dec!(100), 6.0, 0.06, 120.0, None, None),
        ];
        let config = AnalyticsConfig::default();

        let contrib = dv01_contributions(&holdings, &config);

        assert_eq!(contrib.by_holding.len(), 2);
        // DV01 for H1 = 0.04 * 1,000,000 / 100 = 400
        // DV01 for H2 = 0.06 * 1,000,000 / 100 = 600
        // Total = 1000
        assert!((contrib.total_dv01 - dec!(1000)).abs() < dec!(1));

        // Contributions: H1 = 40%, H2 = 60%
        let h2_contrib = contrib.by_holding.iter().find(|c| c.id == "H2").unwrap();
        assert!((h2_contrib.contribution_pct - 60.0).abs() < 0.1);
    }

    #[test]
    fn test_spread_contributions() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), 4.0, 0.04, 80.0, None, None),
            create_test_holding("H2", dec!(100), 6.0, 0.06, 120.0, None, None),
        ];
        let config = AnalyticsConfig::default();

        let contrib = spread_contributions(&holdings, &config);

        assert_eq!(contrib.by_holding.len(), 2);
        // Portfolio spread = 0.5 * 80 + 0.5 * 120 = 100
        assert!((contrib.portfolio_spread - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_top_contributors() {
        let holdings = vec![
            create_test_holding("H1", dec!(50), 2.0, 0.02, 50.0, None, None),
            create_test_holding("H2", dec!(100), 8.0, 0.08, 150.0, None, None),
            create_test_holding("H3", dec!(50), 4.0, 0.04, 100.0, None, None),
        ];
        let config = AnalyticsConfig::default();

        let contrib = duration_contributions(&holdings, &config);
        let top = contrib.top_contributors(2);

        assert_eq!(top.len(), 2);
        // H2 should be first (highest contribution)
        assert_eq!(top[0].id, "H2");
    }

    #[test]
    fn test_large_contributors() {
        let holdings = vec![
            create_test_holding("H1", dec!(50), 2.0, 0.02, 50.0, None, None),
            create_test_holding("H2", dec!(100), 8.0, 0.08, 150.0, None, None),
            create_test_holding("H3", dec!(50), 4.0, 0.04, 100.0, None, None),
        ];
        let config = AnalyticsConfig::default();

        let contrib = duration_contributions(&holdings, &config);
        let large = contrib.large_contributors(50.0);

        // Only H2 contributes > 50%
        assert_eq!(large.len(), 1);
        assert_eq!(large[0].id, "H2");
    }
}
