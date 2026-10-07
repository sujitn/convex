//! Facade re-exporting the public API of `convex-core`, `convex-curves`,
//! `convex-bonds`, `convex-rates`, `convex-credit`, and `convex-analytics`.
//!
//! Depend on `convex` and import the flat names; the internal split is kept
//! for compile-time parallelism but invisible to callers.

#![warn(missing_docs)]
#![forbid(unsafe_code)]
#![deny(unused_must_use)]

/// Pure fixed/floating contractual cash valuation with explicit curve roles.
pub use convex_analytics::bond_contract;
pub use convex_analytics::bond_quote;
/// Pure native-quote single-name CDS calibration and immutable curve sets.
pub use convex_analytics::credit_calibration;
/// Pure credit scenario/risk inputs and checked bond-CDS hedge mappings.
pub use convex_analytics::credit_risk;
/// Full dated product helper inputs without application dependencies.
pub use convex_analytics::curve_inputs;
/// Pure contract-aware futures and actual cash hedge valuation/risk.
pub use convex_analytics::futures;
pub use convex_analytics::futures_reference;
/// Pure supplied IRS/OIS native-quote calibration and dependency graph building.
pub use convex_analytics::swap_calibration;
pub use convex_core::calendars::BusinessDayConvention;
pub use convex_core::contracts;
pub use convex_core::daycounts::DayCountConvention;
pub use convex_core::error::{ConvexError, ConvexResult};
pub use convex_core::types::{
    CashFlow, CashFlowSchedule, CashFlowType, Compounding, Currency, Date, Frequency, Mark, Price,
    PriceKind, Spread, SpreadType, Yield,
};
pub use convex_core::{cashflows, fixings};
/// Pure supplied single-name CDS terms, standard valuation and conversions.
pub use convex_credit as credit;
/// Pure checked curve construction/query contracts.
pub use convex_curves::contracts as curve_contracts;
/// Complete supplied IRS/OIS terms, schedules and publication-aware cashflows.
pub use convex_rates as rates;
/// Supplied multi-curve swap valuation with actual historical coupon provenance.
pub use convex_rates::pricing as swap_pricing;

pub use convex_curves::{
    CalibrationResult, CurveError, CurveResult, Deposit, DiscountCurve, DiscreteCurve,
    ExtrapolationMethod, Fra, GlobalFitter, InstrumentSet, InstrumentType, InterpolationMethod,
    Ois, RateCurve, RateCurveDyn, Swap, ValueType, ZeroCurve,
};

/// Pure optional-bond assumptions, calibration, OAS and factor risk.
pub use convex_analytics::optional_bond;
/// Complete supplied bond terms, profiles, resets, cash and exercise contracts.
pub use convex_bonds::contracts as bond_contracts;
pub use convex_bonds::instruments::{
    CallableBond, FixedRateBond, FloatingRateNote, ZeroCouponBond,
};
pub use convex_bonds::traits::{Bond, BondCashFlow, FixedCouponBond};
pub use convex_bonds::types::{CalendarId, CallEntry, CallSchedule, CallType};
pub use convex_bonds::{BondError, BondResult};

pub use convex_analytics::error::{AnalyticsError, AnalyticsResult};
pub use convex_analytics::functions::{
    clean_price_from_yield, convexity, dirty_price_from_yield, dv01, macaulay_duration,
    modified_duration, yield_to_maturity,
};
pub use convex_analytics::pricing::{price_callable_from_mark, price_from_mark, PricingResult};
pub use convex_analytics::risk::{
    aggregate_risk_profiles, barbell_futures, cash_bond_pair, compare_hedges,
    compute_callable_position_risk, compute_position_risk, duration_futures, hedge_cost_bps,
    interest_rate_swap, key_rate_futures, narrate, position_contributions, select_ctd, BondFuture,
    CashBondLeg, ComparisonReport, ComparisonRow, Constraints, CostFeed, CtdSelection, Deliverable,
    HedgeExecutionStatus, HedgeInstrument, HedgeProposal, HedgeTrade, InterestRateSwap,
    KeyRateBucket, KeyRateBucketLimit, PositionContribution, Provenance, Recommendation,
    RecommendationReason, ResidualRisk, RiskProfile, SwapSide, TradeoffNotes,
    ADVISOR_KEY_RATE_TENORS,
};
pub use convex_analytics::spreads::{
    GSpreadCalculator, ISpreadCalculator, OASCalculator, ZSpreadCalculator,
};
