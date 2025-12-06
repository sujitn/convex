//! Rate index infrastructure for floating rate bonds.
//!
//! This module provides:
//! - [`IndexFixingStore`]: Storage and retrieval of historical rate fixings
//! - [`OvernightCompounding`]: SOFR/SONIA compounding calculations
//! - [`IndexConventions`]: Market conventions for different rate indices
//! - [`ArrearConvention`]: Generalized overnight rate compounding conventions

mod conventions;
mod fixing_store;
mod overnight;

pub use conventions::{
    ArrearConvention, IndexConventions, IndexSource, PublicationTime, ShiftType,
};
pub use fixing_store::{IndexFixing, IndexFixingStore};
pub use overnight::OvernightCompounding;
