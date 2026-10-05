//! Anchored schedules and distinct contractual, accrual and payment dates.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::{validate_date, validate_span, CashflowError, MAX_DATE_OFFSET, MAX_SCHEDULE_PERIODS};
use crate::{
    calendars::{checked, BusinessDayConvention, Calendar},
    daycounts::DayCountConvention,
    types::{Date, Frequency},
};

/// Choice of automatic irregular boundary. Explicit first/penultimate dates
/// override automatic stub length at that boundary. `None` requires an exact grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StubType {
    /// Require regular periods unless explicit boundary dates are supplied.
    #[default]
    None,
    /// Generate backward, retaining a short initial period.
    ShortFirst,
    /// Generate backward, merging the short initial period with its neighbor.
    LongFirst,
    /// Generate forward, retaining a short final period.
    ShortLast,
    /// Generate forward, merging the short final period with its neighbor.
    LongLast,
}

impl StubType {
    /// Whether automatic generation works backward from the regular end.
    pub fn is_front_stub(&self) -> bool {
        matches!(self, Self::ShortFirst | Self::LongFirst)
    }
    /// Whether automatic generation works forward from the regular start.
    pub fn is_back_stub(&self) -> bool {
        matches!(self, Self::ShortLast | Self::LongLast)
    }
}

/// Immutable input terms for a shared coupon schedule, with no calendar lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleSpec {
    /// Contractual accrual start; may differ from issue/trade date.
    pub start_date: Date,
    /// Contractual maturity or leg end.
    pub end_date: Date,
    /// Regular coupon frequency; Zero represents a single principal period.
    pub frequency: Frequency,
    /// Automatic front/back stub choice.
    pub stub_type: StubType,
    /// Preserve calendar month end when the regular anchor is month end.
    pub end_of_month: bool,
    /// Explicit end of the first coupon period, strictly inside the contract.
    pub first_regular_date: Option<Date>,
    /// Explicit start of the last coupon period, strictly inside the contract.
    pub penultimate_date: Option<Date>,
}

/// A signed offset. Business-day counting excludes the starting date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DateOffset {
    /// Exact calendar days.
    CalendarDays(i32),
    /// Supplied calendar's business days.
    BusinessDays(i32),
}

impl DateOffset {
    /// Apply a bounded offset. Use for fixing, observation, settlement or notice dates.
    pub fn apply(self, date: Date, calendar: &dyn Calendar) -> Result<Date, CashflowError> {
        validate_date(date)?;
        let days = match self {
            Self::CalendarDays(n) | Self::BusinessDays(n) => n,
        };
        if !(-MAX_DATE_OFFSET..=MAX_DATE_OFFSET).contains(&days) {
            return Err(CashflowError::BoundExceeded("date offset"));
        }
        let result = match self {
            Self::CalendarDays(n) => date.checked_add_days(i64::from(n))?,
            Self::BusinessDays(n) => checked::add_business_days(date, n, calendar)?,
        };
        validate_date(result)?;
        Ok(result)
    }
}

/// Resolve settlement from supplied lag and business-day convention, without a
/// built-in market cycle. A settlement before the trade date is invalid.
pub fn settlement_date(
    trade_date: Date,
    lag: DateOffset,
    convention: BusinessDayConvention,
    calendar: &dyn Calendar,
) -> Result<Date, CashflowError> {
    let days = match lag {
        DateOffset::CalendarDays(n) | DateOffset::BusinessDays(n) => n,
    };
    if days < 0 {
        return Err(CashflowError::terms(
            "settlement_lag",
            "must be nonnegative",
        ));
    }
    let settlement = calendar.adjust(lag.apply(trade_date, calendar)?, convention)?;
    validate_date(settlement)?;
    if settlement < trade_date {
        return Err(CashflowError::terms("settlement", "precedes trade date"));
    }
    Ok(settlement)
}

/// First date on which a settling buyer loses coupon entitlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExCouponBoundary {
    /// Settlement on the specified date is already ex-coupon.
    OnDate,
    /// Settlement on the record date is entitled; later settlement is ex-coupon.
    AfterDate,
}

/// Date from which the supplied ex-coupon offset is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExDateAnchor {
    /// Unadjusted contractual coupon date.
    ContractualEnd,
    /// Adjusted, delayed cash payment date.
    PaymentDate,
}

/// Contract-specific coupon entitlement rule; principal entitlement is separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExCouponRule {
    /// Nonpositive offset from the selected anchor.
    pub offset: DateOffset,
    /// Explicit contractual or payment anchor.
    pub anchor: ExDateAnchor,
    /// Inclusion of the boundary date.
    pub boundary: ExCouponBoundary,
}

/// Whether an unpaid cashflow on settlement date belongs to the buyer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PaymentDateEntitlement {
    /// Include cash due on settlement, for a supplied pre-payment state.
    Include,
    /// Exclude cash due on settlement, for a supplied post-payment state.
    Exclude,
}

/// Independent date transformations. None leaves fixing/observation/ex dates absent.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DateRules {
    /// Adjustment applied only to accrual boundaries.
    pub accrual_adjustment: BusinessDayConvention,
    /// Adjustment applied to payment dates after payment delay.
    pub payment_adjustment: BusinessDayConvention,
    /// Nonnegative lag from unadjusted coupon end, before payment adjustment.
    pub payment_delay: DateOffset,
    /// Offset from accrual start for term fixing dates.
    pub fixing_offset: Option<DateOffset>,
    /// Offset applied independently to each accrual boundary for observation dates.
    pub observation_offset: Option<DateOffset>,
    /// Independent coupon entitlement terms.
    pub ex_coupon: Option<ExCouponRule>,
}

impl Default for DateRules {
    fn default() -> Self {
        Self {
            accrual_adjustment: BusinessDayConvention::Unadjusted,
            payment_adjustment: BusinessDayConvention::Unadjusted,
            payment_delay: DateOffset::CalendarDays(0),
            fixing_offset: None,
            observation_offset: None,
            ex_coupon: None,
        }
    }
}

impl DateRules {
    /// Compute a principal or coupon payment date without changing the effective date.
    pub fn payment_date(
        &self,
        contractual: Date,
        calendar: &dyn Calendar,
    ) -> Result<Date, CashflowError> {
        let days = match self.payment_delay {
            DateOffset::CalendarDays(n) | DateOffset::BusinessDays(n) => n,
        };
        if days < 0 {
            return Err(CashflowError::terms("payment_delay", "must be nonnegative"));
        }
        let result = calendar.adjust(
            self.payment_delay.apply(contractual, calendar)?,
            self.payment_adjustment,
        )?;
        validate_date(result)?;
        Ok(result)
    }
}

/// One regular notional period, preserved even when a long stub merges coupons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferencePeriod {
    /// Inclusive notional start.
    pub start: Date,
    /// Exclusive notional end.
    pub end: Date,
}

/// A validated coupon period. Date adjustments do not rewrite contractual dates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouponPeriod {
    pub(crate) unadjusted_start: Date,
    pub(crate) unadjusted_end: Date,
    pub(crate) accrual_start: Date,
    pub(crate) accrual_end: Date,
    pub(crate) payment_date: Date,
    fixing_date: Option<Date>,
    observation_dates: Option<(Date, Date)>,
    ex_coupon: Option<(Date, ExCouponBoundary)>,
    reference_periods: Vec<ReferencePeriod>,
    frequency: Frequency,
    termination_date: Date,
}

impl CouponPeriod {
    /// Contractual unadjusted boundaries.
    pub fn contractual_dates(&self) -> (Date, Date) {
        (self.unadjusted_start, self.unadjusted_end)
    }
    /// Actual contractual accrual boundaries after the explicit accrual rule.
    pub fn accrual_dates(&self) -> (Date, Date) {
        (self.accrual_start, self.accrual_end)
    }
    /// Adjusted and delayed payment date.
    pub fn payment_date(&self) -> Date {
        self.payment_date
    }
    /// Independently generated term fixing date, if requested.
    pub fn fixing_date(&self) -> Option<Date> {
        self.fixing_date
    }
    /// Independently shifted observation boundaries, if requested.
    pub fn observation_dates(&self) -> Option<(Date, Date)> {
        self.observation_dates
    }
    /// Ex-coupon date and its inclusion rule, if supplied.
    pub fn ex_coupon(&self) -> Option<(Date, ExCouponBoundary)> {
        self.ex_coupon
    }
    /// Regular notional periods covering the accrual period, including stubs.
    pub fn reference_periods(&self) -> &[ReferencePeriod] {
        &self.reference_periods
    }
    /// Whether the buyer has lost coupon entitlement at the specified settlement.
    pub fn is_ex_coupon(&self, settlement: Date) -> bool {
        match self.ex_coupon {
            Some((date, ExCouponBoundary::OnDate)) => settlement >= date,
            Some((date, ExCouponBoundary::AfterDate)) => settlement > date,
            None => false,
        }
    }
    /// Coupon entitlement, with payment-date state supplied explicitly.
    pub fn is_entitled(&self, settlement: Date, on_payment: PaymentDateEntitlement) -> bool {
        (settlement < self.payment_date
            || (settlement == self.payment_date && on_payment == PaymentDateEntitlement::Include))
            && !self.is_ex_coupon(settlement)
    }
    /// Checked year fraction for any subinterval of this period. ICMA splits
    /// long stubs at regular notional boundaries instead of using one denominator.
    pub fn year_fraction(
        &self,
        start: Date,
        end: Date,
        day_count: DayCountConvention,
    ) -> Result<Decimal, CashflowError> {
        if start < self.accrual_start || end > self.accrual_end || end < start {
            return Err(CashflowError::terms(
                "accrual",
                "subinterval outside coupon period",
            ));
        }
        if start == end {
            return Ok(Decimal::ZERO);
        }
        if day_count == DayCountConvention::Thirty360EIsda {
            return Ok(
                crate::daycounts::Thirty360EIsda::new(Some(self.termination_date))
                    .checked_year_fraction(start, end)?,
            );
        }
        if day_count != DayCountConvention::ActActIcma {
            return Ok(day_count.to_day_count().year_fraction(start, end));
        }
        if self.frequency.is_zero() {
            return Err(CashflowError::terms(
                "day_count",
                "ICMA requires a coupon frequency",
            ));
        }
        let mut total = Decimal::ZERO;
        let mut covered = 0_i64;
        for reference in &self.reference_periods {
            let left = start.max(reference.start);
            let right = end.min(reference.end);
            if right > left {
                let days = left.days_between(&right);
                covered += days;
                let denominator = Decimal::from(self.frequency.periods_per_year())
                    .checked_mul(Decimal::from(reference.start.days_between(&reference.end)))
                    .ok_or(CashflowError::Arithmetic("ICMA denominator"))?;
                let fraction = Decimal::from(days)
                    .checked_div(denominator)
                    .ok_or(CashflowError::Arithmetic("ICMA division"))?;
                total = total
                    .checked_add(fraction)
                    .ok_or(CashflowError::Arithmetic("ICMA sum"))?;
            }
        }
        if covered != start.days_between(&end) {
            return Err(CashflowError::terms(
                "reference_periods",
                "incomplete ICMA coverage",
            ));
        }
        Ok(total)
    }
}

/// A reusable validated schedule for bonds, rates and credit legs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouponSchedule {
    contractual_dates: Vec<Date>,
    periods: Vec<CouponPeriod>,
}

impl CouponSchedule {
    /// Generate an anchored calendar-month grid and independent date schedules.
    /// Custom Calendar implementations must terminate without panicking.
    pub fn generate(
        spec: &ScheduleSpec,
        rules: &DateRules,
        calendar: &dyn Calendar,
    ) -> Result<Self, CashflowError> {
        validate_span(spec.start_date, spec.end_date)?;
        let (contractual_dates, anchor) = generate_dates(spec)?;
        let mut periods = Vec::with_capacity(contractual_dates.len().saturating_sub(1));
        for pair in contractual_dates.windows(2) {
            let accrual_start = calendar.adjust(pair[0], rules.accrual_adjustment)?;
            let accrual_end = calendar.adjust(pair[1], rules.accrual_adjustment)?;
            validate_span(accrual_start, accrual_end)?;
            let payment_date = rules.payment_date(pair[1], calendar)?;
            if payment_date < accrual_start {
                return Err(CashflowError::terms(
                    "payment_date",
                    "precedes accrual start",
                ));
            }
            let fixing_date = rules
                .fixing_offset
                .map(|offset| offset.apply(accrual_start, calendar))
                .transpose()?;
            let observation_dates = rules
                .observation_offset
                .map(|offset| {
                    let start = offset.apply(accrual_start, calendar)?;
                    let end = offset.apply(accrual_end, calendar)?;
                    validate_span(start, end)?;
                    Ok::<_, CashflowError>((start, end))
                })
                .transpose()?;
            let ex_coupon = rules
                .ex_coupon
                .map(|rule| {
                    let n = match rule.offset {
                        DateOffset::CalendarDays(n) | DateOffset::BusinessDays(n) => n,
                    };
                    if n > 0 {
                        return Err(CashflowError::terms(
                            "ex_coupon",
                            "offset must be nonpositive",
                        ));
                    }
                    let anchor_date = match rule.anchor {
                        ExDateAnchor::ContractualEnd => pair[1],
                        ExDateAnchor::PaymentDate => payment_date,
                    };
                    let ex_date = rule.offset.apply(anchor_date, calendar)?;
                    if ex_date > payment_date {
                        return Err(CashflowError::terms("ex_coupon", "date after payment"));
                    }
                    Ok::<_, CashflowError>((ex_date, rule.boundary))
                })
                .transpose()?;
            let reference_periods = if spec.frequency.is_zero() {
                Vec::new()
            } else {
                references(
                    accrual_start,
                    accrual_end,
                    anchor,
                    spec.frequency.months_per_period() as i32,
                    spec.end_of_month,
                )?
            };
            if periods
                .last()
                .is_some_and(|p: &CouponPeriod| p.payment_date >= payment_date)
            {
                return Err(CashflowError::terms(
                    "payment_dates",
                    "must increase strictly",
                ));
            }
            periods.push(CouponPeriod {
                unadjusted_start: pair[0],
                unadjusted_end: pair[1],
                accrual_start,
                accrual_end,
                payment_date,
                fixing_date,
                observation_dates,
                ex_coupon,
                reference_periods,
                frequency: spec.frequency,
                termination_date: calendar.adjust(spec.end_date, rules.accrual_adjustment)?,
            });
        }
        Ok(Self {
            contractual_dates,
            periods,
        })
    }
    /// Contractual grid before any business-day transformation.
    pub fn contractual_dates(&self) -> &[Date] {
        &self.contractual_dates
    }
    /// Validated coupon periods.
    pub fn periods(&self) -> &[CouponPeriod] {
        &self.periods
    }
}

fn roll(anchor: Date, count: i32, months: i32, eom: bool) -> Result<Date, CashflowError> {
    let offset = count
        .checked_mul(months)
        .ok_or(CashflowError::BoundExceeded("month roll"))?;
    let date = anchor.add_months(offset)?;
    let date = if eom && anchor.is_end_of_month() {
        date.end_of_month()
    } else {
        date
    };
    validate_date(date)?;
    Ok(date)
}

fn generate_dates(spec: &ScheduleSpec) -> Result<(Vec<Date>, Date), CashflowError> {
    for date in [spec.first_regular_date, spec.penultimate_date]
        .into_iter()
        .flatten()
    {
        validate_date(date)?;
        if date <= spec.start_date || date >= spec.end_date {
            return Err(CashflowError::terms(
                "stub_dates",
                "must lie strictly inside contract",
            ));
        }
    }
    if spec
        .first_regular_date
        .zip(spec.penultimate_date)
        .is_some_and(|(a, b)| a > b)
    {
        return Err(CashflowError::terms(
            "stub_dates",
            "first date follows penultimate",
        ));
    }
    if spec.frequency.is_zero() {
        if spec.first_regular_date.is_some()
            || spec.penultimate_date.is_some()
            || spec.stub_type != StubType::None
        {
            return Err(CashflowError::terms(
                "frequency",
                "zero coupon cannot have stub terms",
            ));
        }
        return Ok((vec![spec.start_date, spec.end_date], spec.end_date));
    }
    let months = spec.frequency.months_per_period() as i32;
    let left = spec.first_regular_date.unwrap_or(spec.start_date);
    let right = spec.penultimate_date.unwrap_or(spec.end_date);
    let forward = spec.first_regular_date.is_some() || spec.stub_type.is_back_stub();
    let anchor = if forward { left } else { right };
    let mut dates = vec![anchor];
    let mut exact = left == right;
    for step in 1..=MAX_SCHEDULE_PERIODS {
        if exact {
            break;
        }
        let count =
            i32::try_from(step).map_err(|_| CashflowError::BoundExceeded("schedule steps"))?;
        let date = roll(
            anchor,
            if forward { count } else { -count },
            months,
            spec.end_of_month,
        )?;
        let boundary = if forward { right } else { left };
        if date == boundary {
            dates.push(date);
            exact = true;
            break;
        }
        if (forward && date > boundary) || (!forward && date < boundary) {
            break;
        }
        dates.push(date);
        if step == MAX_SCHEDULE_PERIODS {
            return Err(CashflowError::BoundExceeded("schedule periods"));
        }
    }
    if !exact {
        let allowed = if forward {
            spec.stub_type.is_back_stub() && spec.penultimate_date.is_none()
        } else {
            spec.stub_type.is_front_stub() && spec.first_regular_date.is_none()
        };
        if !allowed {
            return Err(CashflowError::terms(
                "stub_type",
                "irregular boundary requires explicit stub terms",
            ));
        }
        if matches!(spec.stub_type, StubType::LongFirst | StubType::LongLast) {
            if dates.len() < 2 {
                return Err(CashflowError::terms(
                    "stub_type",
                    "long stub requires a neighboring regular period",
                ));
            }
            dates.pop();
        }
        dates.push(if forward { right } else { left });
    }
    if !forward {
        dates.reverse();
    }
    if dates.first() != Some(&spec.start_date) {
        dates.insert(0, spec.start_date);
    }
    if dates.last() != Some(&spec.end_date) {
        dates.push(spec.end_date);
    }
    if dates.len() > MAX_SCHEDULE_PERIODS + 1 {
        return Err(CashflowError::BoundExceeded("schedule periods"));
    }
    if dates.windows(2).any(|p| p[0] >= p[1]) {
        return Err(CashflowError::terms(
            "schedule",
            "dates do not increase strictly",
        ));
    }
    Ok((dates, anchor))
}

fn references(
    start: Date,
    end: Date,
    anchor: Date,
    months: i32,
    eom: bool,
) -> Result<Vec<ReferencePeriod>, CashflowError> {
    let difference =
        (start.year() - anchor.year()) * 12 + start.month() as i32 - anchor.month() as i32;
    let mut count = difference.div_euclid(months);
    let mut left = roll(anchor, count, months, eom)?;
    if left > start {
        count -= 1;
        left = roll(anchor, count, months, eom)?;
    }
    let mut periods = Vec::new();
    for _ in 0..MAX_SCHEDULE_PERIODS {
        count += 1;
        let right = roll(anchor, count, months, eom)?;
        periods.push(ReferencePeriod {
            start: left,
            end: right,
        });
        if right >= end {
            return Ok(periods);
        }
        left = right;
    }
    Err(CashflowError::BoundExceeded("reference periods"))
}
