//! Portfolio-level analytics.
//!
//! This module provides aggregated analytics for portfolios, including:
//! - NAV and component breakdown
//! - Weighted yield metrics (YTM, YTW, YTC)
//! - Risk aggregation (duration, DV01, convexity)
//! - Spread analytics (Z-spread, OAS, etc.)
//! - Key rate duration profiles
//! - Credit quality metrics
//!
//! All functions are pure - they take holdings and configuration as input
//! and return computed results. No caching, no I/O, no side effects.

mod credit;
mod key_rates;
mod liquidity;
mod nav;
mod parallel;
mod risk;
mod spreads;
mod summary;
mod yields;

pub use credit::*;
pub use key_rates::*;
pub use liquidity::*;
pub use nav::*;
pub use parallel::*;
pub use risk::*;
pub use spreads::*;
pub use summary::*;
pub use yields::*;
