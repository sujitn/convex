//! Hedging analytics. Two surfaces:
//!
//! - **Hedge advisor** (`types`, `instruments`, `strategies`, `cost`,
//!   `compare`, `narrate`): structured proposal pipeline.
//! - **Ratio helpers** (`hedge_ratio`, `portfolio`): scalar DV01/duration
//!   ratios for callers that want raw numbers.

pub mod compare;
pub mod cost;
pub mod ctd;
mod hedge_ratio;
pub mod instruments;
pub mod narrate;
mod portfolio;
pub mod strategies;
pub mod types;

pub use compare::compare_hedges;
pub use cost::{cost_bps, COST_MODEL_NAME};
pub use ctd::{
    approximate_cme_cf, deliverable_to_bond, fair_futures_price, select_ctd_by_net_basis,
    CtdSelection, Deliverable,
};
pub use hedge_ratio::{duration_hedge_ratio, dv01_hedge_ratio};
pub use instruments::{
    bond_future_risk, cash_bond_risk, interest_rate_swap_risk, BondFutureRisk, LegRisk,
};
pub use narrate::narrate;
pub use portfolio::*;
pub use strategies::{
    barbell_futures, cash_bond_pair, duration_futures, interest_rate_swap, key_rate_futures,
};
pub use types::{
    residual_from, BondFuture, CashBondLeg, ComparisonReport, ComparisonRow, Constraints,
    HedgeInstrument, HedgeProposal, HedgeTrade, InterestRateSwap, Recommendation,
    RecommendationReason, ResidualRisk, SwapSide, TradeoffNotes,
};
