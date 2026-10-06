//! Pure checked single-currency fixed/term IRS and fixed/overnight OIS contracts.
//!
//! Supply immutable calendars, publication times, fixing history and forecasts.
//! Generated cash is expected contractual cash, never a recorded payment or a
//! discounted swap valuation. No market convention is inferred from an index.

#![warn(missing_docs)]
#![forbid(unsafe_code)]
#![deny(unused_must_use)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod cashflows;
mod contract;
/// Supplied multi-curve valuation over the same dated cashflow contract.
pub mod pricing;
/// Checked common typed price results and truthful native capabilities.
pub mod results;
mod terms;

pub use cashflows::{
    CashflowContext, CouponCashflow, CouponEvidence, ForecastRate, ProjectionRequest, RateUse,
    ResolvedResetSlice, SwapCashflows,
};
pub use contract::{FloatingCouponPlan, ResetSlice, SwapContract};
pub use terms::{
    CalendarBinding, CollateralTerms, FixedLegTerms, FixingPolicy, FloatingLegTerms,
    FloatingProfiles, LegCalendarPins, LegCalendars, LegTerms, OvernightLegTerms, ReferencePin,
    SwapCalendars, SwapSide, SwapTerms, TermLegTerms, TermRatePeriod, TermResetTerms,
};

use convex_core::{cashflows::CashflowError, Date};

/// Total reset slices, daily observations or unique requirements in one swap.
pub const MAX_SWAP_TERMS: usize = 65_536;
/// Conservative ceiling for profile scans during construction/cash calculation.
pub const MAX_PROFILE_VISITS: usize = 16_777_216;

/// Checked contract, missing-input, unsupported-economics or arithmetic failure.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SwapError {
    /// Shared date/profile/fixing/decimal failure, including complete missing lists.
    #[error(transparent)]
    Cashflow(#[from] CashflowError),
    /// Exact dependency, curve-role, query-domain or numerical curve failure.
    #[error(transparent)]
    Curve(#[from] convex_curves::contracts::CurveContractError),
    /// Contract requires mathematics not supported by this domain route.
    #[error("unsupported {feature}: {reason}")]
    Unsupported {
        /// Unsupported economic feature.
        feature: &'static str,
        /// Why an implicit approximation is invalid.
        reason: &'static str,
    },
}

pub(crate) fn invalid(field: &'static str, reason: impl Into<String>) -> SwapError {
    CashflowError::InvalidTerms {
        field,
        reason: reason.into(),
    }
    .into()
}
pub(crate) fn date(date: Date) -> Result<(), SwapError> {
    if !(1..=9999).contains(&date.year()) {
        return Err(invalid("date", "outside 0001..9999"));
    }
    Ok(())
}
