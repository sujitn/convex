//! Signed reference quote risk over immutable supplied economics.
//! Native units, actual position scaling, response semantics and validation
//! policies are explicit. No clock, provider, runtime, publication or ledger.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub(crate) mod finite;
mod graph;
mod optional;
mod optional_graph;
mod scenario;
mod types;

pub use finite::*;
pub use graph::*;
pub use optional::*;
pub use optional_graph::*;
pub use scenario::*;
pub use types::*;

/// Typed admission, valuation and calibration failures; no accepted zero fallback.
#[derive(Debug, thiserror::Error)]
pub enum QuoteRiskError {
    /// Invalid or incompatible original risk input.
    #[error("quote risk {field}: {reason}")]
    Invalid {
        /// Exact invalid input/operation.
        field: String,
        /// Original failure explanation.
        reason: String,
    },
    /// Original checked curve failure.
    #[error(transparent)]
    Curve(#[from] convex_curves::contracts::CurveContractError),
    /// Original mixed rate calibration failure.
    #[error(transparent)]
    Rate(#[from] crate::rate_calibration::RateCalibrationError),
    /// Original CDS calibration failure.
    #[error(transparent)]
    Credit(#[from] crate::credit_calibration::CreditCalibrationError),
    /// Original cash calibration failure.
    #[error(transparent)]
    Cash(#[from] crate::cash_calibration::CashCalibrationError),
    /// Original model-specific optional-bond risk/refinement/resolution failure.
    #[error(transparent)]
    Optional(#[from] crate::optional_bond::OptionalBondError),
    /// Original complete basket/cash/carry failure, retaining unsupported/missing inputs.
    #[error(transparent)]
    Futures(#[from] crate::futures::FuturesError),
}
/// Checked risk result.
pub type QuoteRiskResult<T> = Result<T, QuoteRiskError>;
/// Wrap an unrounded native currency PV; this is not a per100 price converter.
pub fn risk_reference_pv(
    value: f64,
    currency: convex_core::Currency,
) -> QuoteRiskResult<convex_core::contracts::ModelMoney> {
    let value = convex_core::contracts::FiniteF64::new(value)
        .map_err(|e| invalid("reference PV", e.to_string()))?;
    Ok(convex_core::contracts::ModelMoney::new(value, currency))
}
fn invalid(field: &str, reason: impl Into<String>) -> QuoteRiskError {
    QuoteRiskError::Invalid {
        field: field.into(),
        reason: reason.into(),
    }
}
fn finite(value: f64, field: &str) -> QuoteRiskResult<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid(field, "nonfinite monetary result"))
    }
}
fn label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
