//! Bond pricing calculations.
//!
//! - [`YieldSolver`]: supplied-cash yield diagnostics
//! - [`YieldEngine`] / [`StandardYieldEngine`]: unified yield calculation trait
//! - [`current_yield`]: current yield calculation
//!
//! Date-free floating-point bill conversion is removed. Supply original dates
//! and cash to `convex::treasury_bill::quote_treasury_bill`.
//!
//! ```compile_fail
//! use convex_bonds::pricing::bond_equivalent_yield;
//! ```
//!
//! ```compile_fail
//! let _ = convex_bonds::pricing::discount_yield(99.0, 100.0, 90);
//! ```

/// Explicit checked bank-discount, money-market and fixed-365 bill rates.
pub mod bill;
/// Checked absolute dated-cash PV and same-basis analytical yield risk.
pub mod dated_cash;
pub mod short_date;
mod yield_engine;
/// Checked scalar reporting and explicitly supplied current/simple yield measures.
pub mod yield_scalars;
pub(crate) mod yield_solver;
/// Checked absolute zero-coupon cash and decimal-rate conversions.
pub mod zero;

pub use short_date::{RollForwardMethod, ShortDateCalculator};
pub use yield_engine::{
    current_yield_simple, simple_yield, StandardYieldEngine, YieldEngine, YieldEngineResult,
};
pub use yield_solver::{current_yield, current_yield_from_bond, YieldResult, YieldSolver};
