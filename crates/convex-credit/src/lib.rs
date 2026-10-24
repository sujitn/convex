//! Pure single-name CDS economics over supplied calendars and checked curves.
//!
//! No clock, market provider, database, MQ, application runtime or external
//! pricing engine enters this crate. Native economics rebuild through checked
//! construction; projected premium cash is conditional on survival, not paid cash.
//!
//! This application is based on the ISDA CDS Standard Model (version 1.8.3),
//! developed and supported in collaboration with Markit Group Ltd.
//! See the separately retained public license and `docs/CDS-VALUATION.md`.

#![warn(missing_docs)]
#![forbid(unsafe_code)]
#![deny(unused_must_use)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod contract;
mod conversion;
mod pricing;
mod terms;

pub use contract::{cds_roll_after, cds_roll_on_or_before, CdsContract, PremiumPeriod};
pub use conversion::{convert_cds, CdsQuote, ConversionResult, ConversionSettings};
pub use pricing::{
    discounted_default_mass, price_cds, price_cds_remaining_exposure, price_flat_hazard,
    CdsValuation, CreditInput, PremiumValue,
};
pub use terms::*;

/// Required attribution for integrations exposing the standard-model route.
pub const STANDARD_MODEL_NOTICE: &str = "This application is based on the ISDA CDS Standard Model (version 1.8.3), developed and supported in collaboration with Markit Group Ltd.";
/// Hard ceiling on root iterations, independent of caller settings.
pub const MAX_CONVERSION_ITERATIONS: usize = 256;
/// Maximum nodes across discount and survival curves, including coupon boundaries.
pub const MAX_INTEGRATION_POINTS: usize = 4096;

/// Checked input, dependency, unsupported convention or numerical failure.
#[derive(Debug, Clone, thiserror::Error)]
pub enum CdsError {
    /// Shared bounded calendar/schedule error.
    #[error(transparent)]
    Cashflow(#[from] convex_core::cashflows::CashflowError),
    /// Checked opaque identity/unit error.
    #[error(transparent)]
    Contract(#[from] convex_core::contracts::ContractError),
    /// Exact dependency, curve role or query failure.
    #[error(transparent)]
    Curve(#[from] convex_curves::contracts::CurveContractError),
    /// Supported root solver failed; no converted quote is returned.
    #[error(transparent)]
    Solver(#[from] convex_math::MathError),
    /// Unimplemented economic/model behavior is never approximated.
    #[error("unsupported {0}")]
    Unsupported(&'static str),
    /// A model postcondition failed rather than becoming zero.
    #[error("numerical failure: {0}")]
    Numerical(&'static str),
}
pub(crate) fn invalid(field: &'static str, reason: impl Into<String>) -> CdsError {
    convex_core::cashflows::CashflowError::InvalidTerms {
        field,
        reason: reason.into(),
    }
    .into()
}
pub(crate) fn checked_date(value: convex_core::Date) -> Result<(), CdsError> {
    if !(1..=9999).contains(&value.year()) {
        return Err(invalid("date", "outside 0001..9999"));
    }
    Ok(())
}
pub(crate) fn finite(value: f64) -> Result<f64, CdsError> {
    if !value.is_finite() {
        return Err(CdsError::Numerical("nonfinite model result"));
    }
    Ok(value)
}
