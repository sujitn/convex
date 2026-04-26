//! Accrued interest calculations.
//!
//! Re-exports `AccruedInterestCalculator` from `convex_bonds`. The analytics
//! crate previously held a byte-equivalent copy; consolidated to keep one
//! source of truth for accrued-interest math (standard, ex-dividend, ICMA
//! irregular periods, year-fraction).

pub use convex_bonds::cashflows::AccruedInterestCalculator;
