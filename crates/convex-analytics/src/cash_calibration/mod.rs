//! Pure original cash helpers, government/issuer/repo fits and dated bond carry.
//! Exact immutable supplied inputs only; no providers, clock, storage or runtime.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod build;
mod fit;
mod inputs;
mod pricing;
mod repo;
mod reprice;

pub use build::build_cash_curve_set;
pub use inputs::*;
pub use pricing::{convert_cash_quote, price_basis_bond};
pub use repo::*;
pub use reprice::{reprice_cash_helpers, CashHelperRepricing};

use convex_bonds::contracts::BondContractError;
use convex_core::{cashflows::CashflowError, Date};
use convex_curves::contracts::*;

/// Maximum original helpers/parameters in one atomic build.
pub const MAX_CASH_HELPERS: usize = convex_math::optimization::MAX_FIT_DIMENSION;
/// Maximum actual helper valuations across roots, global trials and repricing.
pub const MAX_CASH_EVALUATIONS: usize = 16_384;

/// Checked economic, exact dependency or measured numerical failure.
#[derive(Debug, thiserror::Error)]
pub enum CashCalibrationError {
    /// Original curve contract failure; no partial curve set is returned.
    #[error(transparent)]
    Curve(#[from] CurveContractError),
    /// Original full bond schedule/entitlement/optional failure.
    #[error(transparent)]
    Bond(#[from] BondContractError),
    /// Original bounded date/settlement/profile failure.
    #[error(transparent)]
    Cashflow(#[from] CashflowError),
    /// Original recovery integration failure.
    #[error(transparent)]
    Credit(#[from] convex_credit::CdsError),
    /// Original shared bounded root failure.
    #[error(transparent)]
    Root(#[from] convex_math::MathError),
    /// Actual global convergence/rank diagnostics, retaining its economic error.
    #[error(transparent)]
    Global(Box<convex_math::optimization::FitFailure<CashCalibrationError>>),
}
/// Fallible pure output; caller's last-good inputs remain untouched.
pub type CashCalibrationResult<T> = Result<T, CashCalibrationError>;

fn invalid(field: &str, reason: impl Into<String>) -> CashCalibrationError {
    CurveContractError::InvalidInput {
        field: field.into(),
        reason: reason.into(),
    }
    .into()
}
fn label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
fn finite(v: f64, field: &str) -> CashCalibrationResult<f64> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(invalid(field, "nonfinite model arithmetic"))
    }
}
