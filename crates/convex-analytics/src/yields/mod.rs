//! Yield calculation methods.
//!
//! This module provides comprehensive yield calculations for fixed income securities,
//! consolidating all yield-related logic from the Convex library.
//!
//! The approximate date-free floating-point bill helper is removed. Treasury
//! quotation uses [`treasury_bill::quote_treasury_bill`] with original dates.
//!
//! ```compile_fail
//! use convex_analytics::yields::bond_equivalent_yield_simple;
//! ```
//!
//! The unchecked floating-point discount helper is also removed:
//!
//! ```compile_fail
//! let _ = convex_analytics::yields::discount_yield_simple(99.0, 100.0, 90);
//! ```
//!
//! # Yield Types
//!
//! - **Yield-to-Maturity (YTM)**: Internal rate of return assuming all cash flows
//!   are received and reinvested at the same rate until maturity.
//!
//! - **Current Yield**: Annual coupon divided by clean price. Simple measure
//!   that ignores time value of money and capital gains/losses.
//!
//! - **Simple Yield**: Adds annualized capital gain/loss to current yield.
//!   Used in Japanese markets.
//!
//! - **Street Convention**: Standard market yield quote assuming reinvestment
//!   at the yield rate with standard day count conventions.
//!
//! - **True Yield**: Adjusts for actual settlement mechanics and reinvestment
//!   assumptions that differ from street convention.
//!
//! - **Money Market Yields**: Discount yield, bond equivalent yield (BEY),
//!   CD equivalent yield, and explicit constant-rate model returns.
//!
//! # Usage
//!
//! ```rust,ignore
//! use convex_analytics::yields::{YieldSolver, YieldResult, current_yield};
//! use convex_bonds::types::YieldConvention;
//!
//! // Using YieldSolver for YTM
//! let solver = YieldSolver::new()
//!     .with_convention(YieldConvention::StreetConvention);
//!
//! let result = solver.solve(&cash_flows, clean_price, accrued, settlement, day_count, frequency)?;
//! println!("YTM: {:.4}%", result.yield_value * 100.0);
//!
//! // Using simple current yield
//! let cy = current_yield(dec!(7.5), dec!(110.503));
//! ```
//!
//! # Unified Yield Engine
//!
//! For comprehensive yield calculations with full convention support,
//! use the [`YieldEngine`] trait and [`StandardYieldEngine`]:
//!
//! ```rust,ignore
//! use convex_analytics::yields::{StandardYieldEngine, YieldEngine};
//! use convex_bonds::types::YieldCalculationRules;
//!
//! let engine = StandardYieldEngine::default();
//! let rules = YieldCalculationRules::us_treasury();
//! let result = engine.yield_from_price(&cash_flows, clean_price, accrued, settlement, &rules)?;
//! ```

mod current;
mod engine;
mod money_market;
mod short_date;
mod simple;
pub(crate) mod solver;
mod street;
/// Explicit checked Treasury bill convention, independent from legacy helpers.
pub mod treasury_bill;

// Re-export all public types and functions
pub use current::{current_yield, current_yield_from_amount, current_yield_from_bond};
pub use engine::{
    current_yield_simple, simple_yield_f64, StandardYieldEngine, YieldEngine, YieldEngineResult,
};
pub use money_market::{
    bond_equivalent_yield, cd_equivalent_yield, constant_rate_return,
    constant_rate_return_with_horizon, discount_yield,
};
pub use short_date::ShortDateCalculator;
pub use simple::simple_yield;
pub use solver::{current_yield_decimal, current_yield_from_fixed_bond, YieldResult, YieldSolver};
pub use street::street_convention_yield;
