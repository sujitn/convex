//! Convex Term Structure Framework
//!
//! A comprehensive curve framework for fixed income pricing, supporting:
//!
//! - Multiple curve types (rate, credit, inflation, FX)
//! - Segmented interpolation with different methods per tenor range
//! - Curve composition through delegation, derivation, and spreading
//! - Global calibration from market instruments
//! - Curve bumping for sensitivity analysis
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use convex_curves::{CurveBuilder, RateCurve, InterpolationMethod};
//! use convex_core::types::{Date, Compounding};
//!
//! // Build a SOFR curve from market data
//! let sofr_curve = CurveBuilder::rate_curve(today)
//!     .segment(0.0..2.0)
//!         .discrete_zeros(short_tenors, short_rates, Compounding::Continuous)
//!         .interpolate(InterpolationMethod::Linear)
//!     .segment(2.0..10.0)
//!         .calibrate(swap_instruments)
//!         .interpolate(InterpolationMethod::MonotoneConvex)
//!     .build()?;
//!
//! let sofr = RateCurve::new(sofr_curve)?;
//! let df_5y = sofr.discount_factor(today.add_years(5))?;
//! ```
//!
//! # Architecture
//!
//! ## Core Trait: `TermStructure`
//!
//! The fundamental abstraction for any curve. Provides:
//! - `value_at(t)`: Raw curve value at tenor t
//! - `value_type()`: What the values represent (DF, zero rate, etc.)
//! - `derivative_at(t)`: Optional derivative for forward calculations
//!
//! ## Domain Wrappers
//!
//! Provide semantic operations on top of any `TermStructure`:
//!
//! - [`RateCurve<T>`]: `discount_factor()`, `zero_rate()`, `forward_rate()`
//! - [`CreditCurve<T>`]: `survival_probability()`, `hazard_rate()`
//! - `InflationCurve<T>`: `index_ratio()`, `real_rate()` (planned)
//! - `FxCurve<T>`: `forward_rate()`, `forward_points()` (planned)
//!
//! ## Curve Types
//!
//! - [`DiscreteCurve`]: Curve from point data with interpolation
//! - [`SegmentedCurve`]: Different sources/interpolation per tenor range
//! - [`DelegatedCurve`]: Wraps another curve with fallback handling
//! - [`DerivedCurve`]: Transforms a base curve (shift, spread, etc.)
//!
//! ## Builder Pattern
//!
//! Fluent API for constructing complex curves:
//!
//! ```rust,ignore
//! let curve = CurveBuilder::rate_curve(today)
//!     .segment(0.0..2.0)
//!         .discrete_zeros(tenors, rates, Compounding::Continuous)
//!         .interpolate(InterpolationMethod::Linear)
//!     .segment(2.0..)
//!         .delegate(swap_curve)
//!         .extrapolate_with(ExtrapolationMethod::Flat)
//!     .build()?;
//! ```
//!
//! # Thread Safety
//!
//! All term structures implement `Send + Sync`, enabling safe use in
//! parallel pricing scenarios.

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_fields_in_debug)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::float_cmp)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::redundant_else)]
#![allow(clippy::needless_lifetimes)]
#![allow(clippy::extra_unused_lifetimes)]
#![allow(clippy::elidable_lifetime_names)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::unused_self)]
#![allow(clippy::wildcard_imports)]
#![allow(clippy::redundant_guards)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::borrowed_box)]
#![allow(clippy::derivable_impls)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::unnecessary_lazy_evaluations)]
#![allow(clippy::explicit_iter_loop)]

// Core modules
mod conversion;
mod error;
mod term_structure;
mod value_type;

// Curve implementations
pub mod curves;

// Domain wrappers
pub mod wrappers;

// Builder API
pub mod builder;

// Calibration engine
pub mod calibration;

// Curve bumping
pub mod bumping;

// Multi-curve environment
pub mod multicurve;

// Re-exports for convenience
pub use conversion::ValueConverter;
pub use error::{CurveError, CurveResult};
pub use term_structure::{Curve, CurveRef, TermStructure};
pub use value_type::ValueType;

// Re-export curve types
pub use curves::{CurveTransform, DelegationFallback, SegmentSource};
pub use curves::{DelegatedCurve, DerivedCurve, DiscreteCurve, SegmentedCurve};
pub use curves::{DiscountCurve, DiscountCurveBuilder, ForwardCurve, ZeroCurve, ZeroCurveBuilder};

// Re-export wrappers
pub use wrappers::{CreditCurve, RateCurve, RateCurveDyn};

// Re-export builder
pub use builder::{CurveBuilder, CurveFamily, SegmentBuilder};

// Re-export calibration types
pub use calibration::{
    CalibrationInstrument, CalibrationResult, CurveInstrument, Deposit, FitterConfig, Fra, Future,
    GlobalFitter, InstrumentSet, InstrumentType, Ois, SequentialBootstrapper, Swap,
};

// Re-export bumping types
pub use bumping::{
    key_rate_profile, ArcBumpedCurve, ArcKeyRateBumpedCurve, ArcScenarioCurve, BumpedCurve,
    KeyRateBump, KeyRateBumpedCurve, ParallelBump, Scenario, ScenarioBump, ScenarioCurve,
    STANDARD_KEY_TENORS,
};

// Re-export multicurve types
pub use multicurve::{Currency, CurrencyPair, MultiCurveEnvironment, RateIndex, Tenor};

// Re-export core types for convenience
pub use convex_core::types::Compounding;

/// Interpolation methods available for curve construction.
///
/// Re-exported from `convex-math` for convenience.
pub mod interpolation {
    pub use convex_math::interpolation::{
        CubicSpline, Interpolator, LinearInterpolator, LogLinearInterpolator, MonotoneConvex,
        NelsonSiegel, Svensson,
    };
}

/// Extrapolation methods for curve extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtrapolationMethod {
    /// No extrapolation - error if outside range.
    #[default]
    None,
    /// Flat extrapolation - use boundary value.
    Flat,
    /// Linear extrapolation using boundary slope.
    Linear,
    /// Flat forward - constant instantaneous forward rate.
    FlatForward,
}

/// Interpolation method selection for curve construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterpolationMethod {
    /// Simple linear interpolation.
    Linear,
    /// Log-linear interpolation (for discount factors).
    LogLinear,
    /// Natural cubic spline.
    CubicSpline,
    /// Monotone convex (production default, positive forwards).
    #[default]
    MonotoneConvex,
    /// Piecewise constant (for hazard rates).
    PiecewiseConstant,
    /// Nelson-Siegel parametric model.
    NelsonSiegel,
    /// Svensson parametric model.
    Svensson,
}

impl InterpolationMethod {
    /// Returns true if this method guarantees positive forward rates.
    #[must_use]
    pub fn guarantees_positive_forwards(&self) -> bool {
        matches!(
            self,
            InterpolationMethod::MonotoneConvex | InterpolationMethod::LogLinear
        )
    }

    /// Returns the continuity class of the interpolation.
    ///
    /// - C0: Continuous values
    /// - C1: Continuous first derivative
    /// - C2: Continuous second derivative
    #[must_use]
    pub fn continuity_class(&self) -> &'static str {
        match self {
            InterpolationMethod::Linear => "C0",
            InterpolationMethod::LogLinear => "C0",
            InterpolationMethod::CubicSpline => "C2",
            InterpolationMethod::MonotoneConvex => "C1",
            InterpolationMethod::PiecewiseConstant => "C-1",
            InterpolationMethod::NelsonSiegel | InterpolationMethod::Svensson => "C∞",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpolation_method_properties() {
        assert!(InterpolationMethod::MonotoneConvex.guarantees_positive_forwards());
        assert!(InterpolationMethod::LogLinear.guarantees_positive_forwards());
        assert!(!InterpolationMethod::Linear.guarantees_positive_forwards());
        assert!(!InterpolationMethod::CubicSpline.guarantees_positive_forwards());
    }

    #[test]
    fn test_interpolation_continuity() {
        assert_eq!(InterpolationMethod::Linear.continuity_class(), "C0");
        assert_eq!(InterpolationMethod::CubicSpline.continuity_class(), "C2");
        assert_eq!(InterpolationMethod::MonotoneConvex.continuity_class(), "C1");
    }
}
