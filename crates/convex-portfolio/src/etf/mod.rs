//! ETF-specific analytics.
//!
//! Provides ETF-focused calculations including:
//! - NAV and iNAV (indicative NAV)
//! - Premium/discount to NAV
//! - Creation/redemption basket analysis
//! - SEC 30-day yield and compliance metrics
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_portfolio::etf::{EtfMetrics, calculate_etf_metrics};
//!
//! let metrics = calculate_etf_metrics(&portfolio);
//!
//! println!("NAV per share: ${:.4}", metrics.nav_per_share);
//! println!("iNAV: ${:.4}", metrics.inav);
//! println!("Premium/Discount: {:.2}%", metrics.premium_discount_pct.unwrap_or(0.0));
//! ```

mod basket;
mod nav;
mod sec;

pub use basket::*;
pub use nav::*;
pub use sec::*;
