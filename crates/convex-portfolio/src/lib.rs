//! # Convex Portfolio
//!
//! Portfolio and ETF analytics for fixed income securities.
//!
//! This crate provides comprehensive portfolio-level analytics by aggregating
//! bond-level calculations from the Convex library.
//!
//! ## Design Philosophy
//!
//! - **Pure functions**: All calculations are stateless with explicit inputs
//! - **Pre-calculated analytics**: Caller provides bond-level metrics via `HoldingAnalytics`
//! - **Flexible classification**: Normalized enums for analytics + provider maps for source data
//! - **Config-driven parallelism**: Optional rayon support with threshold-based switching
//!
//! ## Features
//!
//! - **NAV Calculation**: Total NAV, iNAV, premium/discount
//! - **Weighted Metrics**: YTM, YTW, duration, convexity, spreads
//! - **Risk Analytics**: DV01, key rate duration profiles, stress testing
//! - **Classification**: Sector, rating, seniority bucketing
//! - **Contribution Analysis**: Risk contribution by holding/sector
//! - **Benchmark Comparison**: Active weights, tracking error
//! - **ETF Analytics**: SEC yields, creation/redemption basket analysis
//! - **Liquidity Analytics**: Bid-ask spreads, liquidity scores, days to liquidate
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use convex_portfolio::prelude::*;
//!
//! // Build a portfolio
//! let portfolio = PortfolioBuilder::new()
//!     .name("MyPortfolio")
//!     .base_currency(Currency::USD)
//!     .as_of_date(Date::from_ymd(2025, 1, 15)?)
//!     .add_holding(holding1)
//!     .add_holding(holding2)
//!     .add_cash(CashPosition::new(dec!(1_000_000), Currency::USD))
//!     .build()?;
//!
//! // Calculate analytics
//! let config = AnalyticsConfig::default();
//! let nav = portfolio.nav();
//! let analytics = calculate_portfolio_analytics(&portfolio, &config);
//! let by_sector = bucket_by_sector(&portfolio.holdings, &config);
//! ```
//!
//! ## Module Overview
//!
//! - [`analytics`] - Core analytics (NAV, yields, risk, spreads, key rates, liquidity)
//! - [`benchmark`] - Benchmark comparison and tracking error
//! - [`bucketing`] - Classification by sector, rating, maturity
//! - [`contribution`] - Risk contribution and return attribution
//! - [`etf`] - ETF-specific analytics (NAV, baskets, SEC yields)
//! - [`portfolio`] - Portfolio and builder types
//! - [`stress`] - Stress testing scenarios and impact
//! - [`types`] - Core types (Holding, Classification, Config)
//!
//! ## Feature Flags
//!
//! - `parallel`: Enable rayon-based parallel processing for large portfolios

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]
#![allow(clippy::module_name_repetitions)]

// Module declarations
pub mod analytics;
pub mod benchmark;
pub mod bucketing;
pub mod contribution;
pub mod error;
pub mod etf;
pub mod portfolio;
pub mod stress;
pub mod types;

// Re-export error types at crate root
pub use error::{PortfolioError, PortfolioResult};

// Re-export main types
pub use types::{
    // Config
    AnalyticsConfig,
    // Holding
    CashPosition,
    // Classification
    Classification,
    CreditRating,
    Holding,
    HoldingAnalytics,
    HoldingBuilder,
    // Maturity
    MaturityBucket,
    RatingBucket,
    RatingInfo,
    Sector,
    SectorInfo,
    Seniority,
    SeniorityInfo,
    WeightingMethod,
};

// Re-export portfolio types
pub use portfolio::{Portfolio, PortfolioBuilder};

// Re-export analytics types and functions
pub use analytics::{
    // Key Rates
    aggregate_key_rate_profile,
    // Credit Quality
    calculate_credit_quality,
    // Liquidity
    calculate_liquidity_metrics,
    calculate_migration_risk,
    // NAV
    calculate_nav_breakdown,
    // Summary
    calculate_portfolio_analytics,
    // Risk
    calculate_risk_metrics,
    // Spreads
    calculate_spread_metrics,
    // Yields
    calculate_yield_metrics,
    cs01_per_share,
    dv01_per_share,
    estimate_days_to_liquidate,
    liquidity_distribution,
    // Parallel utilities
    maybe_parallel_filter_map,
    maybe_parallel_fold,
    maybe_parallel_map,
    partial_dv01s,
    total_cs01,
    total_dv01,
    weighted_asw,
    weighted_best_duration,
    weighted_best_spread,
    weighted_best_yield,
    weighted_bid_ask_spread,
    weighted_convexity,
    weighted_current_yield,
    weighted_effective_convexity,
    weighted_effective_duration,
    weighted_g_spread,
    weighted_i_spread,
    weighted_liquidity_score,
    weighted_macaulay_duration,
    weighted_modified_duration,
    weighted_oas,
    weighted_spread_duration,
    weighted_ytc,
    weighted_ytm,
    weighted_ytw,
    weighted_z_spread,
    CreditQualityMetrics,
    DaysToLiquidate,
    FallenAngelRisk,
    KeyRateProfile,
    LiquidityBucket,
    LiquidityDistribution,
    LiquidityMetrics,
    MigrationRisk,
    NavBreakdown,
    PortfolioAnalytics,
    QualityTiers,
    RisingStarRisk,
    RiskMetrics,
    SpreadMetrics,
    YieldMetrics,
};

// Re-export bucketing types and functions
pub use bucketing::{
    // Custom bucketing
    bucket_by_classifier,
    bucket_by_country,
    bucket_by_currency,
    bucket_by_custom_field,
    bucket_by_issuer,
    // Maturity bucketing
    bucket_by_maturity,
    // Rating bucketing
    bucket_by_rating,
    bucket_by_region,
    // Sector bucketing
    bucket_by_sector,
    BucketMetrics,
    CustomDistribution,
    MaturityDistribution,
    RatingDistribution,
    SectorDistribution,
};

// Re-export stress testing types and functions
pub use stress::{
    // Impact calculations
    best_case,
    key_rate_shift_impact,
    parallel_shift_impact,
    run_stress_scenario,
    run_stress_scenarios,
    spread_shock_impact,
    // Standard scenarios
    standard as stress_scenarios,
    summarize_results,
    worst_case,
    // Scenarios
    RateScenario,
    SpreadScenario,
    StressResult,
    StressScenario,
    StressSummary,
    TenorShift,
};

// Re-export contribution analysis types and functions
pub use contribution::{
    // Return attribution
    calculate_attribution,
    // Risk contribution
    cs01_contributions,
    duration_contributions,
    dv01_contributions,
    estimate_income_returns,
    estimate_rate_returns,
    estimate_spread_returns,
    spread_contributions,
    AggregatedAttribution,
    AttributionInput,
    BucketContribution,
    Cs01Contributions,
    DurationContributions,
    Dv01Contributions,
    HoldingAttribution,
    HoldingContribution,
    PortfolioAttribution,
    SectorAttribution,
    SpreadContributions,
};

// Re-export benchmark comparison types and functions
pub use benchmark::{
    // Tracking
    active_weights,
    // Comparison
    benchmark_comparison,
    duration_difference_by_sector,
    estimate_tracking_error,
    spread_difference_by_sector,
    ActiveWeight,
    ActiveWeights,
    BenchmarkComparison,
    DurationComparison,
    RatingComparison,
    RiskComparison,
    SectorComparison,
    SpreadComparison,
    TrackingErrorEstimate,
    YieldComparison,
};

// Re-export ETF analytics types and functions
pub use etf::{
    // Basket
    analyze_basket,
    arbitrage_opportunity,
    build_creation_basket,
    // SEC
    calculate_distribution_yield,
    // NAV
    calculate_etf_nav,
    calculate_inav,
    calculate_premium_discount_stats,
    calculate_sec_yield,
    estimate_yield_from_holdings,
    premium_discount,
    run_compliance_checks,
    BasketAnalysis,
    BasketComponent,
    BasketFlowSummary,
    ComplianceCheck,
    ComplianceSeverity,
    CreationBasket,
    DistributionYield,
    EtfNavMetrics,
    ExpenseMetrics,
    PremiumDiscountPoint,
    PremiumDiscountStats,
    SecYield,
    SecYieldInput,
};

/// Prelude module for convenient imports.
///
/// ```rust,ignore
/// use convex_portfolio::prelude::*;
/// ```
pub mod prelude {
    // Error types
    pub use crate::error::{PortfolioError, PortfolioResult};

    // Classification types
    pub use crate::types::{
        Classification, CreditRating, RatingBucket, RatingInfo, Sector, SectorInfo, Seniority,
        SeniorityInfo,
    };

    // Holding types
    pub use crate::types::{CashPosition, Holding, HoldingAnalytics, HoldingBuilder};

    // Config types
    pub use crate::types::{AnalyticsConfig, WeightingMethod};

    // Maturity
    pub use crate::types::MaturityBucket;

    // Portfolio
    pub use crate::portfolio::{Portfolio, PortfolioBuilder};

    // Analytics
    pub use crate::analytics::{
        aggregate_key_rate_profile, calculate_credit_quality, calculate_nav_breakdown,
        calculate_portfolio_analytics, calculate_risk_metrics, calculate_spread_metrics,
        calculate_yield_metrics, CreditQualityMetrics, KeyRateProfile, NavBreakdown,
        PortfolioAnalytics, RiskMetrics, SpreadMetrics, YieldMetrics,
    };

    // Bucketing
    pub use crate::bucketing::{
        bucket_by_country, bucket_by_maturity, bucket_by_rating, bucket_by_sector, BucketMetrics,
        CustomDistribution, MaturityDistribution, RatingDistribution, SectorDistribution,
    };

    // Stress testing
    pub use crate::stress::{
        parallel_shift_impact, run_stress_scenario, run_stress_scenarios, spread_shock_impact,
        RateScenario, SpreadScenario, StressResult, StressScenario, StressSummary,
    };

    // Contribution analysis
    pub use crate::contribution::{
        duration_contributions, dv01_contributions, spread_contributions, DurationContributions,
        Dv01Contributions, HoldingContribution, SpreadContributions,
    };

    // Benchmark comparison
    pub use crate::benchmark::{
        active_weights, benchmark_comparison, ActiveWeights, BenchmarkComparison,
    };

    // ETF analytics
    pub use crate::etf::{
        calculate_etf_nav, calculate_sec_yield, premium_discount, EtfNavMetrics, SecYield,
    };

    // Liquidity
    pub use crate::analytics::{calculate_liquidity_metrics, LiquidityBucket, LiquidityMetrics};

    // Re-export commonly used types from dependencies
    pub use convex_core::types::{Currency, Date, Frequency};
    pub use rust_decimal::Decimal;
    pub use rust_decimal_macros::dec;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crate_compiles() {
        // Basic smoke test
        let err = PortfolioError::EmptyPortfolio;
        assert!(err.to_string().contains("no holdings"));
    }
}
