//! Hedging calculations for fixed income portfolios.

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
    bond_future_risk, interest_rate_swap_risk, BondFutureRisk, InterestRateSwapRisk,
};
pub use narrate::narrate;
pub use portfolio::*;
pub use strategies::{barbell_futures, duration_futures, interest_rate_swap};
pub use types::{
    residual_from, BondFuture, ComparisonReport, ComparisonRow, Constraints, HedgeInstrument,
    HedgeProposal, HedgeTrade, InterestRateSwap, Recommendation, RecommendationReason,
    ResidualRisk, SwapSide, TradeoffNotes,
};
