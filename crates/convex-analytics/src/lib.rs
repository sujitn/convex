//! # Convex Analytics
//!
//! Unified analytics engine for fixed income securities.
//!
//! This crate consolidates all calculation logic from the Convex library:
//! - **Yields**: Yield-to-maturity, current yield, simple yield, money market yields
//! - **Pricing**: Bond pricing from curves, present value calculations
//! - **Spreads**: Z-spread, G-spread, I-spread, OAS, ASW, discount margin
//! - **Risk**: Duration, convexity, DV01, VaR, hedging
//! - **Options**: Binomial trees, Hull-White model for callable/putable bonds
//! - **YAS**: Bloomberg YAS replication
//! - **Cash Flows**: Cash flow generation, accrued interest, schedules
//!
//! ## Architecture
//!
//! `convex-analytics` depends on `convex-bonds` for instrument definitions,
//! but `convex-bonds` does NOT depend on this crate. This separation ensures
//! that bond types remain lightweight and calculation-free.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use convex_bonds::prelude::*;
//! use convex_analytics::prelude::*;
//!
//! // Create a bond
//! let bond = FixedRateBondBuilder::new()
//!     .coupon_rate(dec!(0.05))
//!     .maturity(date!(2030-01-15))
//!     .build()?;
//!
//! // Calculate analytics using standalone functions
//! let ytm = yield_to_maturity(&bond, settlement, clean_price, Frequency::SemiAnnual)?;
//! let duration = modified_duration(&bond, settlement, ytm.yield_value, Frequency::SemiAnnual)?;
//!
//! // Or use calculators
//! let yas = YASCalculator::new(&govt_curve, &swap_curve, &spot_curve);
//! let result = yas.analyze(&bond, settlement, clean_price)?;
//! ```

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

// Module declarations - will be populated as we migrate
pub mod error;

// Re-export the error type
pub use error::{AnalyticsError, AnalyticsResult};

// ============================================================================
// MODULES
// ============================================================================

// Phase 1: Cash flows (from convex-bonds/cashflows/)
pub mod cashflows;

// Phase 1c: Yields (from convex-bonds/pricing/ + convex-yas/yields/)
pub mod yields;

// Phase 1c: Pricing (from convex-bonds/pricing/)
pub mod pricing;

// Phase 2: Risk (from convex-risk/)
pub mod risk;

// Phase 3: Spreads (from convex-spreads/)
pub mod spreads;

// Phase 4: YAS (from convex-yas/)
pub mod yas;

// Phase 5: Options (from convex-bonds/options/)
pub mod options;

// Phase 6: Standalone functions (converted from BondAnalytics trait)
pub mod functions;

/// Prelude module for convenient imports.
///
/// ```rust,ignore
/// use convex_analytics::prelude::*;
/// ```
pub mod prelude {
    pub use crate::error::{AnalyticsError, AnalyticsResult};

    // Cash flows
    pub use crate::cashflows::{
        AccruedInterestCalculator, CashFlowGenerator, IrregularPeriodHandler, Schedule,
        ScheduleConfig, SettlementCalculator, SettlementRules, SettlementStatus, StubType,
    };

    // Yields
    pub use crate::yields::{
        current_yield, simple_yield, street_convention_yield, true_yield, RollForwardMethod,
        ShortDateCalculator, StandardYieldEngine, YieldEngine, YieldEngineResult, YieldResult,
        YieldSolver,
    };

    // Pricing
    pub use crate::pricing::{BondPricer, PriceResult};

    // Risk
    pub use crate::risk::{
        aggregate_portfolio_risk,
        analytical_convexity,
        duration_hedge_ratio,
        dv01_from_duration,
        dv01_from_prices,
        // Hedging
        dv01_hedge_ratio,
        dv01_per_100_face,
        historical_var,
        key_rate_duration_at_tenor,
        modified_from_macaulay,
        notional_from_dv01,
        parametric_var,
        parametric_var_from_dv01,
        price_change_from_duration,
        price_change_with_convexity,
        spread_duration,
        // Calculator
        BondRiskCalculator,
        BondRiskMetrics,
        // Convexity types (low-level functions available via crate::risk::)
        Convexity,
        // Duration types (low-level functions available via crate::risk::)
        Duration,
        EffectiveDurationCalculator,
        HedgeDirection,
        HedgeRecommendation,
        KeyRateDuration,
        KeyRateDurationCalculator,
        KeyRateDurations,
        PortfolioRisk,
        Position,
        VaRMethod,
        // VaR
        VaRResult,
        DEFAULT_BUMP_SIZE,
        // DV01
        DV01,
        SMALL_BUMP_SIZE,
        STANDARD_KEY_RATE_TENORS,
    };

    // Spreads
    pub use crate::spreads::{
        // G-spread
        g_spread,
        g_spread_with_benchmark,
        // I-spread
        i_spread,
        // Discount Margin
        simple_margin,
        z_discount_margin,
        // Z-spread
        z_spread,
        z_spread_from_curve,
        // Types
        ASWType,
        BenchmarkSpec,
        DiscountMarginCalculator,
        GSpreadCalculator,
        GovernmentBenchmark,
        GovernmentCurve,
        ISpreadCalculator,
        // OAS
        OASCalculator,
        // ASW
        ParParAssetSwap,
        ProceedsAssetSwap,
        SecurityId,
        Sovereign,
        SupranationalIssuer,
        ZSpreadCalculator,
    };

    // YAS (Bloomberg YAS replication)
    pub use crate::yas::{
        calculate_accrued_amount, calculate_proceeds, calculate_settlement_date,
        BatchYASCalculator, BloombergReference, SettlementInvoice, SettlementInvoiceBuilder,
        ValidationFailure, YASCalculator, YASResult, YasAnalysis, YasAnalysisBuilder,
    };

    // Options (callable/puttable bonds)
    pub use crate::options::{BinomialTree, HullWhite, ModelError, ShortRateModel};

    // Standalone bond analytics functions (replacing BondAnalytics trait)
    pub use crate::functions::{
        clean_price_from_yield,
        // Convexity calculations
        convexity,
        // Price calculations
        dirty_price_from_yield,
        // DV01 calculations
        dv01,
        dv01_notional,
        effective_convexity,
        effective_duration,
        // Price change estimation
        estimate_price_change,
        // Duration calculations
        macaulay_duration,
        modified_duration,
        // Helper
        parse_day_count,
        // Yield calculations
        yield_to_maturity,
        yield_to_maturity_with_convention,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crate_compiles() {
        // Basic smoke test
        let err = AnalyticsError::InvalidInput("test".to_string());
        assert!(err.to_string().contains("test"));
    }
}
