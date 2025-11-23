//! Yield curve types.
//!
//! This module provides various yield curve representations:
//!
//! - [`ZeroCurve`]: Zero-coupon yield curve
//! - [`DiscountCurve`]: Discount factor curve
//! - [`ForwardCurve`]: Forward rate curve

mod discount;
mod forward;
mod zero;

pub use discount::DiscountCurve;
pub use forward::ForwardCurve;
pub use zero::{ZeroCurve, ZeroCurveBuilder};
