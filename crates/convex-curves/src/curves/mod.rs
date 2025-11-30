//! Yield curve types.
//!
//! This module provides various yield curve representations:
//!
//! - [`ZeroCurve`]: Zero-coupon yield curve (rates at pillars)
//! - [`DiscountCurve`]: Discount factor curve (primary curve type)
//! - [`ForwardCurve`]: Forward rate curve for specific tenors
//! - [`SpreadCurve`]: Spread over base curve (credit, basis)
//!
//! # Curve Hierarchy
//!
//! ```text
//! ┌─────────────────────┐
//! │    DiscountCurve    │ ← Primary curve, stores discount factors
//! └─────────┬───────────┘
//!           │
//!     ┌─────┴─────┐
//!     ▼           ▼
//! ┌─────────┐ ┌─────────────┐
//! │ Forward │ │ SpreadCurve │ ← Derived curves
//! │  Curve  │ │             │
//! └─────────┘ └─────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_curves::prelude::*;
//! use std::sync::Arc;
//!
//! // Build a discount curve
//! let ois_curve = Arc::new(
//!     DiscountCurveBuilder::new(Date::from_ymd(2025, 1, 1).unwrap())
//!         .add_pillar(1.0, 0.96)
//!         .add_pillar(5.0, 0.80)
//!         .with_interpolation(InterpolationMethod::LogLinear)
//!         .build()
//!         .unwrap()
//! );
//!
//! // Create forward curve for 3M rates
//! let sofr_3m = ForwardCurve::from_months(ois_curve.clone(), 3);
//!
//! // Create credit spread curve
//! let credit_curve = SpreadCurve::constant_spread(ois_curve, 0.01, SpreadType::Additive);
//! ```

mod discount;
mod forward;
mod spread;
mod zero;

pub use discount::{DiscountCurve, DiscountCurveBuilder};
pub use forward::{ForwardCurve, ForwardCurveBuilder};
pub use spread::{SpreadCurve, SpreadCurveBuilder, SpreadType};
pub use zero::{ZeroCurve, ZeroCurveBuilder};
