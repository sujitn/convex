//! Bond pricing — re-exported from `convex_bonds::pricing`.
//!
//! `BondPricer` and `PriceResult` are defined in `convex-bonds`; this module
//! keeps the `convex_analytics::pricing` path stable for existing callers.

pub use convex_bonds::pricing::{BondPricer, PriceResult};
