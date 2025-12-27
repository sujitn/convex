//! Property-based tests for portfolio invariants.
//!
//! These tests verify key mathematical properties that should always hold:
//! - Weights sum to 100%
//! - NAV = sum of components
//! - Risk contributions sum to total
//! - Bucketing covers all holdings

use convex_bonds::types::BondIdentifiers;
use convex_core::types::{Currency, Date};
use convex_portfolio::prelude::*;
use convex_portfolio::types::{Classification, RatingInfo, SectorInfo};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;

// =============================================================================
// TEST DATA GENERATORS
// =============================================================================

/// Generates a portfolio with N holdings with varying characteristics.
fn generate_portfolio(n: usize, seed: u64) -> Portfolio {
    let mut holdings = Vec::with_capacity(n);
    let sectors = [
        Sector::Government,
        Sector::Corporate,
        Sector::Financial,
        Sector::Utility,
        Sector::Municipal,
    ];
    let ratings = [
        CreditRating::AAA,
        CreditRating::AA,
        CreditRating::A,
        CreditRating::BBB,
        CreditRating::BB,
        CreditRating::B,
    ];

    for i in 0..n {
        // Use deterministic pseudo-random values based on seed and index
        let hash = simple_hash(seed, i as u64);

        let par = Decimal::from(100_000 + (hash % 900_000) as i64);
        let price = Decimal::from(90 + (hash % 20) as i64);
        let ytm = 0.02 + (hash % 600) as f64 / 10000.0; // 2-8%
        let duration = 1.0 + (hash % 180) as f64 / 10.0; // 1-19 years
        let convexity = duration * 0.06 + (hash % 100) as f64 / 1000.0;
        let z_spread = (hash % 400) as f64; // 0-400bp

        let sector = sectors[hash as usize % sectors.len()];
        let rating = ratings[hash as usize % ratings.len()];

        let mut classification = Classification::new();
        classification = classification.with_sector(SectorInfo::from_composite(sector));
        classification = classification.with_rating(RatingInfo::from_composite(rating));

        let holding = HoldingBuilder::new()
            .id(format!("H{}", i))
            .identifiers(BondIdentifiers::from_isin_str("US912828Z229").unwrap())
            .par_amount(par)
            .market_price(price)
            .classification(classification)
            .analytics(
                HoldingAnalytics::new()
                    .with_ytm(ytm)
                    .with_modified_duration(duration)
                    .with_convexity(convexity)
                    .with_z_spread(z_spread)
                    .with_dv01(duration * par.to_f64().unwrap() / 10000.0 / 100.0),
            )
            .build()
            .unwrap();

        holdings.push(holding);
    }

    // Add some cash
    let cash_amount = Decimal::from(50_000 + (simple_hash(seed, 999) % 200_000) as i64);

    PortfolioBuilder::new()
        .name(format!("TestPortfolio_{}", seed))
        .base_currency(Currency::USD)
        .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
        .add_holdings(holdings)
        .add_cash(CashPosition::new(cash_amount, Currency::USD))
        .shares_outstanding(dec!(1_000_000))
        .build()
        .unwrap()
}

/// Simple deterministic hash for test data generation.
fn simple_hash(seed: u64, i: u64) -> u64 {
    let mut x = seed.wrapping_add(i).wrapping_mul(0x517cc1b727220a95);
    x ^= x >> 32;
    x = x.wrapping_mul(0x517cc1b727220a95);
    x ^= x >> 32;
    x
}

// =============================================================================
// PROPERTY: WEIGHTS SUM TO 100%
// =============================================================================

#[test]
fn property_sector_weights_sum_to_100() {
    let config = AnalyticsConfig::default();

    for seed in 0..10 {
        for size in [5, 10, 25, 50, 100] {
            let portfolio = generate_portfolio(size, seed);
            let dist = bucket_by_sector(&portfolio.holdings, &config);

            let total: f64 = dist.by_sector.values().map(|m| m.weight_pct).sum();

            assert!(
                (total - 100.0).abs() < 0.01,
                "Sector weights should sum to 100%, got {} for size={}, seed={}",
                total,
                size,
                seed
            );
        }
    }
}

#[test]
fn property_rating_weights_sum_to_100() {
    let config = AnalyticsConfig::default();

    for seed in 0..10 {
        for size in [5, 10, 25, 50] {
            let portfolio = generate_portfolio(size, seed);
            let dist = bucket_by_rating(&portfolio.holdings, &config);

            let total: f64 = dist.by_bucket.values().map(|m| m.weight_pct).sum();

            assert!(
                (total - 100.0).abs() < 0.01,
                "Rating weights should sum to 100%, got {} for size={}, seed={}",
                total,
                size,
                seed
            );
        }
    }
}

#[test]
fn property_duration_contributions_sum_to_100() {
    let config = AnalyticsConfig::default();

    for seed in 0..10 {
        for size in [5, 10, 25, 50] {
            let portfolio = generate_portfolio(size, seed);
            let contrib = convex_portfolio::contribution::duration_contributions(
                &portfolio.holdings,
                &config,
            );

            let total: f64 = contrib.by_holding.iter().map(|c| c.contribution_pct).sum();

            assert!(
                (total - 100.0).abs() < 0.5,
                "Duration contributions should sum to 100%, got {} for size={}, seed={}",
                total,
                size,
                seed
            );
        }
    }
}

#[test]
fn property_dv01_contributions_sum_to_100() {
    let config = AnalyticsConfig::default();

    for seed in 0..10 {
        for size in [5, 10, 25] {
            let portfolio = generate_portfolio(size, seed);
            let contrib =
                convex_portfolio::contribution::dv01_contributions(&portfolio.holdings, &config);

            let total: f64 = contrib.by_holding.iter().map(|c| c.contribution_pct).sum();

            assert!(
                (total - 100.0).abs() < 0.5,
                "DV01 contributions should sum to 100%, got {} for size={}, seed={}",
                total,
                size,
                seed
            );
        }
    }
}

// =============================================================================
// PROPERTY: NAV = SUM OF COMPONENTS
// =============================================================================

#[test]
fn property_nav_equals_components() {
    for seed in 0..20 {
        for size in [1, 5, 10, 25, 50] {
            let portfolio = generate_portfolio(size, seed);

            let nav = portfolio.nav();
            let securities = portfolio.securities_market_value();
            let accrued = portfolio.total_accrued_interest();
            let cash = portfolio.total_cash();
            let liabilities = portfolio.total_liabilities();

            let computed = securities + accrued + cash - liabilities;

            assert!(
                (nav - computed).abs() < dec!(0.01),
                "NAV should equal sum of components: {} vs {} for size={}, seed={}",
                nav,
                computed,
                size,
                seed
            );
        }
    }
}

// =============================================================================
// PROPERTY: TOTAL DV01 = SUM OF HOLDING DV01S
// =============================================================================

#[test]
fn property_dv01_is_sum() {
    let config = AnalyticsConfig::default();

    for seed in 0..10 {
        for size in [5, 10, 25, 50] {
            let portfolio = generate_portfolio(size, seed);

            // Sum holding DV01s
            let sum_dv01: Decimal = portfolio
                .holdings
                .iter()
                .filter_map(|h| h.total_dv01())
                .sum();
            let sum_dv01 = sum_dv01.to_f64().unwrap_or(0.0);

            // Portfolio DV01
            let risk = convex_portfolio::analytics::calculate_risk_metrics(
                &portfolio.holdings,
                None,
                &config,
            );

            assert!(
                (risk.total_dv01 - sum_dv01).abs() < 1.0,
                "Total DV01 should equal sum of holdings: {} vs {} for size={}, seed={}",
                risk.total_dv01,
                sum_dv01,
                size,
                seed
            );
        }
    }
}

// =============================================================================
// PROPERTY: WEIGHTED AVERAGES ARE WITHIN BOUNDS
// =============================================================================

#[test]
fn property_weighted_ytm_within_bounds() {
    let config = AnalyticsConfig::default();

    for seed in 0..10 {
        for size in [5, 10, 25] {
            let portfolio = generate_portfolio(size, seed);

            // Find min and max YTM
            let ytms: Vec<f64> = portfolio
                .holdings
                .iter()
                .filter_map(|h| h.analytics.ytm)
                .collect();

            if ytms.is_empty() {
                continue;
            }

            let min_ytm = ytms.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_ytm = ytms.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

            let yield_metrics = calculate_yield_metrics(&portfolio.holdings, &config);

            if let Some(wtd_ytm) = yield_metrics.ytm {
                assert!(
                    wtd_ytm >= min_ytm - 0.0001 && wtd_ytm <= max_ytm + 0.0001,
                    "Weighted YTM should be within [min, max]: {} not in [{}, {}] for size={}, seed={}",
                    wtd_ytm, min_ytm, max_ytm, size, seed
                );
            }
        }
    }
}

#[test]
fn property_weighted_duration_within_bounds() {
    let config = AnalyticsConfig::default();

    for seed in 0..10 {
        for size in [5, 10, 25] {
            let portfolio = generate_portfolio(size, seed);

            // Find min and max duration
            let durations: Vec<f64> = portfolio
                .holdings
                .iter()
                .filter_map(|h| h.analytics.modified_duration)
                .collect();

            if durations.is_empty() {
                continue;
            }

            let min_dur = durations.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_dur = durations.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

            let risk_metrics = convex_portfolio::analytics::calculate_risk_metrics(
                &portfolio.holdings,
                None,
                &config,
            );

            if let Some(wtd_dur) = risk_metrics.best_duration {
                assert!(
                    wtd_dur >= min_dur - 0.01 && wtd_dur <= max_dur + 0.01,
                    "Weighted duration should be within [min, max]: {} not in [{}, {}] for size={}, seed={}",
                    wtd_dur, min_dur, max_dur, size, seed
                );
            }
        }
    }
}

// =============================================================================
// PROPERTY: BUCKETING COVERS ALL HOLDINGS
// =============================================================================

#[test]
fn property_sector_bucketing_covers_all() {
    let config = AnalyticsConfig::default();

    for seed in 0..10 {
        for size in [5, 10, 25, 50] {
            let portfolio = generate_portfolio(size, seed);
            let dist = bucket_by_sector(&portfolio.holdings, &config);

            // Count total holdings in all buckets
            let bucket_count: usize = dist.by_sector.values().map(|m| m.count).sum();

            assert_eq!(
                bucket_count,
                portfolio.holding_count(),
                "Sector bucketing should cover all holdings: {} vs {} for size={}, seed={}",
                bucket_count,
                portfolio.holding_count(),
                size,
                seed
            );
        }
    }
}

#[test]
fn property_rating_bucketing_covers_all() {
    let config = AnalyticsConfig::default();

    for seed in 0..10 {
        for size in [5, 10, 25] {
            let portfolio = generate_portfolio(size, seed);
            let dist = bucket_by_rating(&portfolio.holdings, &config);

            // Count total holdings in all buckets
            let bucket_count: usize = dist.by_bucket.values().map(|m| m.count).sum();

            assert_eq!(
                bucket_count,
                portfolio.holding_count(),
                "Rating bucketing should cover all holdings: {} vs {} for size={}, seed={}",
                bucket_count,
                portfolio.holding_count(),
                size,
                seed
            );
        }
    }
}

// =============================================================================
// PROPERTY: STRESS TEST SIGNS ARE CORRECT
// =============================================================================

#[test]
fn property_rate_increase_hurts() {
    let config = AnalyticsConfig::default();

    for seed in 0..10 {
        let portfolio = generate_portfolio(20, seed);

        // +100bp parallel shift
        let impact =
            convex_portfolio::stress::parallel_shift_impact(&portfolio.holdings, 100.0, &config);

        // Duration > 0, so rising rates should hurt
        if let Some(total_pnl) = impact {
            assert!(
                total_pnl <= 0.0,
                "Rising rates should cause negative P&L: {} for seed={}",
                total_pnl,
                seed
            );
        }
    }
}

#[test]
fn property_rate_decrease_helps() {
    let config = AnalyticsConfig::default();

    for seed in 0..10 {
        let portfolio = generate_portfolio(20, seed);

        // -100bp parallel shift
        let impact =
            convex_portfolio::stress::parallel_shift_impact(&portfolio.holdings, -100.0, &config);

        // Duration > 0, so falling rates should help
        if let Some(total_pnl) = impact {
            assert!(
                total_pnl >= 0.0,
                "Falling rates should cause positive P&L: {} for seed={}",
                total_pnl,
                seed
            );
        }
    }
}

#[test]
fn property_spread_widening_hurts_credit() {
    let config = AnalyticsConfig::default();

    for seed in 0..10 {
        let portfolio = generate_portfolio(20, seed);

        // +50bp spread widening
        let impact =
            convex_portfolio::stress::spread_shock_impact(&portfolio.holdings, 50.0, &config);

        // Spread widening should hurt (assuming spread duration > 0)
        if let Some(total_pnl) = impact {
            assert!(
                total_pnl <= 0.0,
                "Spread widening should cause negative P&L: {} for seed={}",
                total_pnl,
                seed
            );
        }
    }
}

// =============================================================================
// PROPERTY: ETF NAV PER SHARE IS CONSISTENT
// =============================================================================

#[test]
fn property_nav_per_share_consistency() {
    for seed in 0..10 {
        for size in [5, 10, 25] {
            let portfolio = generate_portfolio(size, seed);

            if let Some(shares) = portfolio.shares_outstanding {
                let nav = portfolio.nav();
                let nav_per_share = portfolio.nav_per_share();

                assert!(nav_per_share.is_some());
                let nps = nav_per_share.unwrap();

                // NAV per share × shares = total NAV
                let computed_nav = nps * shares;

                assert!(
                    (computed_nav - nav).abs() < dec!(1.0),
                    "NAV per share × shares should equal NAV: {} × {} = {} vs {} for size={}, seed={}",
                    nps, shares, computed_nav, nav, size, seed
                );
            }
        }
    }
}

// =============================================================================
// PROPERTY: BENCHMARK COMPARISON IS SYMMETRIC
// =============================================================================

#[test]
fn property_active_weights_are_antisymmetric() {
    let config = AnalyticsConfig::default();

    for seed in 0..5 {
        let portfolio_a = generate_portfolio(15, seed);
        let portfolio_b = generate_portfolio(15, seed + 100);

        // Compare A to B
        let comparison_ab = convex_portfolio::benchmark::benchmark_comparison(
            &portfolio_a.holdings,
            &portfolio_b.holdings,
            &config,
        );

        // Compare B to A
        let comparison_ba = convex_portfolio::benchmark::benchmark_comparison(
            &portfolio_b.holdings,
            &portfolio_a.holdings,
            &config,
        );

        // Duration difference should be opposite
        if let (Some(diff_ab), Some(diff_ba)) = (
            comparison_ab.duration.difference,
            comparison_ba.duration.difference,
        ) {
            assert!(
                (diff_ab + diff_ba).abs() < 0.01,
                "Duration diff should be antisymmetric: {} vs {} for seed={}",
                diff_ab,
                diff_ba,
                seed
            );
        }

        // Spread difference should be opposite
        if let (Some(diff_ab), Some(diff_ba)) = (
            comparison_ab.spread.difference,
            comparison_ba.spread.difference,
        ) {
            assert!(
                (diff_ab + diff_ba).abs() < 0.01,
                "Spread diff should be antisymmetric: {} vs {} for seed={}",
                diff_ab,
                diff_ba,
                seed
            );
        }
    }
}

// =============================================================================
// PROPERTY: IDENTICAL PORTFOLIOS HAVE ZERO ACTIVE WEIGHTS
// =============================================================================

#[test]
fn property_identical_portfolios_zero_diff() {
    let config = AnalyticsConfig::default();

    for seed in 0..10 {
        let portfolio = generate_portfolio(20, seed);

        let comparison = convex_portfolio::benchmark::benchmark_comparison(
            &portfolio.holdings,
            &portfolio.holdings,
            &config,
        );

        // Duration difference should be zero
        if let Some(diff) = comparison.duration.difference {
            assert!(
                diff.abs() < 0.001,
                "Identical portfolios should have zero duration diff: {} for seed={}",
                diff,
                seed
            );
        }

        // Spread difference should be zero
        if let Some(diff) = comparison.spread.difference {
            assert!(
                diff.abs() < 0.001,
                "Identical portfolios should have zero spread diff: {} for seed={}",
                diff,
                seed
            );
        }

        // Active weights should all be zero
        assert_eq!(
            comparison.active_weights.overweight_count, 0,
            "Identical portfolios should have no overweights for seed={}",
            seed
        );
        assert_eq!(
            comparison.active_weights.underweight_count, 0,
            "Identical portfolios should have no underweights for seed={}",
            seed
        );
    }
}
