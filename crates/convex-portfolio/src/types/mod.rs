//! Domain types for portfolio analytics.
//!
//! This module provides type-safe representations of portfolio concepts:
//!
//! - [`Holding`]: A single bond position with pre-calculated analytics
//! - [`CashPosition`]: Cash with currency and FX rate
//! - [`Classification`]: Flexible sector/rating/seniority classification
//! - [`WeightingMethod`]: Portfolio weighting options
//! - [`AnalyticsConfig`]: Configuration for analytics computation

mod cash;
mod classification;
mod config;
mod holding;
mod maturity;
mod weighting;

// Re-export all types
pub use cash::CashPosition;
pub use classification::{
    Classification, CreditRating, RatingBucket, RatingInfo, Sector, SectorInfo, Seniority,
    SeniorityInfo,
};
pub use config::AnalyticsConfig;
pub use holding::{Holding, HoldingAnalytics, HoldingBuilder};
pub use maturity::MaturityBucket;
pub use weighting::WeightingMethod;
