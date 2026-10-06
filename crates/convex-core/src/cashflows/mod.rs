//! Checked shared building blocks for bond, rates and credit cashflows.
//!
//! Calendars, contracts, fixing histories and time are supplied by callers.
//! These functions project contractual amounts; they do not record payments.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod amounts;
mod overnight;
mod profiles;
mod schedule;

pub use amounts::{
    principal_cashflows, CashRounding, CouponCalculation, CouponPrincipal, CouponProfiles,
    ExCouponAccrual, NegativeCouponPolicy, PrincipalCashflow,
};
pub use overnight::{
    CompoundedCoupon, OvernightConvention, OvernightObservation, OvernightSchedule,
    ResolvedOvernightObservation,
};
pub use profiles::{BoundarySide, DatedProfile, DatedValue, PrincipalChange, ProfileKind};
pub use schedule::{
    settlement_date, CouponPeriod, CouponSchedule, DateOffset, DateRules, ExCouponBoundary,
    ExCouponRule, ExDateAnchor, PaymentDateEntitlement, ReferencePeriod, ScheduleCalendars,
    ScheduleSpec, StubType,
};

use crate::{error::ConvexError, fixings::FixingRequirement, types::Date};

/// Maximum coupon periods or entries in one independently dated profile.
pub const MAX_SCHEDULE_PERIODS: usize = 4096;
/// Maximum contractual span, in calendar days (150 years at 366 days/year).
pub const MAX_CONTRACT_DAYS: i64 = 150 * 366;
/// Maximum lookback, payment, fixing or settlement offset in days.
pub const MAX_DATE_OFFSET: i32 = 366;

/// Typed validation, missing-input and arithmetic failures for shared primitives.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CashflowError {
    /// Invalid supplied contract or inconsistent input dimensions.
    #[error("{field}: {reason}")]
    InvalidTerms {
        /// Invalid input field.
        field: &'static str,
        /// Specific validation failure.
        reason: String,
    },
    /// Work or input exceeds a documented resource ceiling.
    #[error("resource bound exceeded: {0}")]
    BoundExceeded(&'static str),
    /// Calendar or date calculation failed, without substituting a date.
    #[error("date/calendar: {0}")]
    Date(String),
    /// Checked decimal operation overflowed or had an invalid denominator.
    #[error("decimal arithmetic: {0}")]
    Arithmetic(&'static str),
    /// All observations required to have been published but absent at the cutoff.
    #[error("missing {} required published fixing(s)", .0.len())]
    MissingFixings(Vec<FixingRequirement>),
}

impl CashflowError {
    pub(crate) fn terms(field: &'static str, reason: impl Into<String>) -> Self {
        Self::InvalidTerms {
            field,
            reason: reason.into(),
        }
    }
}

impl From<ConvexError> for CashflowError {
    fn from(value: ConvexError) -> Self {
        Self::Date(value.to_string())
    }
}

pub(crate) fn validate_date(date: Date) -> Result<(), CashflowError> {
    if !(1..=9999).contains(&date.year()) {
        return Err(CashflowError::terms("date", "year outside 0001–9999"));
    }
    Ok(())
}

pub(crate) fn validate_span(start: Date, end: Date) -> Result<(), CashflowError> {
    validate_date(start)?;
    validate_date(end)?;
    if end <= start {
        return Err(CashflowError::terms("dates", "end must follow start"));
    }
    if start.days_between(&end) > MAX_CONTRACT_DAYS {
        return Err(CashflowError::BoundExceeded("contract span"));
    }
    Ok(())
}
