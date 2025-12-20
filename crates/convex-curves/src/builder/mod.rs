//! Fluent builder API for curve construction.
//!
//! The builder pattern provides a clean, readable way to construct
//! complex curves with multiple segments and sources.
//!
//! # Simple Curve Construction
//!
//! For single-segment curves, use the direct methods:
//!
//! ```rust,ignore
//! use convex_curves::builder::CurveBuilder;
//! use convex_core::types::{Date, Compounding};
//!
//! let curve = CurveBuilder::rate_curve(today)
//!     .with_zeros(tenors, rates, Compounding::Continuous)
//!     .interpolate(InterpolationMethod::MonotoneConvex)
//!     .build()?;
//! ```
//!
//! # Multi-Segment Curves
//!
//! For curves with different sources or interpolation per tenor range:
//!
//! ```rust,ignore
//! let curve = CurveBuilder::rate_curve(today)
//!     .segment(0.0..2.0)
//!         .with_zeros(short_tenors, short_rates, Compounding::Continuous)
//!         .interpolate(InterpolationMethod::Linear)
//!     .segment(2.0..10.0)
//!         .delegate(swap_curve)
//!         .interpolate(InterpolationMethod::MonotoneConvex)
//!     .segment_from(10.0)
//!         .delegate(long_curve)
//!         .extrapolate(ExtrapolationMethod::FlatForward)
//!     .build()?;
//! ```
//!
//! # Credit Curves
//!
//! ```rust,ignore
//! let credit_curve = CurveBuilder::credit_curve(today, 0.40)
//!     .with_survival_probabilities(tenors, survivals)
//!     .interpolate(InterpolationMethod::LogLinear)
//!     .build_credit_curve()?;
//! ```
//!
//! # Spread and Shifted Curves
//!
//! ```rust,ignore
//! // Spread over base curve
//! let corp_curve = CurveBuilder::rate_curve(today)
//!     .spread_over(govt_curve, 150.0)  // +150bps
//!     .build()?;
//!
//! // Parallel shift for scenario analysis
//! let shifted = CurveBuilder::rate_curve(today)
//!     .shift(base_curve, 50.0)  // +50bps
//!     .build()?;
//! ```

mod curve_builder;

pub use curve_builder::{CurveBuilder, CurveFamily, SegmentBuilder};
