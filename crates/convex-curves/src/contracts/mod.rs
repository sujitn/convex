//! Pure, checked construction and query contracts. Inputs contain actual values;
//! no clock, registry, repository, runtime or provider is consulted.
//!
//! Deserialize input DTOs, then call [`build_curve_set`]. Built objects have
//! private validated state and cannot bypass validation by deserialization.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod build;
mod inputs;
mod numerics;
mod query;
mod transformations;

pub use build::{build_curve_set, BuiltCurve, CurveBuildOutput, CurveSet};
pub use inputs::*;
pub use numerics::*;
pub use query::*;
pub use transformations::*;

/// Maximum recipes in one atomic curve build, including supplied dependencies.
pub const MAX_CURVES: usize = 256;
/// Maximum nodes or calibration helpers per curve.
pub const MAX_CURVE_INPUTS: usize = 1024;
/// Maximum curve/query tenor, in years of the selected time basis.
pub const MAX_CURVE_TENOR: f64 = 150.0;
/// Minimum distinct pillar separation, in years, to avoid an ill-conditioned grid.
pub const MIN_PILLAR_SEPARATION: f64 = 1e-10;
/// Maximum nested immutable derived views, independently of batch size.
pub const MAX_CURVE_DERIVATIONS: usize = 32;

/// Stable checked construction/query failures. No failure returns a curve.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CurveContractError {
    /// Shared checked schedule/profile/fixing failure, preserving missing lists.
    #[error(transparent)]
    Primitives(#[from] convex_core::cashflows::CashflowError),
    /// Invalid units, dimensions, dates or numerical domain.
    #[error("{field}: {reason}")]
    InvalidInput {
        /// Field or operation that failed.
        field: String,
        /// Invariant that was violated.
        reason: String,
    },
    /// Representable terms/methods with no supported implementation in this route.
    #[error("unsupported {capability}: {reason}")]
    Unsupported {
        /// Requested operation or model.
        capability: String,
        /// Required implementation or input contract.
        reason: String,
    },
    /// An exact immutable curve version is absent.
    #[error("missing dependency {0:?}")]
    MissingDependency(CurveKey),
    /// Recipe dependencies form a cycle; no partial set is published.
    #[error("cyclic curve dependencies: {0:?}")]
    DependencyCycle(Vec<CurveKey>),
    /// Final native quote repricing did not satisfy its supplied tolerance.
    #[error("helper {helper_id}: residual {residual} exceeds tolerance {tolerance}")]
    RepricingFailure {
        /// Helper identity.
        helper_id: String,
        /// Signed model minus input, in the original quote unit.
        residual: f64,
        /// Supplied absolute native quote tolerance.
        tolerance: f64,
    },
    /// A finite numerical computation failed its domain/postcondition.
    #[error("numerical failure: {0}")]
    Numerical(String),
    /// Setup/query work exceeds an explicit ceiling.
    #[error("resource bound exceeded: {0}")]
    BoundExceeded(&'static str),
}

impl CurveContractError {
    pub(crate) fn invalid(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidInput {
            field: field.into(),
            reason: reason.into(),
        }
    }
    pub(crate) fn unsupported(capability: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Unsupported {
            capability: capability.into(),
            reason: reason.into(),
        }
    }
}

/// Fallible result for the checked pure API.
pub type CurveContractResult<T> = Result<T, CurveContractError>;
