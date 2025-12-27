//! Benchmark comparison analytics.
//!
//! Provides benchmark-relative analysis:
//! - Active weights by holding and sector
//! - Tracking error estimation
//! - Duration and spread differences
//! - Contribution to tracking error
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_portfolio::benchmark::{active_weights, benchmark_comparison};
//!
//! let comparison = benchmark_comparison(&portfolio, &benchmark, &config);
//! println!("Duration difference: {:.2}", comparison.duration_diff);
//! println!("Spread difference: {:.1}bp", comparison.spread_diff);
//!
//! for (sector, weight) in comparison.active_weights.by_sector.iter() {
//!     println!("{:?}: {:.2}%", sector, weight);
//! }
//! ```

mod comparison;
mod tracking;

pub use comparison::*;
pub use tracking::*;
