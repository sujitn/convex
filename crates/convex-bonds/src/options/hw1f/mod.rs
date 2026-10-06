//! Checked constant-parameter Hull–White calculations on supplied inputs.
//!
//! This route uses exact OU step moments and numerical initial-curve fitting.
//! The older binomial/trinomial compatibility APIs retain their own contracts.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod european;
mod lattice;

pub use european::{bond_option, swaption, OptionSide, SwaptionPayment};
pub use lattice::{event_grid, Hw1fTree, TreeDiagnostics, MAX_TREE_NODES, MAX_TREE_STEPS};

use serde::{Deserialize, Serialize};

/// Explicit annual model parameters. Rates/volatility are decimals, time is years.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hw1fParameters {
    /// OU mean reversion per year, including the Ho–Lee limit of zero.
    pub mean_reversion: f64,
    /// Short-rate diffusion per square-root year; zero is a declared assumption.
    pub volatility: f64,
}
impl Hw1fParameters {
    /// Validate without absolute values, clipping or replacement parameters.
    pub fn validate(self) -> Result<(), Hw1fError> {
        if !self.mean_reversion.is_finite()
            || !(0.0..=10.0).contains(&self.mean_reversion)
            || !self.volatility.is_finite()
            || !(0.0..=1.0).contains(&self.volatility)
        {
            return Err(invalid(
                "parameters",
                "requires a in [0,10], sigma in [0,1]",
            ));
        }
        Ok(())
    }
}

/// Input, resource or numerical failure; no valid model price accompanies it.
#[derive(Debug, thiserror::Error)]
pub enum Hw1fError {
    /// Invalid supplied input.
    #[error("invalid {field}: {reason}")]
    Invalid {
        /// Input field.
        field: &'static str,
        /// Failed condition.
        reason: String,
    },
    /// Explicit bounded work or dimension ceiling.
    #[error("HW1F resource bound: {0}")]
    Bound(&'static str),
    /// Invalid arithmetic is never a market value.
    #[error("nonfinite or invalid numerical result: {0}")]
    Numerical(&'static str),
    /// Original bounded root error.
    #[error(transparent)]
    Math(#[from] convex_math::MathError),
}
pub(super) fn invalid(field: &'static str, reason: impl Into<String>) -> Hw1fError {
    Hw1fError::Invalid {
        field,
        reason: reason.into(),
    }
}
pub(super) fn finite(value: f64, operation: &'static str) -> Result<f64, Hw1fError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Hw1fError::Numerical(operation))
    }
}
pub(super) fn positive(value: f64, operation: &'static str) -> Result<f64, Hw1fError> {
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(Hw1fError::Numerical(operation))
    }
}
pub(super) fn b(a: f64, time: f64) -> f64 {
    if a == 0.0 {
        time
    } else {
        -(-a * time).exp_m1() / a
    }
}
