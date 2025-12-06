//! Multi-curve framework for post-LIBOR pricing.
//!
//! This module provides a complete multi-curve framework supporting:
//!
//! - **Discount curves**: OIS curves (SOFR, €STR, SONIA) for discounting
//! - **Projection curves**: Term rate curves for forward rate projection
//! - **FX forward curves**: Cross-currency forward rates
//! - **Curve sensitivities**: DV01, key rate durations, parallel shifts
//!
//! # Multi-Curve Architecture
//!
//! In the post-LIBOR world, multiple curves are needed for accurate pricing:
//!
//! ```text
//!                     ┌─────────────────┐
//!                     │   OIS Curve     │ (SOFR, €STR, SONIA)
//!                     │  (Discounting)  │
//!                     └────────┬────────┘
//!                              │
//!          ┌───────────────────┼───────────────────┐
//!          ▼                   ▼                   ▼
//! ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
//! │  1M Projection  │ │  3M Projection  │ │  6M Projection  │
//! │     Curve       │ │     Curve       │ │     Curve       │
//! └─────────────────┘ └─────────────────┘ └─────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_curves::multicurve::*;
//!
//! // Build a multi-curve environment
//! let curves = MultiCurveBuilder::new(date!(2024-11-29))
//!     // Discount curve (SOFR OIS)
//!     .add_ois("1M", 0.0530)
//!     .add_ois("1Y", 0.0510)
//!     .add_ois("5Y", 0.0450)
//!     // Term SOFR 3M projection curve
//!     .add_projection(RateIndex::TermSOFR3M, "2Y", 0.0485)
//!     .add_projection(RateIndex::TermSOFR3M, "5Y", 0.0455)
//!     .build()?;
//!
//! // Use the curves
//! let df = curves.discount_factor(date!(2025-11-29))?;
//! let fwd = curves.forward_rate(&RateIndex::TermSOFR3M, start, end)?;
//! ```

mod builder;
mod curve_set;
mod fx_forward;
mod rate_index;
mod sensitivity;

pub use builder::MultiCurveBuilder;
pub use curve_set::{CurveSet, CurveSetBuilder};
pub use fx_forward::{CurrencyPair, FxForwardCurve, FxForwardCurveBuilder};
pub use rate_index::{RateIndex, Tenor};
pub use sensitivity::{BumpType, CurveSensitivityCalculator, KeyRateDuration};
