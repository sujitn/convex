//! Contribution analysis for portfolios.
//!
//! Provides risk and return contribution analytics:
//! - Duration contribution by holding
//! - DV01 contribution by holding
//! - Spread contribution by holding
//! - Aggregated contributions by sector, rating, etc.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_portfolio::contribution::{duration_contributions, dv01_contributions};
//!
//! let dur_contrib = duration_contributions(&portfolio.holdings, &config);
//! for c in dur_contrib.by_holding.iter().take(10) {
//!     println!("{}: {:.2}% of duration", c.id, c.contribution_pct);
//! }
//! ```

mod attribution;
mod risk;

pub use attribution::*;
pub use risk::*;
