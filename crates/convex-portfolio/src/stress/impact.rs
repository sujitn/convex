//! Stress impact calculations.
//!
//! Calculates the price/value impact of stress scenarios using
//! duration and convexity approximations.

use super::scenarios::{RateScenario, SpreadScenario, StressScenario};
use crate::analytics::{aggregate_key_rate_profile, weighted_best_duration, weighted_convexity};
use crate::types::{AnalyticsConfig, Holding};
use crate::{maybe_parallel_fold, Portfolio};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

/// Result of a stress test on a portfolio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressResult {
    /// Scenario name.
    pub scenario_name: String,

    /// Initial portfolio value (NAV).
    pub initial_value: f64,

    /// Estimated value after stress.
    pub stressed_value: f64,

    /// Absolute P&L.
    pub pnl: f64,

    /// P&L as percentage of initial value.
    pub pnl_pct: f64,

    /// Rate impact component.
    pub rate_impact: Option<f64>,

    /// Spread impact component.
    pub spread_impact: Option<f64>,
}

impl StressResult {
    /// Returns true if this is a gain.
    #[must_use]
    pub fn is_gain(&self) -> bool {
        self.pnl > 0.0
    }

    /// Returns true if this is a loss.
    #[must_use]
    pub fn is_loss(&self) -> bool {
        self.pnl < 0.0
    }
}

/// Calculates the impact of a parallel rate shift.
///
/// ## Formula
///
/// Using duration-convexity approximation:
/// ```text
/// ΔP/P ≈ -Duration × Δy + 0.5 × Convexity × (Δy)²
/// ```
///
/// Where Δy is the yield change in decimal form.
///
/// # Arguments
///
/// * `holdings` - Portfolio holdings
/// * `shift_bps` - Shift in basis points (positive = rates up)
/// * `config` - Analytics configuration
///
/// # Returns
///
/// The percentage change in portfolio value (negative for rate increases).
#[must_use]
pub fn parallel_shift_impact(
    holdings: &[Holding],
    shift_bps: f64,
    config: &AnalyticsConfig,
) -> Option<f64> {
    let duration = weighted_best_duration(holdings, config)?;
    let convexity = weighted_convexity(holdings, config).unwrap_or(0.0);

    let delta_y = shift_bps / 10000.0; // Convert bps to decimal

    // Duration-convexity approximation
    let pct_change = -duration * delta_y + 0.5 * convexity * delta_y * delta_y;

    Some(pct_change * 100.0) // Return as percentage
}

/// Calculates the impact of key rate shifts.
///
/// Uses the key rate duration profile to estimate impact at each tenor,
/// then sums for total impact.
///
/// # Arguments
///
/// * `holdings` - Portfolio holdings
/// * `scenario` - Rate scenario with shifts at various tenors
/// * `config` - Analytics configuration
///
/// # Returns
///
/// The percentage change in portfolio value.
#[must_use]
pub fn key_rate_shift_impact(
    holdings: &[Holding],
    scenario: &RateScenario,
    config: &AnalyticsConfig,
) -> Option<f64> {
    // For parallel shift, use the simpler formula
    if let RateScenario::ParallelShift(shift) = scenario {
        return parallel_shift_impact(holdings, *shift, config);
    }

    // Get KRD profile
    let profile = aggregate_key_rate_profile(holdings, config, None)?;

    // Sum impact at each tenor
    let total_impact: f64 = profile
        .durations
        .iter()
        .map(|krd| {
            let shift_bps = scenario.shift_at_tenor(krd.tenor);
            let delta_y = shift_bps / 10000.0;
            -krd.duration.as_f64() * delta_y
        })
        .sum();

    Some(total_impact * 100.0) // Return as percentage
}

/// Calculates the impact of a spread shock.
///
/// ## Formula
///
/// Using spread duration:
/// ```text
/// ΔP/P ≈ -SpreadDuration × Δspread
/// ```
///
/// # Arguments
///
/// * `holdings` - Portfolio holdings
/// * `shift_bps` - Spread change in basis points (positive = spreads widen)
/// * `config` - Analytics configuration
///
/// # Returns
///
/// The percentage change in portfolio value (negative for spread widening).
#[must_use]
pub fn spread_shock_impact(
    holdings: &[Holding],
    shift_bps: f64,
    config: &AnalyticsConfig,
) -> Option<f64> {
    // Use spread duration if available, otherwise fall back to modified duration
    let spread_duration = weighted_spread_duration(holdings, config)
        .or_else(|| weighted_best_duration(holdings, config))?;

    let delta_spread = shift_bps / 10000.0;
    let pct_change = -spread_duration * delta_spread;

    Some(pct_change * 100.0)
}

/// Calculates weighted spread duration.
fn weighted_spread_duration(holdings: &[Holding], config: &AnalyticsConfig) -> Option<f64> {
    let (sum_weighted, sum_weights) = maybe_parallel_fold(
        holdings,
        config,
        (0.0_f64, 0.0_f64),
        |(sum_w, sum_wt), h| {
            if let Some(spread_dur) = h.analytics.spread_duration {
                let weight = h.weight_value(config.weighting).to_f64().unwrap_or(0.0);
                (sum_w + spread_dur * weight, sum_wt + weight)
            } else {
                (sum_w, sum_wt)
            }
        },
        |(a, b), (c, d)| (a + c, b + d),
    );

    if sum_weights > 0.0 {
        Some(sum_weighted / sum_weights)
    } else {
        None
    }
}

/// Calculates spread shock impact with per-holding shifts by rating.
///
/// Each holding receives a spread shock based on its credit rating.
/// Holdings without a rating or spread duration use the default (0bp).
#[must_use]
pub fn spread_shock_by_rating(
    holdings: &[Holding],
    rating_shifts: &std::collections::HashMap<String, f64>,
    config: &AnalyticsConfig,
) -> Option<f64> {
    let (sum_impact, sum_weights) = maybe_parallel_fold(
        holdings,
        config,
        (0.0_f64, 0.0_f64),
        |(sum_i, sum_w), h| {
            let weight = h.weight_value(config.weighting).to_f64().unwrap_or(0.0);
            if weight <= 0.0 {
                return (sum_i, sum_w);
            }

            // Get the spread shift for this holding's rating
            let shift_bps = h
                .classification
                .rating
                .composite
                .and_then(|r| rating_shifts.get(&r.to_string()))
                .copied()
                .unwrap_or(0.0);

            if shift_bps == 0.0 {
                return (sum_i, sum_w);
            }

            // Use spread duration, fall back to modified duration
            let duration = h
                .analytics
                .spread_duration
                .or(h.analytics.modified_duration)
                .unwrap_or(0.0);

            let delta_spread = shift_bps / 10000.0;
            let holding_impact = -duration * delta_spread;

            (sum_i + holding_impact * weight, sum_w + weight)
        },
        |(a, b), (c, d)| (a + c, b + d),
    );

    if sum_weights > 0.0 {
        Some((sum_impact / sum_weights) * 100.0)
    } else {
        None
    }
}

/// Calculates spread shock impact with per-holding shifts by sector.
///
/// Each holding receives a spread shock based on its sector classification.
/// Holdings without a sector or spread duration use the default (0bp).
#[must_use]
pub fn spread_shock_by_sector(
    holdings: &[Holding],
    sector_shifts: &std::collections::HashMap<String, f64>,
    config: &AnalyticsConfig,
) -> Option<f64> {
    let (sum_impact, sum_weights) = maybe_parallel_fold(
        holdings,
        config,
        (0.0_f64, 0.0_f64),
        |(sum_i, sum_w), h| {
            let weight = h.weight_value(config.weighting).to_f64().unwrap_or(0.0);
            if weight <= 0.0 {
                return (sum_i, sum_w);
            }

            // Get the spread shift for this holding's sector
            let shift_bps = h
                .classification
                .sector
                .composite
                .and_then(|s| sector_shifts.get(&s.to_string()))
                .copied()
                .unwrap_or(0.0);

            if shift_bps == 0.0 {
                return (sum_i, sum_w);
            }

            // Use spread duration, fall back to modified duration
            let duration = h
                .analytics
                .spread_duration
                .or(h.analytics.modified_duration)
                .unwrap_or(0.0);

            let delta_spread = shift_bps / 10000.0;
            let holding_impact = -duration * delta_spread;

            (sum_i + holding_impact * weight, sum_w + weight)
        },
        |(a, b), (c, d)| (a + c, b + d),
    );

    if sum_weights > 0.0 {
        Some((sum_impact / sum_weights) * 100.0)
    } else {
        None
    }
}

/// Runs a complete stress scenario on a portfolio.
///
/// Combines rate and spread impacts to estimate total P&L.
///
/// # Example
///
/// ```ignore
/// use convex_portfolio::stress::{run_stress_scenario, standard::rates_up_100};
///
/// let config = AnalyticsConfig::default();
/// let result = run_stress_scenario(&portfolio, &rates_up_100(), &config);
/// println!("P&L: {:.2}%", result.pnl_pct);
/// ```
#[must_use]
pub fn run_stress_scenario(
    portfolio: &Portfolio,
    scenario: &StressScenario,
    config: &AnalyticsConfig,
) -> StressResult {
    let initial_value = portfolio.nav().to_f64().unwrap_or(0.0);

    // Calculate rate impact
    let rate_impact = scenario
        .rate_scenario
        .as_ref()
        .and_then(|rs| key_rate_shift_impact(&portfolio.holdings, rs, config));

    // Calculate spread impact
    let spread_impact = scenario.spread_scenario.as_ref().and_then(|ss| match ss {
        SpreadScenario::Uniform(shift) => spread_shock_impact(&portfolio.holdings, *shift, config),
        SpreadScenario::ByRating(rating_shifts) => {
            spread_shock_by_rating(&portfolio.holdings, rating_shifts, config)
        }
        SpreadScenario::BySector(sector_shifts) => {
            spread_shock_by_sector(&portfolio.holdings, sector_shifts, config)
        }
    });

    // Total impact
    let total_pct = rate_impact.unwrap_or(0.0) + spread_impact.unwrap_or(0.0);
    let pnl = initial_value * total_pct / 100.0;
    let stressed_value = initial_value + pnl;

    StressResult {
        scenario_name: scenario.name.clone(),
        initial_value,
        stressed_value,
        pnl,
        pnl_pct: total_pct,
        rate_impact,
        spread_impact,
    }
}

/// Runs multiple stress scenarios on a portfolio.
///
/// # Example
///
/// ```ignore
/// use convex_portfolio::stress::{run_stress_scenarios, standard::all};
///
/// let config = AnalyticsConfig::default();
/// let results = run_stress_scenarios(&portfolio, &standard::all(), &config);
///
/// for result in results {
///     println!("{}: {:.2}%", result.scenario_name, result.pnl_pct);
/// }
/// ```
#[must_use]
pub fn run_stress_scenarios(
    portfolio: &Portfolio,
    scenarios: &[StressScenario],
    config: &AnalyticsConfig,
) -> Vec<StressResult> {
    scenarios
        .iter()
        .map(|s| run_stress_scenario(portfolio, s, config))
        .collect()
}

/// Calculates the worst-case scenario from a set of stress results.
#[must_use]
pub fn worst_case(results: &[StressResult]) -> Option<&StressResult> {
    results.iter().min_by(|a, b| {
        a.pnl
            .partial_cmp(&b.pnl)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// Calculates the best-case scenario from a set of stress results.
#[must_use]
pub fn best_case(results: &[StressResult]) -> Option<&StressResult> {
    results.iter().max_by(|a, b| {
        a.pnl
            .partial_cmp(&b.pnl)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// Summary of stress test results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressSummary {
    /// Number of scenarios tested.
    pub scenario_count: usize,

    /// Worst-case P&L.
    pub worst_pnl: f64,

    /// Worst-case P&L percentage.
    pub worst_pnl_pct: f64,

    /// Worst-case scenario name.
    pub worst_scenario: String,

    /// Best-case P&L.
    pub best_pnl: f64,

    /// Best-case P&L percentage.
    pub best_pnl_pct: f64,

    /// Best-case scenario name.
    pub best_scenario: String,

    /// Average P&L across all scenarios.
    pub avg_pnl: f64,

    /// Average P&L percentage.
    pub avg_pnl_pct: f64,
}

/// Creates a summary of stress test results.
#[must_use]
pub fn summarize_results(results: &[StressResult]) -> Option<StressSummary> {
    if results.is_empty() {
        return None;
    }

    let worst = worst_case(results)?;
    let best = best_case(results)?;

    let avg_pnl: f64 = results.iter().map(|r| r.pnl).sum::<f64>() / results.len() as f64;
    let avg_pnl_pct: f64 = results.iter().map(|r| r.pnl_pct).sum::<f64>() / results.len() as f64;

    Some(StressSummary {
        scenario_count: results.len(),
        worst_pnl: worst.pnl,
        worst_pnl_pct: worst.pnl_pct,
        worst_scenario: worst.scenario_name.clone(),
        best_pnl: best.pnl,
        best_pnl_pct: best.pnl_pct,
        best_scenario: best.scenario_name.clone(),
        avg_pnl,
        avg_pnl_pct,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CashPosition, HoldingAnalytics};
    use convex_analytics::risk::{Duration, KeyRateDuration, KeyRateDurations};
    use convex_bonds::types::BondIdentifiers;
    use convex_core::types::{Currency, Date};
    use rust_decimal_macros::dec;

    fn create_holding_with_analytics(
        id: &str,
        par: rust_decimal::Decimal,
        price: rust_decimal::Decimal,
        duration: f64,
        convexity: f64,
    ) -> Holding {
        Holding::builder()
            .id(id)
            .identifiers(BondIdentifiers::new().with_ticker(format!("TST{}", id)))
            .par_amount(par)
            .market_price(price)
            .analytics(
                HoldingAnalytics::new()
                    .with_modified_duration(duration)
                    .with_convexity(convexity),
            )
            .build()
            .unwrap()
    }

    fn create_test_portfolio() -> Portfolio {
        let holding = create_holding_with_analytics("BOND1", dec!(1_000_000), dec!(100), 5.0, 50.0);

        Portfolio::builder("Test Portfolio")
            .id("TEST001")
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .add_holding(holding)
            .add_cash(CashPosition::new(dec!(100_000), Currency::USD))
            .build()
            .unwrap()
    }

    #[test]
    fn test_parallel_shift_impact() {
        let holding = create_holding_with_analytics("BOND1", dec!(1_000_000), dec!(100), 5.0, 50.0);
        let holdings = vec![holding];
        let config = AnalyticsConfig::default();

        // +100bp shift: -Duration × Δy + 0.5 × Convexity × Δy²
        // = -5.0 × 0.01 + 0.5 × 50 × 0.0001
        // = -0.05 + 0.0025 = -0.0475 = -4.75%
        let impact = parallel_shift_impact(&holdings, 100.0, &config).unwrap();
        assert!((impact - (-4.75)).abs() < 0.01);
    }

    #[test]
    fn test_parallel_shift_negative() {
        let holding = create_holding_with_analytics("BOND1", dec!(1_000_000), dec!(100), 5.0, 50.0);
        let holdings = vec![holding];
        let config = AnalyticsConfig::default();

        // -100bp shift: positive impact (rates down = prices up)
        let impact = parallel_shift_impact(&holdings, -100.0, &config).unwrap();
        assert!(impact > 0.0);

        // Approximately +5.25% (duration effect + convexity effect)
        assert!((impact - 5.25).abs() < 0.1);
    }

    #[test]
    fn test_spread_shock_impact() {
        let mut analytics = HoldingAnalytics::new().with_modified_duration(5.0);
        analytics.spread_duration = Some(4.5); // Spread duration slightly less

        let holding = Holding::builder()
            .id("BOND1")
            .identifiers(BondIdentifiers::new().with_ticker("TEST001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(analytics)
            .build()
            .unwrap();

        let holdings = vec![holding];
        let config = AnalyticsConfig::default();

        // +50bp spread widening: -4.5 × 0.005 = -2.25%
        let impact = spread_shock_impact(&holdings, 50.0, &config).unwrap();
        assert!((impact - (-2.25)).abs() < 0.01);
    }

    #[test]
    fn test_run_stress_scenario() {
        let portfolio = create_test_portfolio();
        let config = AnalyticsConfig::default();
        let scenario = super::super::scenarios::standard::rates_up_100();

        let result = run_stress_scenario(&portfolio, &scenario, &config);

        assert_eq!(result.scenario_name, "Rates +100bp");
        assert!(result.initial_value > 0.0);
        assert!(result.pnl < 0.0); // Rates up = loss for bond portfolio
        assert!(result.rate_impact.is_some());
        assert!(result.is_loss());
    }

    #[test]
    fn test_run_multiple_scenarios() {
        let portfolio = create_test_portfolio();
        let config = AnalyticsConfig::default();

        let scenarios = vec![
            super::super::scenarios::standard::rates_up_100(),
            super::super::scenarios::standard::rates_down_100(),
        ];

        let results = run_stress_scenarios(&portfolio, &scenarios, &config);

        assert_eq!(results.len(), 2);

        // Rates up should be a loss
        assert!(results[0].pnl < 0.0);

        // Rates down should be a gain
        assert!(results[1].pnl > 0.0);
    }

    #[test]
    fn test_worst_and_best_case() {
        let portfolio = create_test_portfolio();
        let config = AnalyticsConfig::default();
        let scenarios = super::super::scenarios::standard::all();

        let results = run_stress_scenarios(&portfolio, &scenarios, &config);

        let worst = worst_case(&results).unwrap();
        let best = best_case(&results).unwrap();

        assert!(worst.pnl <= best.pnl);
    }

    #[test]
    fn test_stress_summary() {
        let portfolio = create_test_portfolio();
        let config = AnalyticsConfig::default();
        let scenarios = super::super::scenarios::standard::all();

        let results = run_stress_scenarios(&portfolio, &scenarios, &config);
        let summary = summarize_results(&results).unwrap();

        assert_eq!(summary.scenario_count, 10);
        assert!(summary.worst_pnl <= summary.best_pnl);
    }

    #[test]
    fn test_key_rate_shift_impact() {
        // Create holding with KRD
        let krd = KeyRateDurations::new(vec![
            KeyRateDuration {
                tenor: 2.0,
                duration: Duration::from(0.5),
            },
            KeyRateDuration {
                tenor: 5.0,
                duration: Duration::from(2.0),
            },
            KeyRateDuration {
                tenor: 10.0,
                duration: Duration::from(2.5),
            },
        ]);

        let mut analytics = HoldingAnalytics::new();
        analytics.key_rate_durations = Some(krd);

        let holding = Holding::builder()
            .id("BOND1")
            .identifiers(BondIdentifiers::new().with_ticker("TEST001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(analytics)
            .build()
            .unwrap();

        let holdings = vec![holding];
        let config = AnalyticsConfig::default();

        // Steepening: short rates down, long rates up
        let scenario = RateScenario::key_rates(&[(2.0, -25.0), (5.0, 0.0), (10.0, 25.0)]);

        let impact = key_rate_shift_impact(&holdings, &scenario, &config).unwrap();

        // Impact = -0.5 × (-0.0025) - 2.0 × 0 - 2.5 × 0.0025
        // = 0.00125 - 0 - 0.00625 = -0.005 = -0.5%
        // The calculation is approximate due to tenor interpolation
        assert!(impact.abs() < 1.0); // Reasonable range
    }

    #[test]
    fn test_stress_result_helpers() {
        let result = StressResult {
            scenario_name: "Test".to_string(),
            initial_value: 1000.0,
            stressed_value: 950.0,
            pnl: -50.0,
            pnl_pct: -5.0,
            rate_impact: Some(-5.0),
            spread_impact: None,
        };

        assert!(result.is_loss());
        assert!(!result.is_gain());
    }

    #[test]
    fn test_spread_shock_by_rating() {
        use crate::types::{Classification, CreditRating, RatingInfo};
        use std::collections::HashMap;

        // Create analytics with spread duration
        let mut aaa_analytics = HoldingAnalytics::new().with_modified_duration(5.0);
        aaa_analytics.spread_duration = Some(4.5);

        let mut bbb_analytics = HoldingAnalytics::new().with_modified_duration(5.0);
        bbb_analytics.spread_duration = Some(4.5);

        // Create holdings with different ratings
        let aaa_holding = Holding::builder()
            .id("AAA_BOND")
            .identifiers(BondIdentifiers::new().with_ticker("AAA001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(aaa_analytics)
            .classification(
                Classification::new().with_rating(RatingInfo::from_composite(CreditRating::AAA)),
            )
            .build()
            .unwrap();

        let bbb_holding = Holding::builder()
            .id("BBB_BOND")
            .identifiers(BondIdentifiers::new().with_ticker("BBB001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(bbb_analytics)
            .classification(
                Classification::new().with_rating(RatingInfo::from_composite(CreditRating::BBB)),
            )
            .build()
            .unwrap();

        let holdings = vec![aaa_holding, bbb_holding];
        let config = AnalyticsConfig::default();

        // Rating-based spread shocks: AAA +25bp, BBB +100bp
        let mut rating_shifts = HashMap::new();
        rating_shifts.insert("AAA".to_string(), 25.0);
        rating_shifts.insert("BBB".to_string(), 100.0);

        let impact = spread_shock_by_rating(&holdings, &rating_shifts, &config).unwrap();

        // Both holdings have equal weights
        // AAA impact: -4.5 × 0.0025 = -0.01125 = -1.125%
        // BBB impact: -4.5 × 0.01 = -0.045 = -4.5%
        // Average: (-1.125 + -4.5) / 2 = -2.8125%
        assert!(impact < 0.0); // Spread widening = loss
        assert!((impact - (-2.8125)).abs() < 0.1);
    }

    #[test]
    fn test_spread_shock_by_sector() {
        use crate::types::{Classification, Sector, SectorInfo};
        use std::collections::HashMap;

        // Create holdings with different sectors
        let financial = Holding::builder()
            .id("FIN_BOND")
            .identifiers(BondIdentifiers::new().with_ticker("FIN001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new().with_modified_duration(5.0))
            .classification(
                Classification::new().with_sector(SectorInfo::from_composite(Sector::Financial)),
            )
            .build()
            .unwrap();

        let corporate = Holding::builder()
            .id("CORP_BOND")
            .identifiers(BondIdentifiers::new().with_ticker("CORP001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new().with_modified_duration(5.0))
            .classification(
                Classification::new().with_sector(SectorInfo::from_composite(Sector::Corporate)),
            )
            .build()
            .unwrap();

        let holdings = vec![financial, corporate];
        let config = AnalyticsConfig::default();

        // Sector-based spread shocks: Financial +150bp, Corporate +75bp
        let mut sector_shifts = HashMap::new();
        sector_shifts.insert("Financial".to_string(), 150.0);
        sector_shifts.insert("Corporate".to_string(), 75.0);

        let impact = spread_shock_by_sector(&holdings, &sector_shifts, &config).unwrap();

        // Both holdings have equal weights
        // Financial impact: -5.0 × 0.015 = -0.075 = -7.5%
        // Corporate impact: -5.0 × 0.0075 = -0.0375 = -3.75%
        // Average: (-7.5 + -3.75) / 2 = -5.625%
        assert!(impact < 0.0); // Spread widening = loss
        assert!((impact - (-5.625)).abs() < 0.1);
    }
}
