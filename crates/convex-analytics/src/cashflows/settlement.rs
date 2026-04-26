//! Settlement date calculations and ex-dividend handling.
//!
//! Re-exports from `convex_bonds`. The analytics crate previously held a
//! near-equivalent copy with a slimmer `SettlementRules` (no `adjustment`
//! field); consolidated to keep one source of truth alongside the rules
//! types in `convex_bonds::types`.

pub use convex_bonds::cashflows::settlement::{SettlementCalculator, SettlementStatus};
pub use convex_bonds::types::{DayType, ExDivAccruedMethod, ExDividendRules, SettlementRules};
