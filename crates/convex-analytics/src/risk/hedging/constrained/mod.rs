//! Bounded complete-factor proposals over supplied original bond/CDS/IRS/futures risk.
//! Product valuation, provider freshness, authoritative limits and orders stay outside.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod admission;
mod evidence;
mod issuer;
mod solver;
mod types;
pub use issuer::{
    solve_issuer_book_hedge_proposal, solve_issuer_lifecycle_hedge_proposal, IssuerRiskScope,
};
pub use solver::{
    aggregate_directional_hedge_risk, aggregate_hedge_risk, solve_book_hedge_proposal,
    solve_directional_hedge_proposal, solve_hedge_proposal, solve_lifecycle_hedge_proposal,
};
pub use types::*;

/// Invalid, missing, unsupported or bounded immutable input; no accepted proposal.
#[derive(Debug, thiserror::Error)]
pub enum HedgeError {
    /// Incoherent units, original pins, full risk, quantities or numerical policy.
    #[error("hedge {field}: {reason}")]
    Invalid {
        /// Exact original field.
        field: &'static str,
        /// Actual reason.
        reason: String,
    },
    /// Missing expected holdings or complete original factor values.
    #[error("hedge missing {0}")]
    Missing(String),
    /// Explicit dimension/quantity/work admission bound.
    #[error("hedge input bound: {0}")]
    Bound(&'static str),
}
fn invalid(field: &'static str, reason: impl Into<String>) -> HedgeError {
    HedgeError::Invalid {
        field,
        reason: reason.into(),
    }
}
fn finite(value: f64, field: &'static str) -> Result<f64, HedgeError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid(field, "nonfinite input or arithmetic"))
    }
}
