//! Curve implementations.
//!
//! This module provides concrete curve types:
//!
//! - [`DiscreteCurve`]: Curve from discrete point data with interpolation
//! - [`SegmentedCurve`]: Multiple segments with different sources/interpolation
//! - [`DelegatedCurve`]: Wraps another curve with fallback handling
//! - [`DerivedCurve`]: Transforms a base curve (shift, spread, scale)
//! - [`DiscountCurveBuilder`]: Simple builder for discount curves

mod discrete;
mod derived;
mod delegated;
mod segmented;

pub use discrete::DiscreteCurve;
pub use derived::{CurveTransform, DerivedCurve};
pub use delegated::{DelegatedCurve, DelegationFallback};
pub use segmented::{CurveSegment, SegmentedCurve, SegmentSource};

// Re-export compatibility types
pub use crate::compat::{DiscountCurveBuilder, ForwardCurve, ZeroCurve, ZeroCurveBuilder};
