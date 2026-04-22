//! Yield-to-maturity solver — re-exported from `convex_bonds::pricing`.
//!
//! The canonical implementation lives in `convex-bonds` (it owns `BondCashFlow`).
//! This module keeps the `convex_analytics::yields` API stable by re-exporting
//! the solver types and providing the `*_decimal` / `*_from_fixed_bond`
//! naming variants that analytics callers have historically used.

use rust_decimal::Decimal;

use convex_bonds::traits::FixedCouponBond;

pub use convex_bonds::pricing::{YieldResult, YieldSolver};

/// Calculates current yield.
///
/// Current yield = Annual Coupon / Clean Price.
#[must_use]
pub fn current_yield_decimal(annual_coupon: Decimal, clean_price: Decimal) -> f64 {
    convex_bonds::pricing::current_yield(annual_coupon, clean_price)
}

/// Calculates current yield from a fixed coupon bond.
#[must_use]
pub fn current_yield_from_fixed_bond(bond: &dyn FixedCouponBond, clean_price: Decimal) -> f64 {
    convex_bonds::pricing::current_yield_from_bond(bond, clean_price)
}
