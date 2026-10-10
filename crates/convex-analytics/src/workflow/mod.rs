//! Standalone supplied-input valuation, inverse and scenario workflows.
//! Product mathematics and model guards stay in their existing native modules.
//! No lookup, provider, clock, runtime or authoritative cash posting is performed.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod bond;
mod capability;
mod floating_workout;
mod hedge;
mod icma;
mod icma_workout;
mod inputs;
mod inverse;
mod valuation;
mod workout;

pub use bond::*;
pub use capability::*;
pub use floating_workout::*;
pub use hedge::*;
pub use icma::*;
pub use icma_workout::*;
pub use inputs::*;
pub use inverse::*;
pub use valuation::*;
pub use workout::*;

use convex_core::{cashflows::CashflowError, contracts::IndexId, fixings::FixingRequirement, Date};
use convex_curves::contracts::{CurveContractError, CurveKey};

/// Actual missing values, independent of a repository or provider.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum MissingInput {
    /// Exact immutable curve version.
    Curve(CurveKey),
    /// Administrator publication schedule entry, even for a future observation.
    Publication(IndexId, Date),
    /// Required eligible published history; future forecast is not a substitute.
    Fixing(FixingRequirement),
    /// Actual projection source for permitted future floating cash.
    Projection,
}

/// Stable economic outcomes and original product failures. No numeric fallback.
#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    /// Required resolved values are absent.
    #[error("missing supplied inputs: {0:?}")]
    MissingInput(Vec<MissingInput>),
    /// A measure has no economic meaning for this product.
    #[error("not applicable: {0}")]
    NotApplicable(&'static str),
    /// This economic/model combination has no admitted implementation.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),
    /// No unique parameter is identified at the admitted numerical resolution.
    #[error("non-unique inverse: {0}")]
    NonUnique(&'static str),
    /// No root in the unchanged caller bracket; never expand it silently.
    #[error("no root in [{lower}, {upper}], residuals {low_residual}, {high_residual}")]
    NoRoot {
        /// Original lower parameter bound.
        lower: f64,
        /// Original upper parameter bound.
        upper: f64,
        /// Actual lower native residual.
        low_residual: f64,
        /// Actual upper native residual.
        high_residual: f64,
    },
    /// Actual bounded solve failed or its final native residual was rejected.
    #[error("inverse did not converge after {iterations} iterations: residual {residual}")]
    NoConvergence {
        /// Actual work, not an invented iteration count.
        iterations: usize,
        /// Actual last native residual.
        residual: f64,
    },
    /// Invalid domain, units, bounds or quantity.
    #[error("{field}: {reason}")]
    Invalid {
        /// Failed input or operation.
        field: &'static str,
        /// Original explanation.
        reason: String,
    },
    /// Original checked bond error, including optional-contract restrictions.
    #[error(transparent)]
    Bond(convex_bonds::contracts::BondContractError),
    /// Original checked swap error.
    #[error(transparent)]
    Swap(convex_rates::SwapError),
    /// Original standard credit error, retaining model attribution/limits.
    #[error(transparent)]
    Credit(convex_credit::CdsError),
    /// Original optional-model price/grid/risk error; no guard is bypassed.
    #[error(transparent)]
    Optional(#[from] crate::optional_bond::OptionalBondError),
    /// Original futures economics/source error.
    #[error(transparent)]
    Futures(crate::futures::FuturesError),
    /// Original checked curve failure.
    #[error(transparent)]
    Curve(CurveContractError),
    /// Original primitive failure.
    #[error(transparent)]
    Cashflow(CashflowError),
    /// Original bounded mathematical failure.
    #[error(transparent)]
    Math(convex_math::MathError),
    /// Original signed-risk validation/calibration/model failure.
    #[error(transparent)]
    Risk(#[from] crate::quote_risk::QuoteRiskError),
}
/// Fallible complete workflow; failures never contain an accepted zero result.
pub type WorkflowResult<T> = Result<T, WorkflowError>;

fn invalid(field: &'static str, reason: impl Into<String>) -> WorkflowError {
    WorkflowError::Invalid {
        field,
        reason: reason.into(),
    }
}
fn finite(value: f64, field: &'static str) -> WorkflowResult<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid(field, "nonfinite result"))
    }
}
impl From<CashflowError> for WorkflowError {
    fn from(e: CashflowError) -> Self {
        match e {
            CashflowError::MissingFixings(v) => {
                Self::MissingInput(v.into_iter().map(MissingInput::Fixing).collect())
            }
            e => Self::Cashflow(e),
        }
    }
}
impl From<CurveContractError> for WorkflowError {
    fn from(e: CurveContractError) -> Self {
        match e {
            CurveContractError::MissingDependency(k) => {
                Self::MissingInput(vec![MissingInput::Curve(k)])
            }
            CurveContractError::Primitives(e) => e.into(),
            e => Self::Curve(e),
        }
    }
}
impl From<convex_bonds::contracts::BondContractError> for WorkflowError {
    fn from(e: convex_bonds::contracts::BondContractError) -> Self {
        match e {
            convex_bonds::contracts::BondContractError::Cashflow(e) => e.into(),
            e => Self::Bond(e),
        }
    }
}
impl From<convex_rates::SwapError> for WorkflowError {
    fn from(e: convex_rates::SwapError) -> Self {
        match e {
            convex_rates::SwapError::Cashflow(e) => e.into(),
            convex_rates::SwapError::Curve(e) => e.into(),
            e => Self::Swap(e),
        }
    }
}
impl From<convex_credit::CdsError> for WorkflowError {
    fn from(e: convex_credit::CdsError) -> Self {
        match e {
            convex_credit::CdsError::Cashflow(e) => e.into(),
            convex_credit::CdsError::Curve(e) => e.into(),
            convex_credit::CdsError::Solver(e) => e.into(),
            e => Self::Credit(e),
        }
    }
}
impl From<crate::futures::FuturesError> for WorkflowError {
    fn from(e: crate::futures::FuturesError) -> Self {
        match e {
            crate::futures::FuturesError::Cashflow(e) => e.into(),
            crate::futures::FuturesError::Curve(e) => e.into(),
            e => Self::Futures(e),
        }
    }
}
impl From<convex_math::MathError> for WorkflowError {
    fn from(e: convex_math::MathError) -> Self {
        match e {
            convex_math::MathError::InvalidBracket { a, b, fa, fb } => Self::NoRoot {
                lower: a,
                upper: b,
                low_residual: fa,
                high_residual: fb,
            },
            convex_math::MathError::ConvergenceFailed {
                iterations,
                residual,
            } => Self::NoConvergence {
                iterations: iterations as usize,
                residual,
            },
            e => Self::Math(e),
        }
    }
}
