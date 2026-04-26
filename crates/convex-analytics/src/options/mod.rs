//! Option pricing models for callable and puttable bonds.
//!
//! Re-exports the lattice-based pricing primitives from `convex_bonds::options`,
//! which is the single source of truth for bond-with-embedded-option machinery
//! (binomial tree, Hull-White short rate, trinomial HW1F, swaption pricing).
//! The analytics crate previously held a parallel binomial/HW1F implementation;
//! consolidated to one set of types.

pub use convex_bonds::options::{BinomialTree, HullWhite, ModelError, ShortRateModel};

use crate::AnalyticsError;

impl From<ModelError> for AnalyticsError {
    fn from(err: ModelError) -> Self {
        AnalyticsError::CalculationFailed(err.to_string())
    }
}
