//! Portfolio liquidity analytics.
//!
//! Provides liquidity metrics and analysis:
//! - Bid-ask spread aggregation
//! - Liquidity score distribution
//! - Days to liquidate estimation
//! - Liquidity stress testing

use crate::types::{AnalyticsConfig, Holding};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Aggregated liquidity metrics for a portfolio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityMetrics {
    /// Weighted average bid-ask spread (basis points).
    pub avg_bid_ask_spread: Option<f64>,

    /// Weighted average liquidity score (0-100).
    pub avg_liquidity_score: Option<f64>,

    /// Percentage of holdings classified as highly liquid (score >= 70).
    pub highly_liquid_pct: f64,

    /// Percentage of holdings classified as moderately liquid (30 <= score < 70).
    pub moderately_liquid_pct: f64,

    /// Percentage of holdings classified as illiquid (score < 30).
    pub illiquid_pct: f64,

    /// Holdings with bid-ask spread data.
    pub bid_ask_coverage: usize,

    /// Holdings with liquidity score data.
    pub score_coverage: usize,

    /// Total holdings count.
    pub total_holdings: usize,
}

impl LiquidityMetrics {
    /// Returns bid-ask coverage as a percentage.
    pub fn bid_ask_coverage_pct(&self) -> f64 {
        if self.total_holdings > 0 {
            self.bid_ask_coverage as f64 / self.total_holdings as f64 * 100.0
        } else {
            0.0
        }
    }

    /// Returns liquidity score coverage as a percentage.
    pub fn score_coverage_pct(&self) -> f64 {
        if self.total_holdings > 0 {
            self.score_coverage as f64 / self.total_holdings as f64 * 100.0
        } else {
            0.0
        }
    }

    /// Returns true if portfolio has liquidity concerns.
    pub fn has_liquidity_concerns(&self) -> bool {
        self.illiquid_pct > 15.0 || self.avg_liquidity_score.map(|s| s < 50.0).unwrap_or(false)
    }
}

/// Calculates aggregated liquidity metrics for a portfolio.
///
/// # Arguments
///
/// * `holdings` - Portfolio holdings with optional liquidity data
/// * `config` - Analytics configuration
///
/// # Returns
///
/// Aggregated liquidity metrics.
///
/// # Example
///
/// ```rust,ignore
/// use convex_portfolio::analytics::calculate_liquidity_metrics;
///
/// let metrics = calculate_liquidity_metrics(&portfolio.holdings, &config);
/// println!("Avg bid-ask: {:.1}bp", metrics.avg_bid_ask_spread.unwrap_or(0.0));
/// println!("Illiquid: {:.1}%", metrics.illiquid_pct);
/// ```
pub fn calculate_liquidity_metrics(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> crate::PortfolioResult<LiquidityMetrics> {
    Ok({
        if holdings.is_empty() {
            return Ok(LiquidityMetrics {
                avg_bid_ask_spread: None,
                avg_liquidity_score: None,
                highly_liquid_pct: 0.0,
                moderately_liquid_pct: 0.0,
                illiquid_pct: 0.0,
                bid_ask_coverage: 0,
                score_coverage: 0,
                total_holdings: 0,
            });
        }

        let total_holdings = holdings.len();

        // Calculate weighted average bid-ask spread
        let avg_bid_ask_spread = weighted_bid_ask_spread(holdings, config)?;

        // Calculate weighted average liquidity score
        let avg_liquidity_score = weighted_liquidity_score(holdings, config)?;

        // Count coverage
        let bid_ask_coverage = holdings
            .iter()
            .filter(|h| h.analytics.bid_ask_spread.is_some())
            .count();
        let score_coverage = holdings
            .iter()
            .filter(|h| h.analytics.liquidity_score.is_some())
            .count();

        let total = crate::analytics::metrics::clean_exact(holdings.iter())?;
        let bucket = |kind: u8| -> crate::PortfolioResult<f64> {
            let selected = holdings.iter().filter(|h| {
                h.analytics.liquidity_score.is_some_and(|score| match kind {
                    0 => score >= 70.0,
                    1 => (30.0..70.0).contains(&score),
                    _ => score < 30.0,
                })
            });
            let sum =
                crate::analytics::metrics::clean_exact(selected)?.multiply(Decimal::ONE_HUNDRED)?;
            let value =
                crate::analytics::metrics::signed_ratio(&sum, &total)?.ok_or_else(|| {
                    crate::PortfolioError::invalid_portfolio(
                        "liquidity bucket ratio is undefined at zero total value",
                    )
                })?;
            crate::analytics::metrics::presentation(value)
        };
        let (highly_liquid, moderately_liquid, illiquid) = (bucket(0)?, bucket(1)?, bucket(2)?);

        LiquidityMetrics {
            avg_bid_ask_spread,
            avg_liquidity_score,
            highly_liquid_pct: highly_liquid,
            moderately_liquid_pct: moderately_liquid,
            illiquid_pct: illiquid,
            bid_ask_coverage,
            score_coverage,
            total_holdings,
        }
    })
}

/// Calculates weighted average bid-ask spread.
///
/// # Returns
///
/// Weighted average bid-ask spread in basis points, or None if no data.
pub fn weighted_bid_ask_spread(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> crate::PortfolioResult<Option<f64>> {
    crate::analytics::metrics::MetricBook::new(holdings, config)?
        .weighted("bid_ask_spread", |h| h.analytics.bid_ask_spread)
}

/// Calculates weighted average liquidity score.
///
/// # Returns
///
/// Weighted average liquidity score (0-100), or None if no data.
pub fn weighted_liquidity_score(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> crate::PortfolioResult<Option<f64>> {
    crate::analytics::metrics::MetricBook::new(holdings, config)?
        .weighted("liquidity_score", |h| h.analytics.liquidity_score)
}

/// Liquidity bucket classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LiquidityBucket {
    /// Highly liquid (score >= 70 or bid-ask < 10bp).
    HighlyLiquid,
    /// Moderately liquid (30 <= score < 70 or 10 <= bid-ask < 50bp).
    ModeratelyLiquid,
    /// Less liquid (score < 30 or bid-ask >= 50bp).
    LessLiquid,
    /// Unknown liquidity.
    Unknown,
}

impl LiquidityBucket {
    /// Classify a holding based on liquidity score and bid-ask spread.
    pub fn classify(liquidity_score: Option<f64>, bid_ask_spread: Option<f64>) -> Self {
        // Prefer liquidity score if available
        if let Some(score) = liquidity_score {
            if score >= 70.0 {
                return Self::HighlyLiquid;
            } else if score >= 30.0 {
                return Self::ModeratelyLiquid;
            } else {
                return Self::LessLiquid;
            }
        }

        // Fall back to bid-ask spread
        if let Some(spread) = bid_ask_spread {
            if spread < 10.0 {
                return Self::HighlyLiquid;
            } else if spread < 50.0 {
                return Self::ModeratelyLiquid;
            } else {
                return Self::LessLiquid;
            }
        }

        Self::Unknown
    }
}

/// Liquidity distribution by bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityDistribution {
    /// Distribution by liquidity bucket.
    pub by_bucket: HashMap<LiquidityBucket, BucketInfo>,

    /// Total market value.
    pub total_market_value: Decimal,

    /// Holdings without liquidity data.
    pub unknown_count: usize,
}

/// Information about a liquidity bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketInfo {
    /// Market value in this bucket.
    pub market_value: Decimal,

    /// Weight as percentage.
    pub weight_pct: f64,

    /// Number of holdings.
    pub count: usize,

    /// Average liquidity score in this bucket.
    pub avg_score: Option<f64>,

    /// Average bid-ask spread in this bucket.
    pub avg_spread: Option<f64>,
}

/// Calculates liquidity distribution by bucket.
///
/// # Arguments
///
/// * `holdings` - Portfolio holdings
/// * `config` - Analytics configuration
///
/// # Returns
///
/// Distribution of holdings across liquidity buckets.
pub fn liquidity_distribution(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> crate::PortfolioResult<LiquidityDistribution> {
    use crate::analytics::metrics::{self, MetricBook};
    let total = metrics::clean_exact(holdings.iter())?;
    let mut groups = HashMap::<LiquidityBucket, Vec<&Holding>>::new();
    for holding in holdings {
        let bucket = LiquidityBucket::classify(
            holding.analytics.liquidity_score,
            holding.analytics.bid_ask_spread,
        );
        groups.entry(bucket).or_default().push(holding);
    }
    let unknown_count = groups.get(&LiquidityBucket::Unknown).map_or(0, Vec::len);
    let mut by_bucket = HashMap::new();
    for (bucket, group) in groups {
        let value = metrics::clean_exact(group.iter().copied())?;
        let fraction = metrics::signed_ratio(&value.multiply(Decimal::ONE_HUNDRED)?, &total)?
            .ok_or_else(|| {
                crate::PortfolioError::invalid_portfolio(
                    "liquidity distribution ratio undefined at zero value",
                )
            })?;
        let book = MetricBook::from_rows(
            group.iter().copied(),
            &config
                .clone()
                .with_weighting(crate::WeightingMethod::EqualWeight),
        )?;
        by_bucket.insert(
            bucket,
            BucketInfo {
                count: group.len(),
                market_value: value.finish()?,
                weight_pct: metrics::presentation(fraction)?,
                avg_score: book.weighted("liquidity_score", |h| h.analytics.liquidity_score)?,
                avg_spread: book.weighted("bid_ask_spread", |h| h.analytics.bid_ask_spread)?,
            },
        );
    }
    // No empty normalization is reported; an empty distribution contains no buckets.
    Ok(LiquidityDistribution {
        by_bucket,
        total_market_value: total.finish()?,
        unknown_count,
    })
}

/// Estimates days to liquidate portfolio.
///
/// Uses a simple model based on average daily volume (ADV) assumptions.
///
/// # Arguments
///
/// * `holdings` - Portfolio holdings with optional liquidity data
/// * `max_participation_rate` - Maximum percentage of ADV to participate (typically 10-25%)
/// * `config` - Analytics configuration
///
/// # Returns
///
/// Estimated number of days to fully liquidate the portfolio.
pub fn estimate_days_to_liquidate(
    holdings: &[Holding],
    max_participation_rate: f64,
    _config: &AnalyticsConfig,
) -> crate::PortfolioResult<DaysToLiquidate> {
    if !max_participation_rate.is_finite()
        || max_participation_rate <= 0.0
        || max_participation_rate > 100.0
    {
        return Err(crate::PortfolioError::invalid_portfolio(
            "finite participation percentage in (0,100] required",
        ));
    }
    Ok({
        if holdings.is_empty() || max_participation_rate <= 0.0 {
            return Ok(DaysToLiquidate {
                total_days: 0.0,
                highly_liquid_days: 0.0,
                illiquid_days: 0.0,
                holdings_without_adv: holdings.len(),
            });
        }

        // Simple model: estimate ADV from liquidity score
        // Higher score = higher ADV = faster to liquidate
        let mut total_days = 0.0;
        let mut highly_liquid_days = 0.0;
        let mut illiquid_days = 0.0;
        let mut missing_adv = 0;

        for h in holdings {
            let mv = crate::analytics::metrics::presentation(h.market_value()?)?;

            if let Some(score) = h.analytics.liquidity_score {
                // Estimate ADV based on liquidity score
                // Assume liquidity score 100 = 10M ADV, score 0 = 100K ADV (logarithmic)
                let estimated_adv = 100_000.0 * (10.0_f64.powf(score / 50.0));
                let daily_capacity = estimated_adv * max_participation_rate / 100.0;

                if daily_capacity > 0.0 {
                    let days = mv / daily_capacity;
                    total_days += days;

                    if score >= 70.0 {
                        highly_liquid_days += days;
                    } else if score < 30.0 {
                        illiquid_days += days;
                    }
                }
            } else {
                missing_adv += 1;
                // Assume illiquid for unknown
                let conservative_adv = 100_000.0;
                let daily_capacity = conservative_adv * max_participation_rate / 100.0;
                if daily_capacity > 0.0 {
                    let days = mv / daily_capacity;
                    total_days += days;
                    illiquid_days += days;
                }
            }
        }

        DaysToLiquidate {
            total_days,
            highly_liquid_days,
            illiquid_days,
            holdings_without_adv: missing_adv,
        }
    })
}

/// Days to liquidate breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaysToLiquidate {
    /// Total estimated days to liquidate entire portfolio.
    pub total_days: f64,

    /// Days attributed to highly liquid holdings.
    pub highly_liquid_days: f64,

    /// Days attributed to illiquid holdings.
    pub illiquid_days: f64,

    /// Number of holdings without ADV data.
    pub holdings_without_adv: usize,
}

impl DaysToLiquidate {
    /// Percentage of time spent on illiquid holdings.
    pub fn illiquid_pct_of_time(&self) -> f64 {
        if self.total_days > 0.0 {
            self.illiquid_days / self.total_days * 100.0
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{HoldingAnalytics, HoldingBuilder};
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal_macros::dec;

    fn create_holding_with_liquidity(
        id: &str,
        mv: Decimal,
        liquidity_score: Option<f64>,
        bid_ask: Option<f64>,
    ) -> Holding {
        let mut analytics = HoldingAnalytics::new();
        analytics.liquidity_score = liquidity_score;
        analytics.bid_ask_spread = bid_ask;

        HoldingBuilder::new()
            .id(id)
            .identifiers(BondIdentifiers::from_isin_str("US912828Z229").unwrap())
            .par_amount(mv)
            .market_price(dec!(100))
            .analytics(analytics)
            .build()
            .unwrap()
    }

    #[test]
    fn test_calculate_liquidity_metrics_empty() {
        let config = AnalyticsConfig::default();
        let metrics = calculate_liquidity_metrics(&[], &config).unwrap();

        assert!(metrics.avg_bid_ask_spread.is_none());
        assert!(metrics.avg_liquidity_score.is_none());
        assert_eq!(metrics.total_holdings, 0);
    }

    #[test]
    fn test_calculate_liquidity_metrics_with_data() {
        let holdings = vec![
            create_holding_with_liquidity("H1", dec!(500_000), Some(80.0), Some(5.0)),
            create_holding_with_liquidity("H2", dec!(500_000), Some(40.0), Some(25.0)),
        ];

        let config = AnalyticsConfig::default();
        let metrics = calculate_liquidity_metrics(&holdings, &config).unwrap();

        // Equal weights: avg score = (80 + 40) / 2 = 60
        assert!(metrics.avg_liquidity_score.is_some());
        assert!((metrics.avg_liquidity_score.unwrap() - 60.0).abs() < 0.1);

        // Equal weights: avg spread = (5 + 25) / 2 = 15
        assert!(metrics.avg_bid_ask_spread.is_some());
        assert!((metrics.avg_bid_ask_spread.unwrap() - 15.0).abs() < 0.1);

        // H1 (80) = highly liquid, H2 (40) = moderately liquid
        assert!((metrics.highly_liquid_pct - 50.0).abs() < 0.1);
        assert!((metrics.moderately_liquid_pct - 50.0).abs() < 0.1);
        assert!((metrics.illiquid_pct - 0.0).abs() < 0.1);
    }

    #[test]
    fn test_calculate_liquidity_metrics_illiquid() {
        let holdings = vec![
            create_holding_with_liquidity("H1", dec!(300_000), Some(20.0), Some(100.0)),
            create_holding_with_liquidity("H2", dec!(700_000), Some(25.0), Some(80.0)),
        ];

        let config = AnalyticsConfig::default();
        let metrics = calculate_liquidity_metrics(&holdings, &config).unwrap();

        // Both are illiquid (score < 30)
        assert!((metrics.illiquid_pct - 100.0).abs() < 0.1);
        assert!(metrics.has_liquidity_concerns());
    }

    #[test]
    fn test_liquidity_bucket_classify() {
        assert_eq!(
            LiquidityBucket::classify(Some(80.0), None),
            LiquidityBucket::HighlyLiquid
        );
        assert_eq!(
            LiquidityBucket::classify(Some(50.0), None),
            LiquidityBucket::ModeratelyLiquid
        );
        assert_eq!(
            LiquidityBucket::classify(Some(20.0), None),
            LiquidityBucket::LessLiquid
        );

        // Fall back to bid-ask
        assert_eq!(
            LiquidityBucket::classify(None, Some(5.0)),
            LiquidityBucket::HighlyLiquid
        );
        assert_eq!(
            LiquidityBucket::classify(None, Some(30.0)),
            LiquidityBucket::ModeratelyLiquid
        );
        assert_eq!(
            LiquidityBucket::classify(None, Some(100.0)),
            LiquidityBucket::LessLiquid
        );

        // No data
        assert_eq!(
            LiquidityBucket::classify(None, None),
            LiquidityBucket::Unknown
        );
    }

    #[test]
    fn test_liquidity_distribution() {
        let holdings = vec![
            create_holding_with_liquidity("H1", dec!(400_000), Some(80.0), None),
            create_holding_with_liquidity("H2", dec!(300_000), Some(50.0), None),
            create_holding_with_liquidity("H3", dec!(200_000), Some(20.0), None),
            create_holding_with_liquidity("H4", dec!(100_000), None, None),
        ];

        let config = AnalyticsConfig::default();
        let dist = liquidity_distribution(&holdings, &config).unwrap();

        assert_eq!(dist.unknown_count, 1);

        let highly_liquid = dist.by_bucket.get(&LiquidityBucket::HighlyLiquid);
        assert!(highly_liquid.is_some());
        assert!((highly_liquid.unwrap().weight_pct - 40.0).abs() < 0.1);
    }

    #[test]
    fn test_estimate_days_to_liquidate() {
        let holdings = vec![
            create_holding_with_liquidity("H1", dec!(1_000_000), Some(80.0), None),
            create_holding_with_liquidity("H2", dec!(1_000_000), Some(20.0), None),
        ];

        let config = AnalyticsConfig::default();
        let dtl = estimate_days_to_liquidate(&holdings, 20.0, &config).unwrap();

        // High liquidity holding should be faster
        assert!(dtl.total_days > 0.0);
        assert!(dtl.illiquid_days > dtl.highly_liquid_days);
    }

    #[test]
    fn test_days_to_liquidate_empty() {
        let config = AnalyticsConfig::default();
        let dtl = estimate_days_to_liquidate(&[], 20.0, &config).unwrap();

        assert_eq!(dtl.total_days, 0.0);
        assert_eq!(dtl.holdings_without_adv, 0);
    }

    #[test]
    fn test_weighted_bid_ask_spread() {
        let holdings = vec![
            create_holding_with_liquidity("H1", dec!(600_000), None, Some(10.0)),
            create_holding_with_liquidity("H2", dec!(400_000), None, Some(20.0)),
        ];

        let config = AnalyticsConfig::default();
        let spread = weighted_bid_ask_spread(&holdings, &config).unwrap();

        // Weighted: (10 × 600 + 20 × 400) / (600 + 400) = 14
        assert!(spread.is_some());
        assert!((spread.unwrap() - 14.0).abs() < 0.1);
    }

    #[test]
    fn test_has_liquidity_concerns() {
        let metrics = LiquidityMetrics {
            avg_bid_ask_spread: Some(10.0),
            avg_liquidity_score: Some(45.0),
            highly_liquid_pct: 30.0,
            moderately_liquid_pct: 50.0,
            illiquid_pct: 20.0,
            bid_ask_coverage: 10,
            score_coverage: 10,
            total_holdings: 10,
        };

        // Illiquid > 15% → has concerns
        assert!(metrics.has_liquidity_concerns());
    }
}
