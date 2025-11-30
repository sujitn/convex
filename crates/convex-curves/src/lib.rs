//! # Convex Curves
//!
//! Yield curve construction and interpolation for the Convex fixed income analytics library.
//!
//! This crate provides:
//!
//! - **Curve Trait**: Core [`Curve`] trait for all curve operations
//! - **Curve Types**: Zero curves, discount curves, forward curves, spread curves
//! - **Bootstrap**: Curve construction from market instruments
//! - **Interpolation**: Various interpolation methods for curves
//! - **Multi-Curve**: Multi-curve frameworks (OIS discounting)
//! - **Compounding**: Interest rate compounding conventions
//! - **Instruments**: Curve instruments for bootstrapping (deposits, FRAs, swaps, bonds)
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use convex_curves::prelude::*;
//!
//! // Build a discount curve from pillar points
//! let curve = DiscountCurveBuilder::new(Date::from_ymd(2025, 1, 1).unwrap())
//!     .add_pillar(0.25, 0.99)   // 3M discount factor
//!     .add_pillar(0.5, 0.98)    // 6M
//!     .add_pillar(1.0, 0.96)    // 1Y
//!     .add_pillar(2.0, 0.92)    // 2Y
//!     .with_interpolation(InterpolationMethod::MonotoneConvex)
//!     .build()
//!     .unwrap();
//!
//! // Get discount factor at 1.5 years
//! let df = curve.discount_factor(1.5).unwrap();
//!
//! // Get continuously compounded zero rate
//! let rate = curve.zero_rate(1.5, Compounding::Continuous).unwrap();
//!
//! // Get forward rate between 1Y and 2Y
//! let fwd = curve.forward_rate(1.0, 2.0).unwrap();
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod bootstrap;
pub mod builder;
pub mod compounding;
pub mod conventions;
pub mod curves;
pub mod error;
pub mod instruments;
pub mod interpolation;
pub mod repricing;
pub mod traits;
pub mod validation;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::bootstrap::{
        GlobalBootstrapper, IterativeMultiCurveBootstrapper, SequentialBootstrapper,
    };
    pub use crate::builder::{BootstrapMethod, CurveBuilder, CurveBuilderExt, ExtrapolationType};
    pub use crate::compounding::Compounding;
    pub use crate::conventions;
    pub use crate::curves::{
        DiscountCurve, DiscountCurveBuilder, ForwardCurve, SpreadCurve, SpreadType, ZeroCurve,
        ZeroCurveBuilder,
    };
    pub use crate::error::{CurveError, CurveResult};
    pub use crate::instruments::{
        CurveInstrument, Deposit, FRA, InstrumentType, OIS, RateIndex, RateFuture, Swap,
        TreasuryBill, TreasuryBond,
    };
    pub use crate::interpolation::InterpolationMethod;
    pub use crate::traits::Curve;
    pub use crate::validation::{CurveValidator, ValidationError, ValidationReport, ValidationWarning};
    pub use crate::repricing::{
        BootstrapResult, RepricingCheck, RepricingReport, tolerances,
    };
}

pub use compounding::Compounding;
pub use curves::{DiscountCurve, DiscountCurveBuilder, ZeroCurve, ZeroCurveBuilder};
pub use error::{CurveError, CurveResult};
pub use traits::Curve;
