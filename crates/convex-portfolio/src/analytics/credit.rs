//! Credit quality analytics for portfolios.
//!
//! Provides credit quality metrics and analysis.

use crate::bucketing::{bucket_by_rating, RatingDistribution};
use crate::types::{AnalyticsConfig, CreditRating, Holding, RatingBucket};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Credit quality metrics for a portfolio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditQualityMetrics {
    /// Distribution by rating.
    pub rating_distribution: RatingDistribution,

    /// Weighted average rating (1=AAA, 22=D).
    pub average_rating_score: Option<f64>,

    /// Implied average rating.
    pub average_rating: Option<CreditRating>,

    /// Investment grade weight (%).
    pub ig_weight: f64,

    /// High yield weight (%).
    pub hy_weight: f64,

    /// Default weight (%).
    pub default_weight: f64,

    /// Unrated weight (%).
    pub unrated_weight: f64,

    /// Crossover risk: weight in BBB bucket (%).
    pub bbb_weight: f64,

    /// Crossover risk: weight in BB bucket (%).
    pub bb_weight: f64,

    /// Quality tier distribution.
    pub quality_tiers: QualityTiers,
}

/// Distribution by quality tier.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QualityTiers {
    /// High quality: AAA, AA (%).
    pub high_quality: f64,

    /// Upper medium: A (%).
    pub upper_medium: f64,

    /// Lower medium: BBB (%).
    pub lower_medium: f64,

    /// Non-investment grade: BB and below (%).
    pub speculative: f64,

    /// Highly speculative: CCC and below (%).
    pub highly_speculative: f64,

    /// Default: D (%).
    pub default: f64,

    /// Not rated (%).
    pub not_rated: f64,
}

impl CreditQualityMetrics {
    /// Returns the crossover risk (combined BBB + BB weight).
    ///
    /// This measures exposure to bonds near the IG/HY boundary that
    /// may be subject to rating migration.
    #[must_use]
    pub fn crossover_risk(&self) -> f64 {
        self.bbb_weight + self.bb_weight
    }

    /// Returns true if the portfolio is majority investment grade.
    #[must_use]
    pub fn is_investment_grade(&self) -> bool {
        self.ig_weight > 50.0
    }

    /// Returns true if the portfolio has significant HY exposure (>10%).
    #[must_use]
    pub fn has_significant_hy(&self) -> bool {
        self.hy_weight > 10.0
    }
}

/// Calculates credit quality metrics for a portfolio.
///
/// # Arguments
///
/// * `holdings` - Slice of holdings to analyze
/// * `config` - Analytics configuration
///
/// # Returns
///
/// Comprehensive credit quality metrics.
///
/// # Example
///
/// ```rust,ignore
/// use convex_portfolio::analytics::calculate_credit_quality;
///
/// let metrics = calculate_credit_quality(&portfolio.holdings, &config);
/// println!("Average rating: {:?}", metrics.average_rating);
/// println!("IG weight: {:.1}%", metrics.ig_weight);
/// println!("Crossover risk: {:.1}%", metrics.crossover_risk());
/// ```
#[must_use]
pub fn calculate_credit_quality(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> CreditQualityMetrics {
    let rating_distribution = bucket_by_rating(holdings, config);

    let average_rating_score = rating_distribution.average_rating_score();
    let average_rating = rating_distribution.average_rating();

    let ig_weight = rating_distribution.investment_grade_weight();
    let hy_weight = rating_distribution.high_yield_weight();
    let default_weight = rating_distribution.default_weight();
    let unrated_weight = rating_distribution.unrated_weight();

    // Get BBB and BB bucket weights
    let bbb_weight = rating_distribution
        .get_bucket(RatingBucket::BBB)
        .map(|m| m.weight_pct)
        .unwrap_or(0.0);

    let bb_weight = rating_distribution
        .get_bucket(RatingBucket::BB)
        .map(|m| m.weight_pct)
        .unwrap_or(0.0);

    // Calculate quality tiers
    let quality_tiers = calculate_quality_tiers(&rating_distribution);

    CreditQualityMetrics {
        rating_distribution,
        average_rating_score,
        average_rating,
        ig_weight,
        hy_weight,
        default_weight,
        unrated_weight,
        bbb_weight,
        bb_weight,
        quality_tiers,
    }
}

/// Calculates quality tier distribution.
fn calculate_quality_tiers(dist: &RatingDistribution) -> QualityTiers {
    let get_bucket_weight =
        |bucket: RatingBucket| dist.get_bucket(bucket).map(|m| m.weight_pct).unwrap_or(0.0);

    let high_quality = get_bucket_weight(RatingBucket::AAA) + get_bucket_weight(RatingBucket::AA);

    let upper_medium = get_bucket_weight(RatingBucket::A);

    let lower_medium = get_bucket_weight(RatingBucket::BBB);

    let speculative = get_bucket_weight(RatingBucket::BB) + get_bucket_weight(RatingBucket::B);

    let highly_speculative = get_bucket_weight(RatingBucket::CCC);

    let default = get_bucket_weight(RatingBucket::Default);

    let not_rated = get_bucket_weight(RatingBucket::NotRated) + dist.unrated.weight_pct;

    QualityTiers {
        high_quality,
        upper_medium,
        lower_medium,
        speculative,
        highly_speculative,
        default,
        not_rated,
    }
}

/// Credit migration risk metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationRisk {
    /// Holdings at risk of downgrade from IG to HY.
    pub fallen_angel_risk: FallenAngelRisk,

    /// Holdings that could be upgraded from HY to IG.
    pub rising_star_risk: RisingStarRisk,
}

/// Fallen angel risk analysis.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FallenAngelRisk {
    /// Weight of BBB holdings (potential fallen angels).
    pub bbb_weight: f64,

    /// Weight of BBB- holdings (most at risk).
    pub bbb_minus_weight: f64,

    /// Total market value at risk.
    pub market_value_at_risk: Decimal,

    /// Number of holdings at risk.
    pub holdings_count: usize,
}

/// Rising star risk analysis.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RisingStarRisk {
    /// Weight of BB holdings (potential rising stars).
    pub bb_weight: f64,

    /// Weight of BB+ holdings (most likely to upgrade).
    pub bb_plus_weight: f64,

    /// Total market value that could upgrade.
    pub market_value_potential: Decimal,

    /// Number of holdings with upgrade potential.
    pub holdings_count: usize,
}

/// Calculates credit migration risk metrics.
///
/// Analyzes the portfolio for potential rating migrations across
/// the investment grade / high yield boundary.
#[must_use]
pub fn calculate_migration_risk(holdings: &[Holding], _config: &AnalyticsConfig) -> MigrationRisk {
    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();

    let mut fallen_angel = FallenAngelRisk::default();
    let mut rising_star = RisingStarRisk::default();

    for h in holdings {
        if let Some(rating) = h.classification.rating.composite {
            let mv = h.market_value();
            let weight = if total_mv.is_zero() {
                0.0
            } else {
                let w = mv / total_mv * Decimal::ONE_HUNDRED;
                w.try_into().unwrap_or(0.0)
            };

            match rating {
                CreditRating::BBBPlus | CreditRating::BBB | CreditRating::BBBMinus => {
                    fallen_angel.bbb_weight += weight;
                    fallen_angel.market_value_at_risk += mv;
                    fallen_angel.holdings_count += 1;

                    if rating == CreditRating::BBBMinus {
                        fallen_angel.bbb_minus_weight += weight;
                    }
                }
                CreditRating::BBPlus | CreditRating::BB | CreditRating::BBMinus => {
                    rising_star.bb_weight += weight;
                    rising_star.market_value_potential += mv;
                    rising_star.holdings_count += 1;

                    if rating == CreditRating::BBPlus {
                        rising_star.bb_plus_weight += weight;
                    }
                }
                _ => {}
            }
        }
    }

    MigrationRisk {
        fallen_angel_risk: fallen_angel,
        rising_star_risk: rising_star,
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
    fn test_credit_quality_empty() {
        let holdings: Vec<Holding> = vec![];
        let config = AnalyticsConfig::default();

        let metrics = calculate_credit_quality(&holdings, &config);

        assert!(metrics.average_rating.is_none());
        assert_eq!(metrics.ig_weight, 0.0);
        assert_eq!(metrics.hy_weight, 0.0);
    }

    #[test]
    fn test_credit_quality_ig_portfolio() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(CreditRating::AAA)),
            create_test_holding("H2", dec!(100), Some(CreditRating::A)),
            create_test_holding("H3", dec!(100), Some(CreditRating::BBB)),
        ];
        let config = AnalyticsConfig::default();

        let metrics = calculate_credit_quality(&holdings, &config);

        assert!((metrics.ig_weight - 100.0).abs() < 0.01);
        assert_eq!(metrics.hy_weight, 0.0);
        assert!(metrics.is_investment_grade());
    }

    #[test]
    fn test_credit_quality_mixed_portfolio() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(CreditRating::AAA)),
            create_test_holding("H2", dec!(100), Some(CreditRating::BBB)),
            create_test_holding("H3", dec!(100), Some(CreditRating::BB)),
        ];
        let config = AnalyticsConfig::default();

        let metrics = calculate_credit_quality(&holdings, &config);

        assert!((metrics.ig_weight - 66.67).abs() < 0.1);
        assert!((metrics.hy_weight - 33.33).abs() < 0.1);
    }

    #[test]
    fn test_crossover_risk() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(CreditRating::BBB)),
            create_test_holding("H2", dec!(100), Some(CreditRating::BBBMinus)),
            create_test_holding("H3", dec!(100), Some(CreditRating::BBPlus)),
            create_test_holding("H4", dec!(100), Some(CreditRating::AAA)),
        ];
        let config = AnalyticsConfig::default();

        let metrics = calculate_credit_quality(&holdings, &config);

        // BBB bucket = 50%, BB bucket = 25%
        assert!((metrics.bbb_weight - 50.0).abs() < 0.1);
        assert!((metrics.bb_weight - 25.0).abs() < 0.1);
        assert!((metrics.crossover_risk() - 75.0).abs() < 0.1);
    }

    #[test]
    fn test_quality_tiers() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(CreditRating::AAA)),
            create_test_holding("H2", dec!(100), Some(CreditRating::A)),
            create_test_holding("H3", dec!(100), Some(CreditRating::BBB)),
            create_test_holding("H4", dec!(100), Some(CreditRating::BB)),
        ];
        let config = AnalyticsConfig::default();

        let metrics = calculate_credit_quality(&holdings, &config);

        assert!((metrics.quality_tiers.high_quality - 25.0).abs() < 0.1); // AAA
        assert!((metrics.quality_tiers.upper_medium - 25.0).abs() < 0.1); // A
        assert!((metrics.quality_tiers.lower_medium - 25.0).abs() < 0.1); // BBB
        assert!((metrics.quality_tiers.speculative - 25.0).abs() < 0.1); // BB
    }

    #[test]
    fn test_migration_risk() {
        let holdings = vec![
            create_test_holding("H1", dec!(100), Some(CreditRating::BBBMinus)),
            create_test_holding("H2", dec!(100), Some(CreditRating::BBPlus)),
            create_test_holding("H3", dec!(100), Some(CreditRating::AAA)),
        ];
        let config = AnalyticsConfig::default();

        let risk = calculate_migration_risk(&holdings, &config);

        // BBB- is a fallen angel candidate
        assert_eq!(risk.fallen_angel_risk.holdings_count, 1);
        assert!((risk.fallen_angel_risk.bbb_minus_weight - 33.33).abs() < 0.1);

        // BB+ is a rising star candidate
        assert_eq!(risk.rising_star_risk.holdings_count, 1);
        assert!((risk.rising_star_risk.bb_plus_weight - 33.33).abs() < 0.1);
    }
}
