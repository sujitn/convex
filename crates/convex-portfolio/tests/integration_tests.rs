//! Integration tests for convex-portfolio.
//!
//! These tests verify end-to-end functionality with realistic portfolios.

use convex_bonds::types::BondIdentifiers;
use convex_core::types::{Currency, Date};
use convex_portfolio::prelude::*;
use convex_portfolio::types::{Classification, RatingInfo, SectorInfo};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;

// =============================================================================
// TEST FIXTURES
// =============================================================================

/// Creates a realistic corporate bond portfolio with ~10 holdings.
fn create_corporate_portfolio() -> Portfolio {
    let holdings = vec![
        // Investment grade corporates
        create_holding(
            "AAPL-2030",
            dec!(2_000_000),
            dec!(98.50),
            0.045,
            5.2,
            0.35,
            120.0,
            Some(Sector::Corporate),
            Some(CreditRating::AAPlus),
        ),
        create_holding(
            "MSFT-2028",
            dec!(1_500_000),
            dec!(101.25),
            0.038,
            4.1,
            0.28,
            95.0,
            Some(Sector::Corporate),
            Some(CreditRating::AAA),
        ),
        create_holding(
            "JPM-2032",
            dec!(1_000_000),
            dec!(96.75),
            0.052,
            6.8,
            0.45,
            145.0,
            Some(Sector::Financial),
            Some(CreditRating::A),
        ),
        create_holding(
            "GS-2029",
            dec!(800_000),
            dec!(97.00),
            0.048,
            5.5,
            0.38,
            135.0,
            Some(Sector::Financial),
            Some(CreditRating::APlus),
        ),
        // High yield
        create_holding(
            "HYG-2027",
            dec!(500_000),
            dec!(92.00),
            0.072,
            4.2,
            0.32,
            350.0,
            Some(Sector::Corporate),
            Some(CreditRating::BB),
        ),
        create_holding(
            "DELL-2026",
            dec!(600_000),
            dec!(94.50),
            0.065,
            3.8,
            0.28,
            280.0,
            Some(Sector::Corporate),
            Some(CreditRating::BBBMinus),
        ),
        // Utilities
        create_holding(
            "NEE-2031",
            dec!(700_000),
            dec!(99.00),
            0.042,
            5.8,
            0.40,
            110.0,
            Some(Sector::Utility),
            Some(CreditRating::A),
        ),
        // Government
        create_holding(
            "UST-2030",
            dec!(3_000_000),
            dec!(99.50),
            0.040,
            5.0,
            0.32,
            0.0,
            Some(Sector::Government),
            Some(CreditRating::AAA),
        ),
    ];

    PortfolioBuilder::new()
        .name("Corporate Bond Fund")
        .base_currency(Currency::USD)
        .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
        .add_holdings(holdings)
        .add_cash(CashPosition::new(dec!(500_000), Currency::USD))
        .build()
        .unwrap()
}

/// Creates a realistic ETF portfolio.
fn create_etf_portfolio() -> Portfolio {
    let holdings = vec![
        create_holding(
            "UST-2Y",
            dec!(5_000_000),
            dec!(99.80),
            0.042,
            1.9,
            0.04,
            0.0,
            Some(Sector::Government),
            Some(CreditRating::AAA),
        ),
        create_holding(
            "UST-5Y",
            dec!(8_000_000),
            dec!(98.50),
            0.043,
            4.5,
            0.22,
            0.0,
            Some(Sector::Government),
            Some(CreditRating::AAA),
        ),
        create_holding(
            "UST-10Y",
            dec!(6_000_000),
            dec!(95.00),
            0.045,
            8.5,
            0.75,
            0.0,
            Some(Sector::Government),
            Some(CreditRating::AAA),
        ),
        create_holding(
            "UST-30Y",
            dec!(3_000_000),
            dec!(88.00),
            0.048,
            18.5,
            3.80,
            0.0,
            Some(Sector::Government),
            Some(CreditRating::AAA),
        ),
    ];

    PortfolioBuilder::new()
        .name("Treasury ETF")
        .base_currency(Currency::USD)
        .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
        .add_holdings(holdings)
        .add_cash(CashPosition::new(dec!(200_000), Currency::USD))
        .shares_outstanding(dec!(1_000_000))
        .build()
        .unwrap()
}

/// Creates a benchmark portfolio for comparison.
fn create_benchmark_portfolio() -> Portfolio {
    let holdings = vec![
        create_holding(
            "BM-GOV",
            dec!(5_000_000),
            dec!(99.00),
            0.041,
            5.5,
            0.35,
            0.0,
            Some(Sector::Government),
            Some(CreditRating::AAA),
        ),
        create_holding(
            "BM-CORP-IG",
            dec!(3_000_000),
            dec!(97.50),
            0.048,
            5.2,
            0.32,
            125.0,
            Some(Sector::Corporate),
            Some(CreditRating::A),
        ),
        create_holding(
            "BM-FIN",
            dec!(2_000_000),
            dec!(96.00),
            0.052,
            6.0,
            0.40,
            150.0,
            Some(Sector::Financial),
            Some(CreditRating::APlus),
        ),
    ];

    PortfolioBuilder::new()
        .name("Aggregate Bond Index")
        .base_currency(Currency::USD)
        .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
        .add_holdings(holdings)
        .build()
        .unwrap()
}

/// Helper to create a holding with full analytics.
#[allow(clippy::too_many_arguments)]
fn create_holding(
    id: &str,
    par: Decimal,
    price: Decimal,
    ytm: f64,
    duration: f64,
    convexity: f64,
    z_spread: f64,
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

    // Estimate years to maturity from duration (rough approximation)
    let years_to_maturity = duration * 1.1; // Duration is typically ~90% of maturity

    HoldingBuilder::new()
        .id(id)
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
                .with_dv01(duration * par.to_string().parse::<f64>().unwrap() / 10000.0 / 100.0)
                .with_years_to_maturity(years_to_maturity),
        )
        .build()
        .unwrap()
}

// =============================================================================
// PORTFOLIO CONSTRUCTION TESTS
// =============================================================================

#[test]
fn test_portfolio_construction() {
    let portfolio = create_corporate_portfolio();

    assert_eq!(portfolio.holding_count(), 8);
    assert!(!portfolio.is_empty());
    assert!(portfolio.nav() > Decimal::ZERO);
}

#[test]
fn test_portfolio_nav_components() {
    let portfolio = create_corporate_portfolio();

    let securities_mv = portfolio.securities_market_value();
    let cash = portfolio.total_cash();
    let nav = portfolio.nav();

    // NAV = Securities + Cash (no liabilities in this portfolio)
    assert!((nav - securities_mv - cash).abs() < dec!(0.01));
    assert_eq!(cash, dec!(500_000));
}

// =============================================================================
// ANALYTICS TESTS
// =============================================================================

#[test]
fn test_portfolio_analytics_complete() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    let analytics = calculate_portfolio_analytics(&portfolio, &config);

    // Verify all metrics are populated
    assert!(analytics.yields.ytm.is_some());
    assert!(analytics.risk.best_duration.is_some());
    assert!(analytics.spreads.z_spread.is_some());

    // Verify reasonable values
    let ytm = analytics.yields.ytm.unwrap();
    assert!(
        ytm > 0.03 && ytm < 0.08,
        "YTM should be between 3-8%: {}",
        ytm
    );

    let duration = analytics.risk.best_duration.unwrap();
    assert!(
        duration > 3.0 && duration < 10.0,
        "Duration should be 3-10y: {}",
        duration
    );
}

#[test]
fn test_yield_metrics() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    let yield_metrics = calculate_yield_metrics(&portfolio.holdings, &config);

    // All holdings have YTM, so coverage should be 100%
    assert_eq!(yield_metrics.ytm_coverage, portfolio.holding_count());
    assert!(yield_metrics.ytm.is_some());

    // Portfolio YTM should be weighted average
    let ytm = yield_metrics.ytm.unwrap();
    assert!(ytm > 0.0, "YTM should be positive");
}

#[test]
fn test_risk_metrics() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    let risk_metrics = calculate_risk_metrics(&portfolio.holdings, None, &config);

    // Verify duration is reasonable
    assert!(risk_metrics.best_duration.is_some());
    let duration = risk_metrics.best_duration.unwrap();
    assert!(duration > 0.0, "Duration should be positive");

    // Verify DV01 is calculated
    assert!(
        risk_metrics.total_dv01 > 0.0,
        "Total DV01 should be positive"
    );
}

#[test]
fn test_spread_metrics() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    let spread_metrics = calculate_spread_metrics(&portfolio.holdings, None, &config);

    // Most holdings have spread data
    assert!(spread_metrics.z_spread.is_some());

    // Portfolio spread should be weighted average
    let spread = spread_metrics.z_spread.unwrap();
    assert!(spread >= 0.0, "Spread should be non-negative");
}

// =============================================================================
// BUCKETING TESTS
// =============================================================================

#[test]
fn test_sector_bucketing() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    let sector_dist = bucket_by_sector(&portfolio.holdings, &config);

    // Should have multiple sectors
    assert!(sector_dist.by_sector.len() >= 3);

    // Weights should sum to ~100%
    let total_weight: f64 = sector_dist.by_sector.values().map(|m| m.weight_pct).sum();
    assert!(
        (total_weight - 100.0).abs() < 0.1,
        "Sector weights should sum to 100%"
    );

    // Government should be largest (3M out of ~10M)
    let govt = sector_dist.get(Sector::Government);
    assert!(govt.is_some());
    assert!(govt.unwrap().weight_pct > 20.0);
}

#[test]
fn test_rating_bucketing() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    let rating_dist = bucket_by_rating(&portfolio.holdings, &config);

    // Should have multiple rating buckets
    assert!(rating_dist.by_bucket.len() >= 2);

    // Investment grade should dominate
    let ig_weight = rating_dist.investment_grade_weight();
    assert!(ig_weight > 80.0, "IG weight should be > 80%: {}", ig_weight);
}

#[test]
fn test_maturity_bucketing() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    let maturity_dist = bucket_by_maturity(&portfolio.holdings, &config);

    // Should have holdings in intermediate maturity range (3-10y)
    let intermediate_weight = maturity_dist.intermediate_weight();
    assert!(
        intermediate_weight > 0.0,
        "Should have intermediate maturity holdings"
    );
}

// =============================================================================
// CONTRIBUTION TESTS
// =============================================================================

#[test]
fn test_duration_contribution() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    let contributions =
        convex_portfolio::contribution::duration_contributions(&portfolio.holdings, &config);

    // Contributions should exist for each holding
    assert_eq!(contributions.by_holding.len(), portfolio.holding_count());

    // Sum of contributions should equal 100%
    let total_contrib: f64 = contributions
        .by_holding
        .iter()
        .map(|c| c.contribution_pct)
        .sum();
    assert!(
        (total_contrib - 100.0).abs() < 0.1,
        "Contributions should sum to 100%"
    );
}

#[test]
fn test_contribution_by_sector() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    let contributions =
        convex_portfolio::contribution::duration_contributions(&portfolio.holdings, &config);

    // Should have contributions by sector
    assert!(contributions.by_sector.len() >= 3);

    // Sum by sector should equal 100%
    let sector_total: f64 = contributions
        .by_sector
        .values()
        .map(|c| c.contribution_pct)
        .sum();
    assert!((sector_total - 100.0).abs() < 0.1);
}

// =============================================================================
// BENCHMARK COMPARISON TESTS
// =============================================================================

#[test]
fn test_benchmark_comparison() {
    let portfolio = create_corporate_portfolio();
    let benchmark = create_benchmark_portfolio();
    let config = AnalyticsConfig::default();

    let comparison = convex_portfolio::benchmark::benchmark_comparison(
        &portfolio.holdings,
        &benchmark.holdings,
        &config,
    );

    // Should have duration comparison
    assert!(comparison.duration.portfolio_duration.is_some());
    assert!(comparison.duration.benchmark_duration.is_some());
    assert!(comparison.duration.difference.is_some());

    // Should have spread comparison
    assert!(comparison.spread.portfolio_spread.is_some());
    assert!(comparison.spread.benchmark_spread.is_some());

    // Should have sector-level comparison
    assert!(!comparison.by_sector.is_empty());
}

#[test]
fn test_active_weights() {
    let portfolio = create_corporate_portfolio();
    let benchmark = create_benchmark_portfolio();
    let config = AnalyticsConfig::default();

    let active_wts = convex_portfolio::benchmark::active_weights(
        &portfolio.holdings,
        &benchmark.holdings,
        &config,
    );

    // Should have active weights by sector
    assert!(!active_wts.by_sector.is_empty());
}

// =============================================================================
// STRESS TESTING
// =============================================================================

#[test]
fn test_parallel_rate_shift() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    // +100bp parallel shift
    let impact = parallel_shift_impact(&portfolio.holdings, 100.0, &config);

    // Impact should be negative (rates up = prices down)
    assert!(impact.is_some());
    let pnl_pct = impact.unwrap();
    assert!(pnl_pct < 0.0, "Rising rates should hurt: {}", pnl_pct);
}

#[test]
fn test_spread_shock() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    // +50bp spread widening
    let impact = convex_portfolio::stress::spread_shock_impact(&portfolio.holdings, 50.0, &config);

    // Impact should be negative (spreads widen = prices down)
    assert!(impact.is_some());
    let pnl_pct = impact.unwrap();
    assert!(pnl_pct < 0.0, "Spread widening should hurt: {}", pnl_pct);
}

#[test]
fn test_stress_scenarios() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    let scenarios = convex_portfolio::stress_scenarios::all();
    let results = convex_portfolio::stress::run_stress_scenarios(&portfolio, &scenarios, &config);

    // Should have results for each scenario
    assert_eq!(results.len(), scenarios.len());

    // Find worst case
    let worst = convex_portfolio::stress::worst_case(&results);
    assert!(worst.is_some());
    assert!(worst.unwrap().pnl < 0.0, "Worst case should be negative");
}

// =============================================================================
// ETF ANALYTICS TESTS
// =============================================================================

#[test]
fn test_etf_nav_calculation() {
    let portfolio = create_etf_portfolio();

    let nav_metrics = convex_portfolio::etf::calculate_etf_nav(&portfolio, None);

    // NAV per share should be reasonable
    assert!(nav_metrics.nav_per_share > 0.0);

    // With 1M shares and ~22M NAV, expect ~22 per share
    assert!(nav_metrics.nav_per_share > 15.0 && nav_metrics.nav_per_share < 30.0);
}

#[test]
fn test_etf_premium_discount() {
    let portfolio = create_etf_portfolio();

    // Calculate NAV
    let nav_per_share = portfolio.nav_per_share().unwrap();

    // Trading at 1% premium
    let market_price = nav_per_share * dec!(1.01);
    let market_price_f64 = market_price.to_f64().unwrap();
    let nav_metrics = convex_portfolio::etf::calculate_etf_nav(&portfolio, Some(market_price_f64));

    assert!(nav_metrics.is_premium());
    assert!((nav_metrics.premium_discount_pct.unwrap() - 1.0).abs() < 0.1);
}

#[test]
fn test_creation_basket() {
    let portfolio = create_etf_portfolio();

    let basket = convex_portfolio::etf::build_creation_basket(
        &portfolio.holdings,
        dec!(50_000), // creation unit size
        portfolio.shares_outstanding.unwrap(),
        portfolio.total_cash(),
    );

    // Basket should have same number of components as holdings
    assert_eq!(basket.security_count, portfolio.holding_count());

    // Basket value should scale correctly
    // 50K / 1M shares = 5% of portfolio
    let expected_value = portfolio.nav() * dec!(50_000) / portfolio.shares_outstanding.unwrap();
    let actual_value = basket.total_value;
    assert!((actual_value - expected_value).abs() < dec!(100.0));
}

// =============================================================================
// CREDIT QUALITY TESTS
// =============================================================================

#[test]
fn test_credit_quality_metrics() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    let credit =
        convex_portfolio::analytics::calculate_credit_quality(&portfolio.holdings, &config);

    // Should have investment grade weight
    assert!(credit.ig_weight > 0.0);

    // Average rating score should be investment grade
    assert!(credit.average_rating_score.is_some());
    let avg_score = credit.average_rating_score.unwrap();
    assert!(avg_score < 11.0, "Should be IG on average (score < 11)");
}

#[test]
fn test_migration_risk() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    let migration =
        convex_portfolio::analytics::calculate_migration_risk(&portfolio.holdings, &config);

    // Should identify fallen angel risk (IG holdings close to BBB-)
    // DELL is BBB-, so at risk
    assert!(
        migration.fallen_angel_risk.holdings_count > 0
            || migration.fallen_angel_risk.bbb_minus_weight > 0.0
    );
}

// =============================================================================
// INVARIANT TESTS
// =============================================================================

#[test]
fn test_weight_invariants() {
    let portfolio = create_corporate_portfolio();
    let config = AnalyticsConfig::default();

    // Sector weights should sum to 100%
    let sector_dist = bucket_by_sector(&portfolio.holdings, &config);
    let sector_sum: f64 = sector_dist.by_sector.values().map(|m| m.weight_pct).sum();
    assert!(
        (sector_sum - 100.0).abs() < 0.01,
        "Sector weights: {}",
        sector_sum
    );

    // Rating weights should sum to 100%
    let rating_dist = bucket_by_rating(&portfolio.holdings, &config);
    let rating_sum: f64 = rating_dist.by_bucket.values().map(|m| m.weight_pct).sum();
    assert!(
        (rating_sum - 100.0).abs() < 0.01,
        "Rating weights: {}",
        rating_sum
    );

    // Duration contributions should sum to 100%
    let dur_contrib =
        convex_portfolio::contribution::duration_contributions(&portfolio.holdings, &config);
    let dur_sum: f64 = dur_contrib
        .by_holding
        .iter()
        .map(|c| c.contribution_pct)
        .sum();
    assert!(
        (dur_sum - 100.0).abs() < 0.1,
        "Duration contributions: {}",
        dur_sum
    );
}

#[test]
fn test_nav_equals_sum_of_parts() {
    let portfolio = create_corporate_portfolio();

    let nav = portfolio.nav();
    let securities = portfolio.securities_market_value();
    let accrued = portfolio.total_accrued_interest();
    let cash = portfolio.total_cash();
    let liabilities = portfolio.total_liabilities();

    let computed_nav = securities + accrued + cash - liabilities;
    assert!((nav - computed_nav).abs() < dec!(0.01), "NAV mismatch");
}

#[test]
fn test_dv01_is_sum_of_holdings() {
    let portfolio = create_corporate_portfolio();

    // Sum holding DV01s
    let sum_dv01: Decimal = portfolio
        .holdings
        .iter()
        .filter_map(|h| h.total_dv01())
        .sum();

    // Portfolio DV01
    let config = AnalyticsConfig::default();
    let risk = calculate_risk_metrics(&portfolio.holdings, None, &config);

    // Should be approximately equal
    let sum_dv01_f64 = sum_dv01.to_f64().unwrap_or(0.0);
    assert!(
        (risk.total_dv01 - sum_dv01_f64).abs() < 1.0,
        "DV01 mismatch: {} vs {}",
        risk.total_dv01,
        sum_dv01_f64
    );
}

// =============================================================================
// EDGE CASES
// =============================================================================

#[test]
fn test_empty_portfolio() {
    let portfolio = PortfolioBuilder::new()
        .name("Empty")
        .base_currency(Currency::USD)
        .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
        .build()
        .unwrap();

    assert!(portfolio.is_empty());
    assert_eq!(portfolio.nav(), Decimal::ZERO);

    let config = AnalyticsConfig::default();
    let analytics = calculate_portfolio_analytics(&portfolio, &config);
    assert!(analytics.yields.ytm.is_none());
}

#[test]
fn test_single_holding_portfolio() {
    let holding = create_holding(
        "ONLY",
        dec!(1_000_000),
        dec!(100.0),
        0.05,
        5.0,
        0.30,
        100.0,
        Some(Sector::Corporate),
        Some(CreditRating::A),
    );

    let portfolio = PortfolioBuilder::new()
        .name("Single")
        .base_currency(Currency::USD)
        .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
        .add_holding(holding)
        .build()
        .unwrap();

    let config = AnalyticsConfig::default();

    // Analytics should match the single holding
    let yield_metrics = calculate_yield_metrics(&portfolio.holdings, &config);
    assert!((yield_metrics.ytm.unwrap() - 0.05).abs() < 0.001);

    let risk_metrics = calculate_risk_metrics(&portfolio.holdings, None, &config);
    assert!((risk_metrics.best_duration.unwrap() - 5.0).abs() < 0.01);

    // Sector should be 100% corporate
    let sector_dist = bucket_by_sector(&portfolio.holdings, &config);
    assert!((sector_dist.get(Sector::Corporate).unwrap().weight_pct - 100.0).abs() < 0.01);
}

#[test]
fn test_cash_only_portfolio() {
    let portfolio = PortfolioBuilder::new()
        .name("CashOnly")
        .base_currency(Currency::USD)
        .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
        .add_cash(CashPosition::new(dec!(1_000_000), Currency::USD))
        .build()
        .unwrap();

    assert!(portfolio.is_empty()); // No bond holdings
    assert_eq!(portfolio.nav(), dec!(1_000_000));
    assert_eq!(portfolio.total_cash(), dec!(1_000_000));
}
