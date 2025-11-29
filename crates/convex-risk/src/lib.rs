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

pub mod duration;
pub mod convexity;
pub mod dv01;
pub mod var;
pub mod hedging;
mod error;

pub use error::RiskError;

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::duration::*;
    pub use crate::convexity::*;
    pub use crate::dv01::*;
    pub use crate::RiskError;
}
