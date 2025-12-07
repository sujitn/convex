//! # convex-yas
//!
//! Bloomberg YAS (Yield Analysis System) replication.
//!
//! This crate provides comprehensive yield analysis matching Bloomberg's YAS function,
//! including:
//!
//! - **Yield Calculations**: Street convention, true yield, current yield, simple yield
//! - **Spread Calculations**: G-spread, I-spread, Z-spread, ASW spread
//! - **Risk Metrics**: Duration, convexity, DV01
//! - **Settlement Invoice**: Accrued interest, settlement amount
//!
//! ## Bloomberg Validation
//!
//! All calculations are validated against Bloomberg YAS for the reference bond:
//! - Boeing 7.5% 06/15/2025 (CUSIP: 097023AH7)
//! - Settlement: 04/29/2020
//! - Price: 110.503
//!
//! ## Example
//!
//! ```ignore
//! use convex_yas::prelude::*;
//! use convex_bonds::FixedRateBond;
//!
//! let bond = FixedRateBond::builder()
//!     .cusip("097023AH7")
//!     .coupon_rate(0.075)
//!     .maturity(date!(2025-06-15))
//!     .build()?;
//!
//! let analysis = YasAnalysis::calculate(&bond, settlement, price, &curve)?;
//!
//! println!("Street Convention: {}", analysis.street_convention);
//! println!("G-Spread: {} bps", analysis.g_spread);
//! println!("Modified Duration: {}", analysis.modified_duration);
//! ```

pub mod calculator;
pub mod yas;
pub mod yields;
pub mod invoice;
pub mod formatting;
mod error;

pub use calculator::{
    BatchYASCalculator, BloombergReference, ValidationFailure, YASCalculator, YASResult,
};
pub use error::YasError;
pub use yas::YasAnalysis;

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::yas::*;
    pub use crate::yields::*;
    pub use crate::invoice::*;
    pub use crate::YasError;
}
