//! Curve instrument wrappers for yield curve bootstrapping.
//!
//! This module provides wrappers around generic bond types that implement
//! the `CurveInstrument` trait from `convex-curves`, enabling these bonds
//! to be used in curve construction.
//!
//! # Supported Bond Types
//!
//! - [`GovernmentZeroCoupon`]: Zero-coupon government bonds (T-Bills, Gilts, etc.)
//! - [`GovernmentCouponBond`]: Fixed coupon government bonds (T-Notes, Gilts, Bunds, JGBs)
//!
//! # Market Conventions
//!
//! Different markets use different conventions:
//!
//! | Market | Day Count | Frequency | Settlement |
//! |--------|-----------|-----------|------------|
//! | US Treasury | ACT/ACT | Semi-annual | T+1 |
//! | UK Gilt | ACT/365F | Semi-annual | T+1 |
//! | German Bund | ACT/ACT ICMA | Annual | T+2 |
//! | JGB | ACT/365F | Semi-annual | T+2 |
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_bonds::prelude::*;
//! use convex_curves::prelude::*;
//!
//! // Build a Gilt curve from UK government bonds
//! let settlement = Date::from_ymd(2025, 1, 15).unwrap();
//!
//! let bill = GovernmentZeroCoupon::new(
//!     ZeroCouponBond::new("GB0000000001", maturity, Currency::GBP),
//!     settlement,
//!     99.50,  // Market price
//!     MarketConvention::UKGilt,
//! );
//!
//! let curve = CurveBuilder::new(settlement)
//!     .add(bill)
//!     .bootstrap()?;
//! ```

mod conventions;
mod gov_coupon;
mod gov_zero;

pub use conventions::{day_count_factor, MarketConvention};
pub use gov_coupon::GovernmentCouponBond;
pub use gov_zero::GovernmentZeroCoupon;
