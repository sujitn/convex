//! Hedging calculations for fixed income portfolios.
//!
//! Two distinct surfaces share this module:
//!
//! - **Numeric helpers** (`hedge_ratio`, `portfolio`): low-level building
//!   blocks. [`dv01_hedge_ratio`] / [`duration_hedge_ratio`] return a single
//!   ratio; [`HedgeRecommendation`] is a tiny three-field DTO they emit.
//!   These exist independently of the advisor and are used by callers that
//!   want raw numbers, not proposals.
//!
//! - **Hedge advisor** (`types`, `instruments`, `strategies`, `cost`,
//!   `compare`, `narrate`): the structured proposal pipeline. Strategies
//!   produce [`HedgeProposal`]s; [`compare_hedges`] aggregates them into a
//!   [`ComparisonReport`] with a deterministic [`Recommendation`].
//!
//! Don't confuse [`HedgeRecommendation`] (legacy numeric DTO from the
//! ratio helpers) with [`Recommendation`] (advisor's pick, lives on a
//! `ComparisonReport`).

pub mod compare;
pub mod cost;
mod hedge_ratio;
pub mod instruments;
pub mod narrate;
mod portfolio;
pub mod strategies;
pub mod types;

pub use compare::compare_hedges;
pub use cost::{CostModel, HeuristicCostModel};
pub use hedge_ratio::*;
pub use instruments::{
    bond_future_risk, cash_bond_risk, interest_rate_swap_risk, BondFutureRisk, CashBondRisk,
    InterestRateSwapRisk,
};
pub use narrate::narrate;
pub use portfolio::*;
pub use strategies::{barbell_futures, cash_bond_pair, duration_futures, interest_rate_swap};
pub use types::{
    residual_from, BondFuture, CashBondLeg, ComparisonReport, ComparisonRow, Constraints,
    HedgeInstrument, HedgeProposal, HedgeTrade, InterestRateSwap, Recommendation,
    RecommendationReason, ResidualRisk, SwapSide, TradeoffNotes,
};
