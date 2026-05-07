//! Risk analytics: duration, convexity, DV01, VaR, KRD profiles, and the
//! hedge advisor surface.

pub mod calculator;
pub mod convexity;
pub mod duration;
pub mod dv01;
pub mod hedging;
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
    compare_hedges, cost_bps as hedge_cost_bps, duration_futures, duration_hedge_ratio,
    dv01_hedge_ratio, interest_rate_swap, interest_rate_swap_risk, narrate, residual_from,
    BondFuture, BondFutureRisk, CashBondLeg, ComparisonReport, ComparisonRow, Constraints,
    HedgeInstrument, HedgeProposal, HedgeTrade, InterestRateSwap, LegRisk, PortfolioRisk, Position,
    Recommendation, RecommendationReason, ResidualRisk, SwapSide, TradeoffNotes, COST_MODEL_NAME,
};
pub use profile::{
    compute_position_risk, KeyRateBucket, Provenance, RiskProfile, ADVISOR_KEY_RATE_TENORS,
};
pub use var::{historical_var, parametric_var, parametric_var_from_dv01, VaRMethod, VaRResult};

/// Glob-importable re-exports.
pub mod prelude {
    pub use super::calculator::*;
    pub use super::convexity::*;
    pub use super::duration::*;
    pub use super::dv01::*;
    pub use super::hedging::*;
    pub use super::var::*;
}
