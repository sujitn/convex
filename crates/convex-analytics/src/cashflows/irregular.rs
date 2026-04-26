//! Irregular (stub) period handling.
//!
//! Re-exports from `convex_bonds::cashflows::irregular`. The analytics crate
//! previously held a near-equivalent copy with a 3-variant `IrregularStubType`;
//! consolidated to a single source. `IrregularStubType` is kept as an alias of
//! `convex_bonds::types::StubType` for callers using the analytics path.

pub use convex_bonds::cashflows::irregular::IrregularPeriodHandler;
pub use convex_bonds::types::{ReferenceMethod, StubType as IrregularStubType};
