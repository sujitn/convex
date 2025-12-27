//! Return attribution helpers.
//!
//! Provides return attribution analysis following CFA Fixed Income Attribution:
//! - Income return = coupon / price
//! - Treasury return = -(duration × Δyield) + convexity adjustment
//! - Spread return = -(spread duration × Δspread)
//! - Residual = total - income - treasury - spread

use crate::types::{AnalyticsConfig, Holding, Sector};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Return attribution for a single holding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldingAttribution {
    /// Holding identifier.
    pub id: String,

    /// Total return (%).
    pub total_return: f64,

    /// Income return = (coupon / price) × holding period (%).
    pub income_return: f64,

    /// Treasury/rate return = -(duration × Δyield) (%).
    pub treasury_return: f64,

    /// Spread return = -(spread duration × Δspread) (%).
    pub spread_return: f64,

    /// Residual = total - income - treasury - spread (%).
    pub residual: f64,

    /// Convexity adjustment (included in treasury_return) (%).
    pub convexity_adjustment: f64,

    /// Market value weight (0-1).
    pub weight: f64,
}

/// Aggregated return attribution for a portfolio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioAttribution {
    /// Attribution by holding.
    pub by_holding: Vec<HoldingAttribution>,

    /// Attribution by sector.
    pub by_sector: HashMap<Sector, SectorAttribution>,

    /// Portfolio-level aggregated attribution.
    pub portfolio: AggregatedAttribution,
}

/// Aggregated attribution for a sector or other grouping.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SectorAttribution {
    /// Number of holdings.
    pub count: usize,

    /// Total weight (0-1).
    pub weight: f64,

    /// Weighted total return (%).
    pub total_return: f64,

    /// Weighted income return (%).
    pub income_return: f64,

    /// Weighted treasury return (%).
    pub treasury_return: f64,

    /// Weighted spread return (%).
    pub spread_return: f64,

    /// Weighted residual (%).
    pub residual: f64,
}

/// Portfolio-level aggregated attribution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AggregatedAttribution {
    /// Weighted total return (%).
    pub total_return: f64,

    /// Weighted income return (%).
    pub income_return: f64,

    /// Weighted treasury return (%).
    pub treasury_return: f64,

    /// Weighted spread return (%).
    pub spread_return: f64,

    /// Weighted residual (%).
    pub residual: f64,

    /// Weighted convexity adjustment (%).
    pub convexity_adjustment: f64,
}

/// Input for attribution calculation (period returns and changes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionInput {
    /// Holding ID.
    pub id: String,

    /// Total return for the period (as decimal, e.g., 0.02 for 2%).
    pub total_return: f64,

    /// Change in yield (as decimal, e.g., 0.0025 for 25bp).
    pub yield_change: f64,

    /// Change in spread (in bps, e.g., 10.0 for 10bp widening).
    pub spread_change: f64,

    /// Holding period in years (e.g., 1.0/12.0 for 1 month).
    pub holding_period: f64,
}

/// Calculates return attribution for a set of holdings.
///
/// Uses the CFA Fixed Income Attribution methodology:
/// - Income return = (coupon / price) × holding period
/// - Treasury return = -(duration × Δyield) + 0.5 × convexity × Δyield²
/// - Spread return = -(spread duration × Δspread)
/// - Residual = total - income - treasury - spread
///
/// # Arguments
///
/// * `holdings` - Holdings with analytics
/// * `inputs` - Period returns and changes for each holding
/// * `config` - Analytics configuration
///
/// # Returns
///
/// Portfolio attribution breakdown.
///
/// # Example
///
/// ```rust,ignore
/// use convex_portfolio::contribution::{calculate_attribution, AttributionInput};
///
/// let inputs = vec![
///     AttributionInput {
///         id: "H1".to_string(),
///         total_return: 0.015,        // 1.5% total return
///         yield_change: -0.0025,      // 25bp rally
///         spread_change: 5.0,         // 5bp widening
///         holding_period: 1.0 / 12.0, // 1 month
///     },
/// ];
///
/// let attribution = calculate_attribution(&portfolio.holdings, &inputs, &config);
/// println!("Income: {:.2}%", attribution.portfolio.income_return * 100.0);
/// println!("Treasury: {:.2}%", attribution.portfolio.treasury_return * 100.0);
/// println!("Spread: {:.2}%", attribution.portfolio.spread_return * 100.0);
/// ```
#[must_use]
pub fn calculate_attribution(
    holdings: &[Holding],
    inputs: &[AttributionInput],
    _config: &AnalyticsConfig,
) -> PortfolioAttribution {
    if holdings.is_empty() || inputs.is_empty() {
        return PortfolioAttribution {
            by_holding: vec![],
            by_sector: HashMap::new(),
            portfolio: AggregatedAttribution::default(),
        };
    }

    // Create lookup for inputs
    let input_map: HashMap<&str, &AttributionInput> =
        inputs.iter().map(|i| (i.id.as_str(), i)).collect();

    // Calculate total market value for weighting
    let total_mv: Decimal = holdings.iter().map(|h| h.market_value()).sum();
    let total_mv_f: f64 = total_mv.try_into().unwrap_or(1.0);

    // Calculate individual attributions
    let mut by_holding: Vec<HoldingAttribution> = Vec::with_capacity(holdings.len());

    for h in holdings {
        if let Some(input) = input_map.get(h.id.as_str()) {
            let mv: f64 = h.market_value().try_into().unwrap_or(0.0);
            let weight = mv / total_mv_f;

            // Income return = (current_yield × holding_period)
            // Or approximate from coupon rate if current_yield not available
            let income_return = h.analytics.current_yield.unwrap_or(0.0) * input.holding_period;

            // Treasury return = -(duration × Δyield) + 0.5 × convexity × Δyield²
            let duration = h.analytics.best_duration().unwrap_or(0.0);
            let convexity = h.analytics.convexity.unwrap_or(0.0);
            let yield_change = input.yield_change;

            let treasury_base = -duration * yield_change;
            let convexity_adjustment = 0.5 * convexity * yield_change.powi(2);
            let treasury_return = treasury_base + convexity_adjustment;

            // Spread return = -(spread duration × Δspread in decimal)
            let spread_duration = h.analytics.spread_duration.unwrap_or(duration);
            let spread_change_decimal = input.spread_change / 10000.0; // Convert bps to decimal
            let spread_return = -spread_duration * spread_change_decimal;

            // Residual
            let residual = input.total_return - income_return - treasury_return - spread_return;

            by_holding.push(HoldingAttribution {
                id: h.id.clone(),
                total_return: input.total_return * 100.0, // Convert to percentage
                income_return: income_return * 100.0,
                treasury_return: treasury_return * 100.0,
                spread_return: spread_return * 100.0,
                residual: residual * 100.0,
                convexity_adjustment: convexity_adjustment * 100.0,
                weight,
            });
        }
    }

    // Aggregate by sector
    let by_sector = aggregate_attribution_by_sector(holdings, &by_holding);

    // Aggregate portfolio level
    let portfolio = aggregate_portfolio_attribution(&by_holding);

    PortfolioAttribution {
        by_holding,
        by_sector,
        portfolio,
    }
}

/// Estimates income return based on coupon and price.
///
/// Income return = (annual coupon rate / clean price) × holding period
///
/// # Arguments
///
/// * `holdings` - Holdings to analyze
/// * `holding_period` - Period in years (e.g., 1/12 for 1 month)
/// * `config` - Analytics configuration
///
/// # Returns
///
/// Estimated income returns by holding.
#[must_use]
pub fn estimate_income_returns(
    holdings: &[Holding],
    holding_period: f64,
    _config: &AnalyticsConfig,
) -> Vec<(String, f64)> {
    holdings
        .iter()
        .filter_map(|h| {
            h.analytics.current_yield.map(|cy| {
                let income = cy * holding_period * 100.0; // Convert to percentage
                (h.id.clone(), income)
            })
        })
        .collect()
}

/// Estimates rate return (treasury return) based on duration and yield change.
///
/// Rate return = -(duration × Δyield) + 0.5 × convexity × Δyield²
///
/// # Arguments
///
/// * `holdings` - Holdings to analyze
/// * `yield_change` - Change in yield (as decimal, e.g., 0.0025 for 25bp)
/// * `config` - Analytics configuration
///
/// # Returns
///
/// Estimated rate returns by holding.
#[must_use]
pub fn estimate_rate_returns(
    holdings: &[Holding],
    yield_change: f64,
    _config: &AnalyticsConfig,
) -> Vec<(String, f64)> {
    holdings
        .iter()
        .filter_map(|h| {
            h.analytics.best_duration().map(|dur| {
                let convexity = h.analytics.convexity.unwrap_or(0.0);
                let rate_return =
                    (-dur * yield_change + 0.5 * convexity * yield_change.powi(2)) * 100.0;
                (h.id.clone(), rate_return)
            })
        })
        .collect()
}

/// Estimates spread return based on spread duration and spread change.
///
/// Spread return = -(spread duration × Δspread)
///
/// # Arguments
///
/// * `holdings` - Holdings to analyze
/// * `spread_change` - Change in spread (in bps, e.g., 10.0 for 10bp)
/// * `config` - Analytics configuration
///
/// # Returns
///
/// Estimated spread returns by holding.
#[must_use]
pub fn estimate_spread_returns(
    holdings: &[Holding],
    spread_change: f64,
    _config: &AnalyticsConfig,
) -> Vec<(String, f64)> {
    let spread_change_decimal = spread_change / 10000.0;

    holdings
        .iter()
        .filter_map(|h| {
            let spread_dur = h
                .analytics
                .spread_duration
                .or(h.analytics.best_duration())?;
            let spread_return = -spread_dur * spread_change_decimal * 100.0;
            Some((h.id.clone(), spread_return))
        })
        .collect()
}

/// Helper to aggregate attribution by sector.
fn aggregate_attribution_by_sector(
    holdings: &[Holding],
    attributions: &[HoldingAttribution],
) -> HashMap<Sector, SectorAttribution> {
    let mut by_sector: HashMap<Sector, SectorAttribution> = HashMap::new();

    // Create lookup for holdings
    let holding_map: HashMap<&str, &Holding> =
        holdings.iter().map(|h| (h.id.as_str(), h)).collect();

    for attr in attributions {
        if let Some(holding) = holding_map.get(attr.id.as_str()) {
            if let Some(sector) = holding.classification.sector.composite {
                let entry = by_sector.entry(sector).or_default();
                entry.count += 1;
                entry.weight += attr.weight;
                entry.total_return += attr.weight * attr.total_return;
                entry.income_return += attr.weight * attr.income_return;
                entry.treasury_return += attr.weight * attr.treasury_return;
                entry.spread_return += attr.weight * attr.spread_return;
                entry.residual += attr.weight * attr.residual;
            }
        }
    }

    by_sector
}

/// Helper to aggregate portfolio-level attribution.
fn aggregate_portfolio_attribution(attributions: &[HoldingAttribution]) -> AggregatedAttribution {
    let mut result = AggregatedAttribution::default();

    for attr in attributions {
        result.total_return += attr.weight * attr.total_return;
        result.income_return += attr.weight * attr.income_return;
        result.treasury_return += attr.weight * attr.treasury_return;
        result.spread_return += attr.weight * attr.spread_return;
        result.residual += attr.weight * attr.residual;
        result.convexity_adjustment += attr.weight * attr.convexity_adjustment;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Classification, HoldingAnalytics, HoldingBuilder, SectorInfo};
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal_macros::dec;

    fn create_test_holding(
        id: &str,
        mv: Decimal,
        duration: f64,
        convexity: f64,
        _current_yield: f64,
        sector: Option<Sector>,
    ) -> Holding {
        let mut classification = Classification::new();
        if let Some(s) = sector {
            classification = classification.with_sector(SectorInfo::from_composite(s));
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
                    .with_convexity(convexity)
                    .with_z_spread(100.0),
            )
            .build()
            .unwrap()
    }

    fn add_current_yield(mut holding: Holding, current_yield: f64) -> Holding {
        holding.analytics.current_yield = Some(current_yield);
        holding
    }

    #[test]
    fn test_calculate_attribution_empty() {
        let holdings: Vec<Holding> = vec![];
        let inputs: Vec<AttributionInput> = vec![];
        let config = AnalyticsConfig::default();

        let attr = calculate_attribution(&holdings, &inputs, &config);

        assert!(attr.by_holding.is_empty());
        assert_eq!(attr.portfolio.total_return, 0.0);
    }

    #[test]
    fn test_calculate_attribution_single() {
        let holdings = vec![add_current_yield(
            create_test_holding("H1", dec!(100), 5.0, 50.0, 0.05, None),
            0.05,
        )];

        let inputs = vec![AttributionInput {
            id: "H1".to_string(),
            total_return: 0.02,         // 2% total
            yield_change: -0.005,       // 50bp rally
            spread_change: 10.0,        // 10bp widening
            holding_period: 1.0 / 12.0, // 1 month
        }];

        let config = AnalyticsConfig::default();
        let attr = calculate_attribution(&holdings, &inputs, &config);

        assert_eq!(attr.by_holding.len(), 1);
        let h1 = &attr.by_holding[0];

        // Income = 5% × (1/12) = 0.417%
        assert!((h1.income_return - 0.417).abs() < 0.01);

        // Treasury = -(-5 × 0.005) + 0.5 × 50 × 0.005² = 2.5% + 0.0625% ≈ 2.56%
        assert!((h1.treasury_return - 2.56).abs() < 0.1);

        // Spread = -(5 × 0.001) = -0.5%
        assert!((h1.spread_return - (-0.5)).abs() < 0.01);
    }

    #[test]
    fn test_calculate_attribution_by_sector() {
        let holdings = vec![
            add_current_yield(
                create_test_holding("H1", dec!(100), 4.0, 40.0, 0.04, Some(Sector::Government)),
                0.04,
            ),
            add_current_yield(
                create_test_holding("H2", dec!(100), 6.0, 60.0, 0.06, Some(Sector::Corporate)),
                0.06,
            ),
        ];

        let inputs = vec![
            AttributionInput {
                id: "H1".to_string(),
                total_return: 0.015,
                yield_change: -0.003,
                spread_change: 5.0,
                holding_period: 1.0 / 12.0,
            },
            AttributionInput {
                id: "H2".to_string(),
                total_return: 0.025,
                yield_change: -0.003,
                spread_change: 15.0,
                holding_period: 1.0 / 12.0,
            },
        ];

        let config = AnalyticsConfig::default();
        let attr = calculate_attribution(&holdings, &inputs, &config);

        assert_eq!(attr.by_sector.len(), 2);
        assert!(attr.by_sector.contains_key(&Sector::Government));
        assert!(attr.by_sector.contains_key(&Sector::Corporate));
    }

    #[test]
    fn test_estimate_income_returns() {
        let holdings = vec![
            add_current_yield(
                create_test_holding("H1", dec!(100), 5.0, 50.0, 0.05, None),
                0.05,
            ),
            add_current_yield(
                create_test_holding("H2", dec!(100), 5.0, 50.0, 0.04, None),
                0.04,
            ),
        ];

        let config = AnalyticsConfig::default();
        let income = estimate_income_returns(&holdings, 1.0 / 12.0, &config);

        assert_eq!(income.len(), 2);
        // H1: 5% × (1/12) = 0.417%
        let h1 = income.iter().find(|(id, _)| id == "H1").unwrap();
        assert!((h1.1 - 0.417).abs() < 0.01);
    }

    #[test]
    fn test_estimate_rate_returns() {
        let holdings = vec![create_test_holding("H1", dec!(100), 5.0, 50.0, 0.05, None)];

        let config = AnalyticsConfig::default();
        // 25bp rally
        let rate = estimate_rate_returns(&holdings, -0.0025, &config);

        assert_eq!(rate.len(), 1);
        // Rate return = -(-5 × 0.0025) + 0.5 × 50 × 0.0025² = 1.25% + 0.016% ≈ 1.27%
        assert!((rate[0].1 - 1.27).abs() < 0.1);
    }

    #[test]
    fn test_estimate_spread_returns() {
        let holdings = vec![create_test_holding("H1", dec!(100), 5.0, 50.0, 0.05, None)];

        let config = AnalyticsConfig::default();
        // 10bp widening
        let spread = estimate_spread_returns(&holdings, 10.0, &config);

        assert_eq!(spread.len(), 1);
        // Spread return = -(5 × 0.001) × 100 = -0.5%
        assert!((spread[0].1 - (-0.5)).abs() < 0.01);
    }

    #[test]
    fn test_portfolio_attribution_aggregation() {
        let holdings = vec![
            add_current_yield(
                create_test_holding("H1", dec!(100), 4.0, 40.0, 0.04, None),
                0.04,
            ),
            add_current_yield(
                create_test_holding("H2", dec!(200), 6.0, 60.0, 0.06, None),
                0.06,
            ),
        ];

        let inputs = vec![
            AttributionInput {
                id: "H1".to_string(),
                total_return: 0.015,
                yield_change: -0.003,
                spread_change: 5.0,
                holding_period: 1.0,
            },
            AttributionInput {
                id: "H2".to_string(),
                total_return: 0.025,
                yield_change: -0.003,
                spread_change: 10.0,
                holding_period: 1.0,
            },
        ];

        let config = AnalyticsConfig::default();
        let attr = calculate_attribution(&holdings, &inputs, &config);

        // Portfolio total should be weighted average
        // H1 weight = 1/3, H2 weight = 2/3
        // Expected total = (1/3 × 1.5%) + (2/3 × 2.5%) ≈ 2.17%
        assert!((attr.portfolio.total_return - 2.17).abs() < 0.1);
    }
}
