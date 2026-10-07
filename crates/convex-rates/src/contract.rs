//! Checked immutable schedules and actual reset/overnight coupon plans.

use convex_core::{
    calendars::BusinessDayConvention,
    cashflows::{
        BoundarySide, CashflowError, CouponCalculation, CouponPeriod, CouponPrincipal,
        CouponProfiles, CouponSchedule, DateOffset, DateRules, DatedProfile, DatedValue,
        OvernightSchedule, ProfileKind, ScheduleCalendars,
    },
    daycounts::DayCountConvention,
    Date,
};
use rust_decimal::Decimal;

use crate::{
    date, invalid, CalendarBinding, FixedLegTerms, FloatingLegTerms, LegCalendars, LegTerms,
    ReferencePin, SwapCalendars, SwapError, SwapTerms, TermRatePeriod, TermResetTerms,
    MAX_PROFILE_VISITS, MAX_SWAP_TERMS,
};

/// A coupon accrual slice consuming one independently dated term observation.
/// Rate interval and coupon accrual interval need not coincide (e.g. a stub).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ResetSlice {
    /// Actual coupon accrual slice start.
    pub accrual_start: Date,
    /// Actual coupon accrual slice end.
    pub accrual_end: Date,
    /// Observation date after fixing offset and adjustment.
    pub observation_date: Date,
    /// Start of the actual index quotation interval.
    pub rate_start: Date,
    /// End of the actual index quotation interval.
    pub rate_end: Date,
    /// Simple annualized index quotation day count.
    pub projection_day_count: DayCountConvention,
    /// Contractual index tenor in months, distinct from reset/payment frequency.
    pub tenor_months: u32,
}

/// One checked immutable floating coupon plan.
#[derive(Debug, Clone)]
pub struct FloatingCouponPlan {
    pub(crate) period: CouponPeriod,
    pub(crate) resets: Vec<ResetSlice>,
    pub(crate) overnight: Option<OvernightSchedule>,
}
impl FloatingCouponPlan {
    /// Exact contractual/accrual/payment/reference dates.
    pub fn period(&self) -> &CouponPeriod {
        &self.period
    }
    /// Actual term reset slices; empty for OIS.
    pub fn reset_slices(&self) -> &[ResetSlice] {
        &self.resets
    }
    /// Actual overnight weights; absent for term IRS.
    pub fn overnight_schedule(&self) -> Option<&OvernightSchedule> {
        self.overnight.as_ref()
    }
}

/// Immutable admitted contract. Serialize the original terms and rebuild with
/// actual exact-version calendars; cached plans cannot bypass validation.
#[derive(Debug, Clone)]
pub struct SwapContract {
    pub(crate) terms: SwapTerms,
    pub(crate) fixed_schedule: CouponSchedule,
    pub(crate) reset_schedule: Option<CouponSchedule>,
    pub(crate) floating: Vec<FloatingCouponPlan>,
    pub(crate) fixed_profiles: CouponProfiles,
    pub(crate) floating_profiles: CouponProfiles,
}

impl SwapContract {
    pub(crate) fn new(terms: SwapTerms, calendars: &SwapCalendars<'_>) -> Result<Self, SwapError> {
        if terms.schema_version != 1 {
            return Err(invalid("schema_version", "expected 1"));
        }
        date(terms.effective_date)?;
        date(terms.maturity_date)?;
        if terms.effective_date >= terms.maturity_date {
            return Err(invalid("swap_dates", "maturity must follow effective date"));
        }
        if terms.collateral.currency != terms.currency {
            return Err(SwapError::Unsupported {
                feature: "collateral currency",
                reason: "requires cross-currency discount/FX economics",
            });
        }
        terms
            .collateral
            .discount_curve
            .validate()
            .map_err(|e| invalid("discount_curve", e.to_string()))?;
        terms
            .floating
            .projection_curve()
            .validate()
            .map_err(|e| invalid("projection_curve", e.to_string()))?;
        let fixed_schedule = leg_schedule(&terms.fixed.leg, calendars.fixed, &terms)?;
        let float_schedule = leg_schedule(terms.floating.leg(), calendars.floating, &terms)?;
        let fixed_profiles = fixed_profiles(&terms.fixed, &fixed_schedule)?;
        let floating_profiles = floating_profiles(&terms.floating, &float_schedule)?;
        let mut floating: Vec<_> = float_schedule
            .periods()
            .iter()
            .cloned()
            .map(|period| FloatingCouponPlan {
                period,
                resets: Vec::new(),
                overnight: None,
            })
            .collect();
        let (reset_schedule, scans) = match &terms.floating {
            FloatingLegTerms::Term(t) => {
                let (schedule, scans) =
                    prepare_term(&terms, &t.resets, calendars, &float_schedule, &mut floating)?;
                (Some(schedule), scans)
            }
            FloatingLegTerms::Overnight(t) => {
                prepare_overnight(t, calendars, &mut floating)?;
                (None, 0)
            }
        };
        profile_budget(&terms, &fixed_schedule, &floating, scans)?;
        let contract = Self {
            terms,
            fixed_schedule,
            reset_schedule,
            floating,
            fixed_profiles,
            floating_profiles,
        };
        for period in contract.fixed_schedule.periods() {
            contract.fixed_profiles.coupon_amount(
                period,
                calculation(&contract.terms.fixed.leg, Decimal::ZERO),
            )?;
        }
        Ok(contract)
    }

    /// Exact original economic terms and immutable resource/dependency pins.
    pub fn terms(&self) -> &SwapTerms {
        &self.terms
    }
    /// Actual checked fixed coupons/reference periods, using supplied calendars.
    pub fn fixed_schedule(&self) -> &CouponSchedule {
        &self.fixed_schedule
    }
    /// Actual independent term reset grid; absent for OIS.
    pub fn term_reset_schedule(&self) -> Option<&CouponSchedule> {
        self.reset_schedule.as_ref()
    }
    /// Actual floating coupons and reset/observation plans.
    pub fn floating_coupons(&self) -> &[FloatingCouponPlan] {
        &self.floating
    }
    /// Fixed/floating notional at the explicit accrual boundary. Outside accrual
    /// life is zero; this is a notional state, never a principal payment.
    pub fn outstanding_notional(
        &self,
        fixed: bool,
        at: Date,
        side: BoundarySide,
    ) -> Result<Decimal, SwapError> {
        date(at)?;
        let profile = if fixed {
            self.fixed_profiles.notional()
        } else {
            self.floating_profiles.notional()
        };
        let (a, b) = profile.coverage();
        if at < a
            || at > b
            || (at == a && side == BoundarySide::Before)
            || (at == b && side == BoundarySide::After)
        {
            return Ok(Decimal::ZERO);
        }
        Ok(profile.value_at(at, side)?)
    }
}

fn prepare_term(
    terms: &SwapTerms,
    t: &TermResetTerms,
    calendars: &SwapCalendars<'_>,
    float_schedule: &CouponSchedule,
    floating: &mut [FloatingCouponPlan],
) -> Result<(CouponSchedule, usize), SwapError> {
    let mut prepared = 0;
    let mut scans = 0;
    if calendars.observation.is_some() {
        return Err(invalid("observation_calendar", "irrelevant to term leg"));
    }
    let reset = binding(calendars.reset, &t.reset_calendar, "reset_calendar")?;
    let fixing = binding(calendars.fixing, &t.fixing_calendar, "fixing_calendar")?;
    validate_reset_terms(t, terms)?;
    let rules = DateRules {
        accrual_adjustment: t.reset_adjustment,
        fixing_offset: Some(t.fixing_offset),
        ..DateRules::default()
    };
    let schedule = CouponSchedule::generate_with_calendars(
        &t.schedule,
        &rules,
        &ScheduleCalendars {
            accrual: reset.calendar,
            payment: reset.calendar,
            fixing: fixing.calendar,
            observation: reset.calendar,
            ex_coupon: reset.calendar,
        },
    )?;
    let coverage = schedule_coverage(&schedule)?;
    let coupon_coverage = schedule_coverage(float_schedule)?;
    if coverage.0 > coupon_coverage.0 || coverage.1 < coupon_coverage.1 {
        return Err(invalid("resets", "must cover all actual floating accrual"));
    }
    for coupon in floating.iter_mut() {
        let (a, b) = coupon.period.accrual_dates();
        for period in schedule.periods() {
            charge(&mut scans, 1, MAX_PROFILE_VISITS, "reset joins")?;
            let (start, end) = period.accrual_dates();
            let left = a.max(start);
            let right = b.min(end);
            if left >= right {
                continue;
            }
            let observation = fixing
                .calendar
                .adjust(
                    period
                        .fixing_date()
                        .ok_or_else(|| invalid("fixing", "absent"))?,
                    t.fixing_adjustment,
                )
                .map_err(CashflowError::from)?;
            date(observation)?;
            if observation > start {
                return Err(SwapError::Unsupported {
                    feature: "in-arrears term reset",
                    reason: "requires explicit convexity/model treatment",
                });
            }
            let rate_end = quotation_end(t, reset.calendar, start, end)?;
            date(rate_end)?;
            if rate_end <= start {
                return Err(invalid("rate_period", "nonpositive interval"));
            }
            charge(&mut prepared, 1, MAX_SWAP_TERMS, "swap reset slices")?;
            coupon.resets.push(ResetSlice {
                accrual_start: left,
                accrual_end: right,
                observation_date: observation,
                rate_start: start,
                rate_end,
                projection_day_count: t.projection_day_count,
                tenor_months: t.tenor_months,
            });
        }
        let covered: i64 = coupon
            .resets
            .iter()
            .map(|s| s.accrual_start.days_between(&s.accrual_end))
            .sum();
        if covered != a.days_between(&b) {
            return Err(invalid("resets", "incomplete coupon coverage"));
        }
    }
    Ok((schedule, scans))
}
fn prepare_overnight(
    t: &crate::OvernightLegTerms,
    calendars: &SwapCalendars<'_>,
    floating: &mut [FloatingCouponPlan],
) -> Result<(), SwapError> {
    let mut prepared = 0;
    if calendars.reset.is_some() || calendars.fixing.is_some() {
        return Err(invalid(
            "reset/fixing_calendar",
            "irrelevant to overnight leg",
        ));
    }
    let obs = binding(
        calendars.observation,
        &t.observation_calendar,
        "observation_calendar",
    )?;
    for coupon in floating.iter_mut() {
        let (a, b) = coupon.period.accrual_dates();
        for profile in [&t.leg.notionals, &t.profiles.gearings] {
            if profile
                .windows(2)
                .any(|p| p[1].start > a && p[1].start < b && p[0].value != p[1].value)
            {
                return Err(SwapError::Unsupported {
                    feature: "intracoupon overnight notional/gearing step",
                    reason: "requires a separately defined segmented compounding contract",
                });
            }
        }
        let overnight =
            OvernightSchedule::generate(a, b, t.leg.day_count, obs.calendar, t.convention)?;
        charge(
            &mut prepared,
            overnight.observations().len(),
            MAX_SWAP_TERMS,
            "swap overnight observations",
        )?;
        coupon.overnight = Some(overnight);
    }
    Ok(())
}
fn profile_budget(
    terms: &SwapTerms,
    fixed_schedule: &CouponSchedule,
    floating: &[FloatingCouponPlan],
    mut scans: usize,
) -> Result<(), SwapError> {
    // Each amount call scans four independently dated profiles. Include
    // reset joins and full fixed-coupon admission plus subsequent cash work.
    let fixed_entries = terms.fixed.rates.len() + terms.fixed.leg.notionals.len() + 2;
    let float_entries = terms.floating.profiles().spreads.len()
        + terms.floating.profiles().gearings.len()
        + terms.floating.leg().notionals.len()
        + 1;
    let fixed_work = fixed_schedule
        .periods()
        .len()
        .checked_mul(fixed_entries)
        .and_then(|n| n.checked_mul(2))
        .ok_or(CashflowError::BoundExceeded("profile visits"))?;
    let float_calls = floating
        .iter()
        .map(|p| p.resets.len().max(1))
        .sum::<usize>();
    let float_work = float_calls
        .checked_mul(float_entries)
        .ok_or(CashflowError::BoundExceeded("profile visits"))?;
    charge(&mut scans, fixed_work, MAX_PROFILE_VISITS, "profile visits")?;
    charge(&mut scans, float_work, MAX_PROFILE_VISITS, "profile visits")?;
    if matches!(terms.floating, FloatingLegTerms::Overnight(_)) {
        // Admission additionally scans principal/gearing changes per coupon.
        let entries =
            terms.floating.leg().notionals.len() + terms.floating.profiles().gearings.len();
        let work = floating
            .len()
            .checked_mul(entries)
            .ok_or(CashflowError::BoundExceeded("profile visits"))?;
        charge(&mut scans, work, MAX_PROFILE_VISITS, "profile visits")?;
    }
    Ok(())
}

fn quotation_end(
    t: &TermResetTerms,
    calendar: &dyn convex_core::calendars::Calendar,
    start: Date,
    reset_end: Date,
) -> Result<Date, SwapError> {
    match t.rate_period {
        TermRatePeriod::ResetInterval => Ok(reset_end),
        TermRatePeriod::IndexTenor => {
            let mut end = start
                .add_months(t.tenor_months as i32)
                .map_err(CashflowError::from)?;
            date(end)?;
            if t.index_end_of_month && start.is_end_of_month() {
                end = Date::from_ymd(end.year(), end.month(), end.days_in_month())
                    .map_err(CashflowError::from)?;
            }
            Ok(calendar
                .adjust(end, t.index_end_adjustment)
                .map_err(CashflowError::from)?)
        }
    }
}

fn binding<'a>(
    actual: Option<CalendarBinding<'a>>,
    expected: &ReferencePin,
    field: &'static str,
) -> Result<CalendarBinding<'a>, SwapError> {
    let actual = actual.ok_or_else(|| invalid(field, "missing actual calendar"))?;
    if actual.pin != expected {
        return Err(invalid(field, "exact calendar identity/version mismatch"));
    }
    Ok(actual)
}

fn leg_schedule(
    leg: &LegTerms,
    calendars: LegCalendars<'_>,
    terms: &SwapTerms,
) -> Result<CouponSchedule, SwapError> {
    binding(
        Some(calendars.accrual),
        &leg.calendars.accrual,
        "accrual_calendar",
    )?;
    binding(
        Some(calendars.payment),
        &leg.calendars.payment,
        "payment_calendar",
    )?;
    if leg.schedule.frequency.is_zero()
        || leg.schedule.start_date != terms.effective_date
        || leg.schedule.end_date != terms.maturity_date
    {
        return Err(invalid(
            "leg_schedule",
            "nonzero coupon frequency and common unadjusted effective/maturity required",
        ));
    }
    if leg.dates.fixing_offset.is_some()
        || leg.dates.observation_offset.is_some()
        || leg.dates.ex_coupon.is_some()
    {
        return Err(invalid(
            "leg_dates",
            "reset/observation terms are independent; swap has no ex-coupon rule",
        ));
    }
    leg.rounding.apply(Decimal::ZERO)?;
    let schedule = CouponSchedule::generate_with_calendars(
        &leg.schedule,
        &leg.dates,
        &ScheduleCalendars {
            accrual: calendars.accrual.calendar,
            payment: calendars.payment.calendar,
            fixing: calendars.accrual.calendar,
            observation: calendars.accrual.calendar,
            ex_coupon: calendars.payment.calendar,
        },
    )?;
    let mut previous = None;
    for period in schedule.periods() {
        if period.payment_date() < period.accrual_dates().1
            || previous.is_some_and(|p| period.payment_date() < p)
        {
            return Err(invalid(
                "payment",
                "must be on/after actual accrual end and nondecreasing",
            ));
        }
        let fraction = period.year_fraction(
            period.accrual_dates().0,
            period.accrual_dates().1,
            leg.day_count,
        )?;
        if fraction <= Decimal::ZERO {
            return Err(invalid("day_count", "nonpositive coupon fraction"));
        }
        previous = Some(period.payment_date());
    }
    Ok(schedule)
}
fn schedule_coverage(s: &CouponSchedule) -> Result<(Date, Date), SwapError> {
    Ok((
        s.periods()
            .first()
            .ok_or_else(|| invalid("schedule", "empty"))?
            .accrual_dates()
            .0,
        s.periods()
            .last()
            .ok_or_else(|| invalid("schedule", "empty"))?
            .accrual_dates()
            .1,
    ))
}
fn constant(coverage: (Date, Date), value: Decimal) -> Vec<DatedValue> {
    vec![DatedValue {
        start: coverage.0,
        end: coverage.1,
        value,
    }]
}
fn profiles(
    leg: &LegTerms,
    s: &CouponSchedule,
    rates: Vec<DatedValue>,
    spreads: Vec<DatedValue>,
    gearings: Vec<DatedValue>,
) -> Result<CouponProfiles, SwapError> {
    let p = CouponProfiles::new(
        DatedProfile::new(ProfileKind::Coupon, rates)?,
        DatedProfile::new(ProfileKind::Spread, spreads)?,
        DatedProfile::new(ProfileKind::Gearing, gearings)?,
        DatedProfile::new(ProfileKind::Notional, leg.notionals.clone())?,
    )?;
    if p.notional().coverage() != schedule_coverage(s)?
        || p.notional().entries()[0].value <= Decimal::ZERO
    {
        return Err(invalid(
            "profiles",
            "coverage must equal actual accrual dates with positive initial notional",
        ));
    }
    Ok(p)
}
fn fixed_profiles(t: &FixedLegTerms, s: &CouponSchedule) -> Result<CouponProfiles, SwapError> {
    profiles(
        &t.leg,
        s,
        t.rates.clone(),
        constant(schedule_coverage(s)?, Decimal::ZERO),
        constant(schedule_coverage(s)?, Decimal::ZERO),
    )
}
fn floating_profiles(
    t: &FloatingLegTerms,
    s: &CouponSchedule,
) -> Result<CouponProfiles, SwapError> {
    profiles(
        t.leg(),
        s,
        constant(schedule_coverage(s)?, Decimal::ZERO),
        t.profiles().spreads.clone(),
        t.profiles().gearings.clone(),
    )
}
fn validate_reset_terms(t: &TermResetTerms, swap: &SwapTerms) -> Result<(), SwapError> {
    let offset = match t.fixing_offset {
        DateOffset::BusinessDays(n) | DateOffset::CalendarDays(n) => n,
    };
    if !(1..=120).contains(&t.tenor_months)
        || t.schedule.frequency.is_zero()
        || t.schedule.start_date != swap.effective_date
        || t.schedule.end_date != swap.maturity_date
        || offset > 0
    {
        return Err(invalid(
            "resets",
            "bounded tenor, full independent reset grid and nonpositive fixing offset required",
        ));
    }
    if matches!(
        t.projection_day_count,
        DayCountConvention::ActActIcma | DayCountConvention::Thirty360EIsda
    ) {
        return Err(SwapError::Unsupported {
            feature: "contextual term projection day count",
            reason: "requires explicit independent index reference/termination quotation",
        });
    }
    if t.rate_period == TermRatePeriod::ResetInterval
        && (t.index_end_of_month || t.index_end_adjustment != BusinessDayConvention::Unadjusted)
    {
        return Err(invalid(
            "rate_period",
            "IndexTenor-only rules must not be supplied for ResetInterval",
        ));
    }
    Ok(())
}
pub(crate) fn calculation(leg: &LegTerms, index_rate: Decimal) -> CouponCalculation {
    CouponCalculation {
        index_rate,
        day_count: leg.day_count,
        principal: CouponPrincipal::AccrualProfile,
        rounding: leg.rounding,
        negative_coupons: leg.negative_coupons,
    }
}
fn charge(
    total: &mut usize,
    additional: usize,
    max: usize,
    label: &'static str,
) -> Result<(), SwapError> {
    *total = total
        .checked_add(additional)
        .ok_or(CashflowError::BoundExceeded(label))?;
    if *total > max {
        return Err(CashflowError::BoundExceeded(label).into());
    }
    Ok(())
}
