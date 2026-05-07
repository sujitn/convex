//! Facade re-exporting the public API of `convex-core`, `convex-curves`,
//! `convex-bonds`, and `convex-analytics`.
//!
//! Depend on `convex` and import the flat names; the internal split is kept
//! for compile-time parallelism but invisible to callers.

#![warn(missing_docs)]

pub use convex_core::daycounts::DayCountConvention;
pub use convex_core::error::{ConvexError, ConvexResult};
pub use convex_core::types::{
    CashFlow, CashFlowSchedule, CashFlowType, Compounding, Currency, Date, Frequency, Mark, Price,
    PriceKind, Spread, SpreadType, Yield,
};

pub use convex_curves::{
    CalibrationResult, CurveError, CurveResult, Deposit, DiscountCurve, DiscreteCurve, Fra,
    GlobalFitter, InstrumentSet, InstrumentType, InterpolationMethod, Ois, RateCurve, RateCurveDyn,
    Swap, ValueType, ZeroCurve,
};

pub use convex_bonds::instruments::{
    CallableBond, FixedRateBond, FloatingRateNote, ZeroCouponBond,
};
pub use convex_bonds::traits::{Bond, BondCashFlow, FixedCouponBond};
pub use convex_bonds::types::CallSchedule;
pub use convex_bonds::{BondError, BondResult};

pub use convex_analytics::error::{AnalyticsError, AnalyticsResult};
pub use convex_analytics::functions::{
    clean_price_from_yield, convexity, dirty_price_from_yield, dv01, macaulay_duration,
    modified_duration, yield_to_maturity,
};
pub use convex_analytics::pricing::{price_from_mark, PricingResult};
pub use convex_analytics::risk::{
    barbell_futures, cash_bond_pair, compare_hedges, compute_position_risk, duration_futures,
    interest_rate_swap, narrate, BondFuture, CashBondLeg, ComparisonReport, ComparisonRow,
    Constraints, HedgeInstrument, HedgeProposal, HedgeTrade, InterestRateSwap, KeyRateBucket,
    Provenance, Recommendation, RecommendationReason, ResidualRisk, RiskProfile, SwapSide,
    TradeoffNotes, ADVISOR_KEY_RATE_TENORS,
};
pub use convex_analytics::spreads::{
    GSpreadCalculator, ISpreadCalculator, OASCalculator, ZSpreadCalculator,
};
