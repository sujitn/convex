//! # Convex Curves
//!
//! Yield curve construction and interpolation for the Convex fixed income analytics library.
//!
//! This crate provides:
//!
//! - **Curve Types**: Zero curves, discount curves, forward curves
//! - **Bootstrap**: Curve construction from market instruments
//! - **Interpolation**: Various interpolation methods for curves
//! - **Multi-Curve**: Multi-curve frameworks (OIS discounting)
//!
//! ## Example
//!
//! ```rust,ignore
//! use convex_curves::prelude::*;
//! use rust_decimal_macros::dec;
//!
//! // Build a zero curve from market rates
//! let curve = ZeroCurveBuilder::new()
//!     .reference_date(Date::from_ymd(2025, 1, 15).unwrap())
//!     .add_rate(Date::from_ymd(2025, 4, 15).unwrap(), dec!(0.045))
//!     .add_rate(Date::from_ymd(2025, 7, 15).unwrap(), dec!(0.048))
//!     .add_rate(Date::from_ymd(2026, 1, 15).unwrap(), dec!(0.050))
//!     .interpolation(InterpolationMethod::Linear)
//!     .build()
//!     .unwrap();
//!
//! let df = curve.discount_factor(Date::from_ymd(2025, 6, 15).unwrap());
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod bootstrap;
pub mod curves;
pub mod error;
pub mod interpolation;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::curves::{DiscountCurve, ForwardCurve, ZeroCurve, ZeroCurveBuilder};
    pub use crate::error::{CurveError, CurveResult};
    pub use crate::interpolation::InterpolationMethod;
}

pub use curves::{ZeroCurve, ZeroCurveBuilder};
pub use error::{CurveError, CurveResult};
