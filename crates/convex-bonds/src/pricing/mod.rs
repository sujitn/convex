//! Bond pricing calculations.
//!
//! - [`YieldSolver`]: Bloomberg YAS-style yield-to-maturity solver
//! - [`YieldEngine`] / [`StandardYieldEngine`]: unified yield calculation trait
//! - [`current_yield`]: current yield calculation

pub mod short_date;
mod yield_engine;
pub(crate) mod yield_solver;

pub use short_date::{RollForwardMethod, ShortDateCalculator};
pub use yield_engine::{
    bond_equivalent_yield, current_yield_simple, discount_yield, simple_yield, StandardYieldEngine,
    YieldEngine, YieldEngineResult,
};
pub use yield_solver::{current_yield, current_yield_from_bond, YieldResult, YieldSolver};
