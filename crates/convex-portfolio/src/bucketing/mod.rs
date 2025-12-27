//! Portfolio bucketing and classification.
//!
//! This module provides bucketing capabilities for portfolio analysis:
//!
//! - **Sector bucketing**: Distribution by issuer sector
//! - **Rating bucketing**: Distribution by credit rating
//! - **Maturity bucketing**: Distribution by time to maturity
//! - **Custom bucketing**: User-defined classification schemes
//!
//! All functions are pure - they take holdings and return distributions
//! without modifying state.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_portfolio::bucketing::*;
//! use convex_portfolio::prelude::*;
//!
//! let holdings = portfolio.holdings();
//! let config = AnalyticsConfig::default();
//!
//! // Get sector distribution
//! let by_sector = bucket_by_sector(holdings, &config);
//! for (sector, metrics) in &by_sector {
//!     println!("{}: {:.2}% weight", sector, metrics.weight_pct);
//! }
//!
//! // Get rating distribution
//! let by_rating = bucket_by_rating(holdings, &config);
//! let ig_weight = by_rating.investment_grade_weight();
//! ```

mod custom;
mod maturity;
mod rating;
mod sector;

pub use custom::*;
pub use maturity::*;
pub use rating::*;
pub use sector::*;
