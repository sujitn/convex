//! Checked complete supplied bond terms and cashflow composition over core.
//!
//! Construction and expected cash are pure. Optional valuation must request an
//! explicit model; contractual cash projections are not an option price.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod exercise;
mod projection;
mod terms;

pub(crate) use exercise::{call_terms, put_terms};
pub use exercise::{ExerciseAccrued, ExerciseDates, ExerciseRight, ExerciseStyle, ExerciseTerms};
pub use projection::{
    BondProjection, BondProjectionContext, BondProjectionPolicy, CouponProjection,
    OvernightAccrualCutoff, ProjectionRequest,
};
pub use terms::{
    BondCalendarIds, BondCalendars, BondCoupon, BondProfiles, BondTerms, OvernightRateTerms,
    PrincipalPrice, TermRateTerms,
};

use convex_core::{
    cashflows::{
        CashflowError, CouponPeriod, CouponProfiles, CouponSchedule, DateRules, OvernightSchedule,
        PrincipalCashflow, MAX_SCHEDULE_PERIODS,
    },
    Date,
};
use rust_decimal::Decimal;

/// Maximum total prepared/reset/overnight terms or profile segments in one bond.
pub const MAX_BOND_TERMS: usize = 65_536;

/// Checked bond validation, missing-history and unsupported economic failures.
#[derive(Debug, Clone, thiserror::Error)]
pub enum BondContractError {
    /// Core dates/profiles/fixing/decimal failures, retaining missing requirements.
    #[error(transparent)]
    Cashflow(#[from] CashflowError),
    /// Existing checked calendar/instrument admission failure.
    #[error(transparent)]
    Bond(#[from] crate::BondError),
    /// Explicitly unsupported economic contract or valuation model.
    #[error("unsupported {feature}: {reason}")]
    Unsupported {
        /// Contract/model feature.
        feature: &'static str,
        /// Reason it cannot be simplified.
        reason: String,
    },
}

pub(super) fn invalid(field: &'static str, reason: impl Into<String>) -> BondContractError {
    CashflowError::InvalidTerms {
        field,
        reason: reason.into(),
    }
    .into()
}
pub(super) fn unsupported(feature: &'static str, reason: impl Into<String>) -> BondContractError {
    BondContractError::Unsupported {
        feature,
        reason: reason.into(),
    }
}

#[derive(Debug, Clone)]
struct ResetSlice {
    start: Date,
    end: Date,
    request: ProjectionRequest,
}
#[derive(Debug, Clone)]
struct CouponPlan {
    period: CouponPeriod,
    resets: Vec<ResetSlice>,
    overnight: Option<OvernightSchedule>,
}

/// Immutable checked composition. Its full original terms are serializable;
/// reconstruct through `BondTerms::build[_with_calendars]`, never unchecked
/// deserialization into precomputed schedules or projected cash.
#[derive(Debug, Clone)]
pub struct BondContract {
    terms: BondTerms,
    profiles: CouponProfiles,
    coupons: Vec<CouponPlan>,
    principal: Vec<PrincipalCashflow>,
}

impl BondContract {
    fn new(terms: BondTerms, calendars: &BondCalendars<'_>) -> Result<Self, BondContractError> {
        if terms.schema_version != 1
            || !(1..=9999).contains(&terms.issue_date.year())
            || terms.issue_date >= terms.schedule.end_date
            || terms.face_value <= Decimal::ZERO
            || terms.face_value > Decimal::from(1_000_000_000_000_000_000_i64)
            || terms.schedule.frequency.is_zero()
        {
            return Err(invalid("bond", "invalid schema/date/face/coupon frequency"));
        }
        if terms.dates.fixing_offset.is_some() || terms.dates.observation_offset.is_some() {
            return Err(invalid(
                "dates",
                "fixings belong to independent reset/overnight terms",
            ));
        }
        // Validate unused settlement fields as well as coupon fields.
        convex_core::cashflows::settlement_date(
            terms.issue_date,
            terms.settlement_lag,
            terms.settlement_adjustment,
            calendars.payment,
        )?;
        let profiles = terms.profiles.checked()?;
        let schedule = CouponSchedule::generate(&terms.schedule, &terms.dates, calendars.payment)?;
        let first = schedule
            .periods()
            .first()
            .ok_or_else(|| invalid("schedule", "empty"))?;
        let last = schedule
            .periods()
            .last()
            .ok_or_else(|| invalid("schedule", "empty"))?;
        if profiles.notional().coverage() != (first.accrual_dates().0, last.accrual_dates().1)
            || profiles.notional().entries()[0].value != terms.face_value
        {
            return Err(invalid(
                "profiles",
                "coverage must equal actual accrual; initial principal must equal face",
            ));
        }
        if terms
            .profiles
            .notional
            .windows(2)
            .any(|p| p[1].value > p[0].value)
        {
            return Err(unsupported(
                "principal advance",
                "initial scope supports nonincreasing outstanding principal",
            ));
        }
        let mut principal = convex_core::cashflows::principal_cashflows(
            profiles.notional(),
            &terms.dates,
            calendars.payment,
            terms.redemption_multiplier,
            terms.rounding,
        )?;
        if terms.principal_prices.len() > MAX_SCHEDULE_PERIODS
            || terms
                .principal_prices
                .windows(2)
                .any(|p| p[0].effective_date >= p[1].effective_date)
        {
            return Err(invalid(
                "principal_prices",
                "requires bounded sorted unique effective dates",
            ));
        }
        for price in &terms.principal_prices {
            if price.multiplier < Decimal::ZERO || price.multiplier > Decimal::TEN {
                return Err(invalid("principal_prices", "multiplier outside 0..10"));
            }
            let position = principal
                .iter()
                .position(|p| p.effective_date == price.effective_date)
                .ok_or_else(|| {
                    invalid("principal_prices", "requires an actual principal reduction")
                })?;
            let mut remainder = principal[position].clone();
            let allocation = price.amount.unwrap_or(remainder.principal.repayment);
            if allocation <= Decimal::ZERO || allocation > remainder.principal.repayment {
                return Err(invalid(
                    "principal_prices.amount",
                    "must be within actual repayment",
                ));
            }
            let after_part = remainder
                .principal
                .before
                .checked_sub(allocation)
                .ok_or(CashflowError::Arithmetic("principal allocation"))?;
            principal[position].principal.after = after_part;
            principal[position].principal.repayment = allocation;
            principal[position].amount = terms.rounding.apply(
                allocation
                    .checked_mul(price.multiplier)
                    .ok_or(CashflowError::Arithmetic("principal redemption price"))?,
            )?;
            if allocation < remainder.principal.repayment {
                remainder.principal.before = after_part;
                remainder.principal.repayment = remainder
                    .principal
                    .repayment
                    .checked_sub(allocation)
                    .ok_or(CashflowError::Arithmetic("principal remainder"))?;
                let multiplier = if remainder.is_redemption {
                    terms.redemption_multiplier
                } else {
                    Decimal::ONE
                };
                remainder.amount = terms.rounding.apply(
                    remainder
                        .principal
                        .repayment
                        .checked_mul(multiplier)
                        .ok_or(CashflowError::Arithmetic("principal remainder price"))?,
                )?;
                principal.insert(position + 1, remainder);
            }
        }
        let mut coupons: Vec<_> = schedule
            .periods()
            .iter()
            .cloned()
            .map(|period| CouponPlan {
                period,
                resets: vec![],
                overnight: None,
            })
            .collect();
        match &terms.coupon {
            BondCoupon::Fixed => {
                if terms
                    .profiles
                    .gearing
                    .iter()
                    .any(|p| p.value != Decimal::ZERO)
                {
                    return Err(invalid(
                        "gearing",
                        "fixed coupons require zero index gearing",
                    ));
                }
            }
            BondCoupon::Term(rate) => {
                if matches!(
                    rate.projection_day_count,
                    convex_core::daycounts::DayCountConvention::ActActIcma
                ) {
                    return Err(unsupported(
                        "term projection ICMA",
                        "requires independent index reference-period quotation",
                    ));
                }
                let reset_dates = DateRules {
                    accrual_adjustment: rate.reset_adjustment,
                    fixing_offset: Some(rate.fixing_offset),
                    ..DateRules::default()
                };
                let reset_grid =
                    CouponSchedule::generate(&rate.resets, &reset_dates, calendars.reset)?;
                let reset_start = reset_grid
                    .periods()
                    .first()
                    .ok_or_else(|| invalid("resets", "empty"))?
                    .accrual_dates()
                    .0;
                let reset_end = reset_grid
                    .periods()
                    .last()
                    .ok_or_else(|| invalid("resets", "empty"))?
                    .accrual_dates()
                    .1;
                if rate.resets.frequency.is_zero()
                    || reset_start > first.accrual_dates().0
                    || reset_end < last.accrual_dates().1
                {
                    return Err(invalid(
                        "resets",
                        "reset grid must cover the entire actual accrual interval",
                    ));
                }
                let mut count = 0;
                for coupon in &mut coupons {
                    let (start, end) = coupon.period.accrual_dates();
                    for reset in reset_grid.periods() {
                        let (left, right) = reset.accrual_dates();
                        let a = left.max(start);
                        let b = right.min(end);
                        if b > a {
                            count += 1;
                            if count > MAX_BOND_TERMS {
                                return Err(
                                    CashflowError::BoundExceeded("bond reset slices").into()
                                );
                            }
                            coupon.resets.push(ResetSlice {
                                start: a,
                                end: b,
                                request: ProjectionRequest {
                                    index_id: rate.index_id.clone(),
                                    observation_date: reset
                                        .fixing_date()
                                        .ok_or_else(|| invalid("fixing", "absent"))?,
                                    rate_start: left,
                                    rate_end: right,
                                    day_count: rate.projection_day_count,
                                },
                            });
                        }
                    }
                }
            }
            BondCoupon::Overnight(rate) => {
                if terms.coupon_principal != convex_core::cashflows::CouponPrincipal::AccrualProfile
                {
                    return Err(unsupported(
                        "overnight principal basis",
                        "requires actual accrual principal",
                    ));
                }
                let mut count = 0;
                for coupon in &mut coupons {
                    let (start, end) = coupon.period.accrual_dates();
                    for profile in [&terms.profiles.gearing, &terms.profiles.notional] {
                        if profile.windows(2).any(|p| {
                            p[1].start > start && p[1].start < end && p[0].value != p[1].value
                        }) {
                            return Err(unsupported("overnight intracoupon principal/gearing step",
                                "requires an explicit segmented compounding contract; cannot use an averaged rate/principal"));
                        }
                    }
                    let overnight = OvernightSchedule::generate(
                        start,
                        end,
                        terms.day_count,
                        calendars.observation,
                        rate.convention,
                    )?;
                    count += overnight.observations().len();
                    if count > MAX_BOND_TERMS {
                        return Err(CashflowError::BoundExceeded("bond overnight terms").into());
                    }
                    coupon.overnight = Some(overnight);
                }
            }
        }
        if terms.exercise.len() > MAX_SCHEDULE_PERIODS {
            return Err(CashflowError::BoundExceeded("exercise terms").into());
        }
        let mut previous = std::collections::BTreeMap::new();
        for exercise in &terms.exercise {
            exercise.validate(
                terms.issue_date,
                terms.schedule.end_date,
                calendars.exercise,
            )?;
            let (start, end) = exercise.style.bounds()?;
            let key = match exercise.right {
                ExerciseRight::Call => 0,
                ExerciseRight::Put => 1,
            };
            if previous.get(&key).is_some_and(|prev| *prev >= start) {
                return Err(invalid(
                    "exercise",
                    "same-right schedules must be sorted and nonoverlapping",
                ));
            }
            previous.insert(key, end);
        }
        let bond = Self {
            terms,
            profiles,
            coupons,
            principal,
        };
        // Validate deterministic effective rate/rounding on every period at admission.
        // Floating effective rates are checked after actual observation resolution.
        if matches!(bond.terms.coupon, BondCoupon::Fixed) {
            bond.fixed_projection(
                bond.terms.issue_date,
                convex_core::cashflows::PaymentDateEntitlement::Exclude,
            )?;
        }
        Ok(bond)
    }

    /// Full supplied terms, without inferred or averaged economic replacements.
    pub fn terms(&self) -> &BondTerms {
        &self.terms
    }
    /// Actual prepared coupon periods, with distinct date/reference fields.
    pub fn periods(&self) -> impl Iterator<Item = &CouponPeriod> {
        self.coupons.iter().map(|p| &p.period)
    }
    /// Actual prepared term-reset slices and requests, using the calendars
    /// supplied at checked construction. Optional models must not regenerate
    /// these with a compiled calendar that drops the caller's holiday versions.
    pub fn term_reset_slices(
        &self,
    ) -> impl Iterator<Item = (&CouponPeriod, Date, Date, &ProjectionRequest)> {
        self.coupons.iter().flat_map(|coupon| {
            coupon
                .resets
                .iter()
                .map(move |reset| (&coupon.period, reset.start, reset.end, &reset.request))
        })
    }
    /// Full expected principal changes, distinct from realized cash.
    pub fn principal_cashflows(&self) -> &[PrincipalCashflow] {
        &self.principal
    }
    /// Boundary outstanding principal in absolute units. At maturity After is
    /// zero, even when the contractual payment is delayed. Outside life is zero.
    pub fn outstanding_principal(
        &self,
        date: Date,
        side: convex_core::cashflows::BoundarySide,
    ) -> Result<Decimal, BondContractError> {
        let (start, end) = self.profiles.notional().coverage();
        if !(1..=9999).contains(&date.year()) {
            return Err(invalid("date", "outside 0001..9999"));
        }
        if date < start
            || date > end
            || (date == start && side == convex_core::cashflows::BoundarySide::Before)
            || (date == end && side == convex_core::cashflows::BoundarySide::After)
        {
            return Ok(Decimal::ZERO);
        }
        Ok(self.profiles.notional().value_at(date, side)?)
    }
    /// A deterministic cash projection cannot be treated as an optional price.
    pub fn require_plain_valuation(&self) -> Result<(), BondContractError> {
        if self.terms.exercise.is_empty() {
            Ok(())
        } else {
            Err(unsupported("option valuation", "requires explicit exercise/model/volatility inputs; straight cashflows are conditional"))
        }
    }
}
