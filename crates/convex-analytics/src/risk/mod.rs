//! Risk analytics: duration, convexity, DV01, VaR, KRD profiles, and the
//! hedge advisor surface.

pub mod calculator;
pub mod convexity;
pub mod duration;
pub mod dv01;
pub mod hedging;
pub mod pnl;
pub mod profile;
pub mod var;

pub use calculator::{
    BondRiskCalculator, BondRiskMetrics, EffectiveDurationCalculator, KeyRateDurationCalculator,
};
pub use convexity::{
    analytical_convexity, effective_convexity, price_change_with_convexity, Convexity,
};
pub use duration::{
    effective_duration, key_rate_duration_at_tenor, macaulay_duration, modified_duration,
    modified_from_macaulay, price_change_from_duration, spread_duration, Duration, KeyRateDuration,
    KeyRateDurations, DEFAULT_BUMP_SIZE, SMALL_BUMP_SIZE, STANDARD_KEY_RATE_TENORS,
};
pub use dv01::{dv01_from_duration, dv01_from_prices, dv01_per_100_face, notional_from_dv01, DV01};
pub use hedging::{
    aggregate_portfolio_risk, barbell_futures, bond_future_risk, cash_bond_pair, cash_bond_risk,
    compare_hedges, duration_futures, duration_hedge_ratio, dv01_hedge_ratio, hedge_cost_bps,
    interest_rate_swap, interest_rate_swap_risk, key_rate_futures, narrate, position_contributions,
    residual_from, select_ctd, BondFuture, BondFutureRisk, CashBondLeg, ComparisonReport,
    ComparisonRow, Constraints, CostFeed, CtdSelection, Deliverable, HedgeInstrument,
    HedgeProposal, HedgeTrade, HeuristicCostFeed, InterestRateSwap, KeyRateBucketLimit, LegRisk,
    PortfolioRisk, Position, PositionContribution, Recommendation, RecommendationReason,
    ResidualRisk, SwapSide, TradeoffNotes, COST_MODEL_NAME,
};
pub use pnl::{
    Attribution, AttributionConfig, AttributionProvenance, CurveBreakdown, FactorPnl, PnlFactor,
    PositionAttribution, SwapPnlSpec, DEFAULT_PIVOT_TENOR_YEARS, FACTOR_MODEL_NAME,
};
pub use profile::{
    aggregate_risk_profiles, compute_callable_position_risk, compute_position_risk, KeyRateBucket,
    Provenance, RiskProfile, ADVISOR_KEY_RATE_TENORS,
};
pub use var::{historical_var, parametric_var, parametric_var_from_dv01, VaRMethod, VaRResult};

/// Glob-importable re-exports.
pub mod prelude {
    pub use super::calculator::*;
    pub use super::convexity::*;
    pub use super::duration::*;
    pub use super::dv01::*;
    pub use super::hedging::*;
    pub use super::pnl::*;
    pub use super::var::*;
}
