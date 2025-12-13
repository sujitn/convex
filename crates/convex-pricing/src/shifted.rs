//! Curve shifting utilities.
//!
//! This module re-exports curve transformation types from `convex-curves`.
//!
//! # Available Types
//!
//! - [`ShiftedCurve`]: Applies a constant parallel shift to rates
//! - [`ScaledCurve`]: Scales all rates by a constant factor
//! - [`BlendedCurve`]: Blends two curves with configurable weights
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_pricing::ShiftedCurve;
//! use convex_curves::traits::Curve;
//!
//! let base_curve = /* your curve */;
//! let shifted = ShiftedCurve::new(&base_curve, 0.0050); // +50 bps
//!
//! // The shifted curve can be used anywhere a Curve is expected
//! let df = shifted.discount_factor(1.0)?;
//! ```

// Re-export from convex-curves for convenience
pub use convex_curves::curves::shifted::{BlendedCurve, ScaledCurve, ShiftedCurve};
