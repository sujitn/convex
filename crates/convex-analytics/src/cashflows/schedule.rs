//! Schedule generation.
//!
//! Re-exports `Schedule`, `ScheduleConfig`, and `StubType` from `convex_bonds`.
//! The analytics crate previously held a byte-equivalent copy; consolidated to
//! keep one source of truth for schedule generation. Callers needing the
//! types from this path continue to work unchanged.

pub use convex_bonds::cashflows::{Schedule, ScheduleConfig, StubType};
