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
    pub weight: Option<f64>,

    /// Absolute contribution value.
    pub contribution: f64,

    /// Contribution as percentage of total (0-100).
    pub contribution_pct: Option<f64>,
}

/// Aggregated contribution for a bucket (sector, rating, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BucketContribution {
    /// Number of holdings in this bucket.
    pub count: usize,

    /// Total weight of holdings in bucket (0-1).
    pub weight: Option<f64>,

    /// Absolute contribution value.
    pub contribution: f64,

    /// Contribution as percentage of total (0-100).
    pub contribution_pct: Option<f64>,
}

/// Duration contribution analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DurationContributions {
    /// Exact supplied sensitivity family selected by the caller.
    pub measure: DurationMeasure,
    /// Contributions by holding, sorted by absolute contribution descending.
    pub by_holding: Vec<HoldingContribution>,

    /// Contributions by sector.
    pub by_sector: HashMap<Sector, BucketContribution>,

    /// Contributions by rating bucket.
    pub by_rating: HashMap<RatingBucket, BucketContribution>,

    /// Portfolio weighted average duration.
    pub portfolio_duration: f64,

    /// Total portfolio market value.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    pub total_market_value: Decimal,
}

impl DurationContributions {
    /// Returns the top N contributors by absolute contribution.
    pub fn top_contributors(&self, n: usize) -> Vec<&HoldingContribution> {
        self.by_holding.iter().take(n).collect()
    }

    /// Returns holdings that contribute more than the given percentage.
    pub fn large_contributors(&self, threshold_pct: f64) -> Vec<&HoldingContribution> {
        self.by_holding
            .iter()
            .filter(|c| c.contribution_pct.is_some_and(|v| v.abs() > threshold_pct))
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
    #[serde(with = "convex_core::contracts::decimal_serde")]
    pub total_dv01: Decimal,

    /// Total portfolio market value.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    pub total_market_value: Decimal,
}

impl Dv01Contributions {
    /// Returns the top N contributors by absolute DV01.
    pub fn top_contributors(&self, n: usize) -> Vec<&HoldingContribution> {
        self.by_holding.iter().take(n).collect()
    }
}

/// Spread contribution analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpreadContributions {
    /// Exact supplied spread family selected by the caller.
    pub measure: SpreadMeasure,
    /// Contributions by holding, sorted by absolute contribution descending.
    pub by_holding: Vec<HoldingContribution>,

    /// Contributions by sector.
    pub by_sector: HashMap<Sector, BucketContribution>,

    /// Contributions by rating bucket.
    pub by_rating: HashMap<RatingBucket, BucketContribution>,

    /// Portfolio weighted average spread.
    pub portfolio_spread: f64,

    /// Total portfolio market value.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    pub total_market_value: Decimal,
}

impl SpreadContributions {
    /// Returns the top N contributors by absolute spread contribution.
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
    #[serde(with = "convex_core::contracts::decimal_serde")]
    pub total_cs01: Decimal,

    /// Total portfolio market value.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    pub total_market_value: Decimal,
}

/// Explicit duration family; no effective/modified substitution is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurationMeasure {
    /// Supplied modified yield duration.
    Modified,
    /// Supplied effective curve duration.
    Effective,
}
impl DurationMeasure {
    fn value(self, h: &Holding) -> Option<f64> {
        match self {
            Self::Modified => h.analytics.modified_duration,
            Self::Effective => h.analytics.effective_duration,
        }
    }
}
/// Explicit supplied spread family; absent measures cannot use a different spread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpreadMeasure {
    /// Supplied Z-spread.
    ZSpread,
    /// Supplied option-adjusted spread.
    Oas,
    /// Supplied government spread.
    GSpread,
    /// Supplied interpolated swap spread.
    ISpread,
    /// Supplied asset-swap spread.
    Asw,
}
impl SpreadMeasure {
    fn value(self, h: &Holding) -> Option<f64> {
        match self {
            Self::ZSpread => h.analytics.z_spread,
            Self::Oas => h.analytics.oas,
            Self::GSpread => h.analytics.g_spread,
            Self::ISpread => h.analytics.i_spread,
            Self::Asw => h.analytics.asw,
        }
    }
}

use crate::analytics::metrics;
use crate::{PortfolioError, PortfolioResult};
use convex_math::decimal::BoundedDecimal;
use std::hash::Hash;
struct Row<'a> {
    holding: &'a Holding,
    weight: BoundedDecimal,
    value: BoundedDecimal,
}
struct Allocation {
    by_holding: Vec<HoldingContribution>,
    by_sector: HashMap<Sector, BucketContribution>,
    by_rating: HashMap<RatingBucket, BucketContribution>,
    total: BoundedDecimal,
    denominator: BoundedDecimal,
    market: Decimal,
}
fn displayed_ratio(n: &BoundedDecimal, d: &BoundedDecimal) -> PortfolioResult<Option<f64>> {
    metrics::ratio_as_f64(n, d)
}
fn required_ratio(n: &BoundedDecimal, d: &BoundedDecimal) -> PortfolioResult<f64> {
    metrics::ratio_as_f64(n, d)?.ok_or_else(|| {
        PortfolioError::invalid_portfolio(
            "requested normalized contribution is undefined at zero weight",
        )
    })
}
fn grouped<K: Copy + Eq + Hash>(
    rows: &[Row<'_>],
    key: impl Fn(&Holding) -> Option<K>,
    weights: &BoundedDecimal,
    total: &BoundedDecimal,
    denominator: &BoundedDecimal,
) -> PortfolioResult<HashMap<K, BucketContribution>> {
    let mut sums = HashMap::<K, (usize, BoundedDecimal, BoundedDecimal)>::new();
    for row in rows {
        if let Some(key) = key(row.holding) {
            let group = sums
                .entry(key)
                .or_insert_with(|| (0, BoundedDecimal::zero(), BoundedDecimal::zero()));
            group.0 += 1;
            group.1.add(&row.weight)?;
            group.2.add(&row.value)?;
        }
    }
    sums.into_iter()
        .map(|(key, (count, weight, value))| {
            Ok((
                key,
                BucketContribution {
                    count,
                    weight: displayed_ratio(&weight, weights)?,
                    contribution: required_ratio(&value, denominator)?,
                    contribution_pct: displayed_ratio(
                        &value.multiply(Decimal::ONE_HUNDRED)?,
                        total,
                    )?,
                },
            ))
        })
        .collect()
}
fn allocate(
    holdings: &[Holding],
    config: &AnalyticsConfig,
    field: &str,
    get: impl Fn(&Holding) -> Option<f64>,
    currency: bool,
) -> PortfolioResult<Allocation> {
    if holdings.is_empty() {
        return Err(PortfolioError::EmptyPortfolio);
    }
    metrics::amount_rows(holdings.iter())?;
    let market = metrics::clean_exact(holdings.iter())?;
    let mut weight_sum = BoundedDecimal::zero();
    let mut total = BoundedDecimal::zero();
    let mut rows = Vec::with_capacity(holdings.len());
    for holding in holdings {
        let scalar = get(holding).ok_or_else(|| {
            PortfolioError::invalid_holding(
                &holding.id,
                format!("required supplied {field} is absent"),
            )
        })?;
        let scalar = metrics::scalar(scalar, &holding.id, field)?;
        let weight = holding.exact_weight(if currency {
            crate::WeightingMethod::MarketValue
        } else {
            config.weighting
        })?;
        let value = if currency {
            BoundedDecimal::decimal(scalar)
                .multiply(holding.par_amount)?
                .multiply(holding.fx_rate)?
                .divide_power_of_ten(2)?
        } else {
            weight.multiply(scalar)?
        };
        weight_sum.add(&weight)?;
        total.add(&value)?;
        rows.push(Row {
            holding,
            weight,
            value,
        });
    }
    let denominator = if currency {
        BoundedDecimal::decimal(Decimal::ONE)
    } else {
        weight_sum.clone()
    };
    required_ratio(&total, &denominator)?;
    let mut by_holding = rows
        .iter()
        .map(|row| {
            Ok(HoldingContribution {
                id: row.holding.id.clone(),
                weight: displayed_ratio(&row.weight, &weight_sum)?,
                contribution: required_ratio(&row.value, &denominator)?,
                contribution_pct: displayed_ratio(
                    &row.value.multiply(Decimal::ONE_HUNDRED)?,
                    &total,
                )?,
            })
        })
        .collect::<PortfolioResult<Vec<_>>>()?;
    by_holding.sort_by(|a, b| b.contribution.abs().total_cmp(&a.contribution.abs()));
    let by_sector = grouped(
        &rows,
        |h| h.classification.sector.composite,
        &weight_sum,
        &total,
        &denominator,
    )?;
    let by_rating = grouped(
        &rows,
        |h| h.classification.rating.composite.map(|r| r.bucket()),
        &weight_sum,
        &total,
        &denominator,
    )?;
    Ok(Allocation {
        by_holding,
        by_sector,
        by_rating,
        total,
        denominator,
        market: market.finish()?,
    })
}
/// Complete configured-weight duration allocation in the explicitly selected family.
pub fn duration_contributions(
    holdings: &[Holding],
    config: &AnalyticsConfig,
    measure: DurationMeasure,
) -> PortfolioResult<DurationContributions> {
    let a = allocate(
        holdings,
        config,
        "selected duration",
        |h| measure.value(h),
        false,
    )?;
    Ok(DurationContributions {
        measure,
        by_holding: a.by_holding,
        by_sector: a.by_sector,
        by_rating: a.by_rating,
        portfolio_duration: required_ratio(&a.total, &a.denominator)?,
        total_market_value: a.market,
    })
}
/// Complete signed currency DV01 allocation; missing DV01 is an error, not zero.
pub fn dv01_contributions(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> PortfolioResult<Dv01Contributions> {
    let a = allocate(holdings, config, "dv01", |h| h.analytics.dv01, true)?;
    Ok(Dv01Contributions {
        by_holding: a.by_holding,
        by_sector: a.by_sector,
        by_rating: a.by_rating,
        total_dv01: a.total.finish()?,
        total_market_value: a.market,
    })
}
/// Complete configured-weight spread allocation in the explicitly selected family.
pub fn spread_contributions(
    holdings: &[Holding],
    config: &AnalyticsConfig,
    measure: SpreadMeasure,
) -> PortfolioResult<SpreadContributions> {
    let a = allocate(
        holdings,
        config,
        "selected spread",
        |h| measure.value(h),
        false,
    )?;
    Ok(SpreadContributions {
        measure,
        by_holding: a.by_holding,
        by_sector: a.by_sector,
        by_rating: a.by_rating,
        portfolio_spread: required_ratio(&a.total, &a.denominator)?,
        total_market_value: a.market,
    })
}
/// Complete signed currency CS01 allocation; missing CS01 is an error, not zero.
pub fn cs01_contributions(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> PortfolioResult<Cs01Contributions> {
    let a = allocate(holdings, config, "cs01", |h| h.analytics.cs01, true)?;
    Ok(Cs01Contributions {
        by_holding: a.by_holding,
        by_sector: a.by_sector,
        by_rating: a.by_rating,
        total_cs01: a.total.finish()?,
        total_market_value: a.market,
    })
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

        let contrib = duration_contributions(
            &holdings,
            &config,
            crate::contribution::DurationMeasure::Modified,
        )
        .unwrap_err();

        assert!(matches!(contrib, crate::PortfolioError::EmptyPortfolio));
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

        let contrib = duration_contributions(
            &holdings,
            &config,
            crate::contribution::DurationMeasure::Modified,
        )
        .unwrap();

        assert_eq!(contrib.by_holding.len(), 1);
        assert!((contrib.portfolio_duration - 5.0).abs() < 0.01);
        assert!((contrib.by_holding[0].contribution_pct.unwrap() - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_duration_contributions_multiple() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), 4.0, 0.04, 80.0, None, None),
            create_test_holding("H2", dec!(100), 6.0, 0.06, 120.0, None, None),
        ];
        let config = AnalyticsConfig::default();

        let contrib = duration_contributions(
            &holdings,
            &config,
            crate::contribution::DurationMeasure::Modified,
        )
        .unwrap();

        assert_eq!(contrib.by_holding.len(), 2);
        // Portfolio duration = 0.5 * 4 + 0.5 * 6 = 5.0
        assert!((contrib.portfolio_duration - 5.0).abs() < 0.01);
        // H1: 0.5 * 4 = 2 → 40% of portfolio duration
        // H2: 0.5 * 6 = 3 → 60% of portfolio duration
        // After sorting by absolute contribution desc, H2 (60%) comes first
        assert_eq!(contrib.by_holding[0].id, "H2");
        assert!((contrib.by_holding[0].contribution_pct.unwrap() - 60.0).abs() < 1.0);
        assert!((contrib.by_holding[1].contribution_pct.unwrap() - 40.0).abs() < 1.0);
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

        let contrib = duration_contributions(
            &holdings,
            &config,
            crate::contribution::DurationMeasure::Modified,
        )
        .unwrap();

        assert_eq!(contrib.by_sector.len(), 2);
        let govt = contrib.by_sector.get(&Sector::Government).unwrap();
        let corp = contrib.by_sector.get(&Sector::Corporate).unwrap();

        assert_eq!(govt.count, 1);
        assert_eq!(corp.count, 1);
        // Government: 0.5 * 4 = 2 / 5 = 40%
        assert!((govt.contribution_pct.unwrap() - 40.0).abs() < 0.1);
        // Corporate: 0.5 * 6 = 3 / 5 = 60%
        assert!((corp.contribution_pct.unwrap() - 60.0).abs() < 0.1);
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

        let contrib = duration_contributions(
            &holdings,
            &config,
            crate::contribution::DurationMeasure::Modified,
        )
        .unwrap();

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

        let contrib = dv01_contributions(&holdings, &config).unwrap();

        assert_eq!(contrib.by_holding.len(), 2);
        // DV01 for H1 = 0.04 * 1,000,000 / 100 = 400
        // DV01 for H2 = 0.06 * 1,000,000 / 100 = 600
        // Total = 1000
        assert!((contrib.total_dv01 - dec!(1000)).abs() < dec!(1));

        // Contributions: H1 = 40%, H2 = 60%
        let h2_contrib = contrib.by_holding.iter().find(|c| c.id == "H2").unwrap();
        assert!((h2_contrib.contribution_pct.unwrap() - 60.0).abs() < 0.1);
    }

    #[test]
    fn test_spread_contributions() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), 4.0, 0.04, 80.0, None, None),
            create_test_holding("H2", dec!(100), 6.0, 0.06, 120.0, None, None),
        ];
        let config = AnalyticsConfig::default();

        let contrib = spread_contributions(
            &holdings,
            &config,
            crate::contribution::SpreadMeasure::ZSpread,
        )
        .unwrap();

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

        let contrib = duration_contributions(
            &holdings,
            &config,
            crate::contribution::DurationMeasure::Modified,
        )
        .unwrap();
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

        let contrib = duration_contributions(
            &holdings,
            &config,
            crate::contribution::DurationMeasure::Modified,
        )
        .unwrap();
        let large = contrib.large_contributors(50.0);

        // Only H2 contributes > 50%
        assert_eq!(large.len(), 1);
        assert_eq!(large[0].id, "H2");
    }
}
