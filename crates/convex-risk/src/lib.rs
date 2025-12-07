//! # convex-risk
//!
//! Risk analytics for fixed income instruments.
//!
//! This crate provides comprehensive risk calculations including:
//!
//! - **Duration**: Macaulay, Modified, Effective, Key Rate, Spread
//! - **Convexity**: Analytical and Effective
//! - **DV01/PV01**: Dollar value of a basis point
//! - **VaR**: Value at Risk (Historical and Parametric)
//! - **Hedging**: Hedge ratios and portfolio risk
//!
//! ## Example
//!
//! ```ignore
//! use convex_risk::prelude::*;
//! use convex_bonds::FixedRateBond;
//!
//! let bond = FixedRateBond::builder()
//!     .coupon_rate(0.05)
//!     .maturity(date!(2030-06-15))
//!     .build()?;
//!
//! let duration = modified_duration(&bond, settlement, ytm)?;
//! let dv01 = dv01_from_duration(duration, dirty_price, face_value);
//! ```

#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::similar_names)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::struct_field_names)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::cast_possible_truncation)]
#![allow(dead_code)]

pub mod calculator;
pub mod convexity;
pub mod duration;
pub mod dv01;
mod error;
pub mod hedging;
pub mod var;

pub use calculator::{
    BondRiskCalculator, BondRiskMetrics, EffectiveDurationCalculator, KeyRateDurationCalculator,
};
pub use error::RiskError;

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::calculator::*;
    pub use crate::convexity::*;
    pub use crate::duration::*;
    pub use crate::dv01::*;
    pub use crate::RiskError;
}
