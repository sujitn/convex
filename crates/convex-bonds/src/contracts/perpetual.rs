//! Perpetual bonds: redemption with no maturity, issuer calls at a fixed interval, and
//! coupons that may reset to a benchmark plus a margin.
//!
//! A perpetual's schedule ends on its first call. After it, coupon periods, calls and
//! resets recur at fixed intervals for ever. A perpetual never builds a [`BondContract`],
//! so no reader of a contract's schedule end takes the first call for a maturity. Its cash
//! up to any coupon date comes from [`PerpetualContract::dated_through`].

use convex_core::{
    calendars::{BusinessDayConvention, Calendar},
    cashflows::{
        CashflowError, CouponPeriod, DateOffset, DatedValue, StubType, MAX_SCHEDULE_PERIODS,
    },
    contracts::IndexId,
    daycounts::DayCountConvention,
    Date,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::{
    invalid, unsupported, BondCalendars, BondContract, BondContractError, BondCoupon, BondProfiles,
    BondTerms, ExerciseAccrued, ExerciseRight, ExerciseStyle, ExerciseTerms,
};

/// How a bond's principal is redeemed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BondRedemption {
    /// On the schedule's end date, its maturity.
    #[default]
    Dated,
    /// Never: the issuer may call it on its first call and at a fixed interval after.
    Perpetual(PerpetualTerms),
}

impl BondRedemption {
    /// Whether the bond matures on its schedule's end date.
    pub fn is_dated(&self) -> bool {
        matches!(self, Self::Dated)
    }
}

// As for `BondCoupon::Fixed`: an internally tagged unit variant would ignore extra
// fields, so `Dated` decodes as an empty struct.
impl<'de> Deserialize<'de> for BondRedemption {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum CheckedRedemption {
            Dated {},
            Perpetual(PerpetualTerms),
        }
        Ok(match CheckedRedemption::deserialize(deserializer)? {
            CheckedRedemption::Dated {} => Self::Dated,
            CheckedRedemption::Perpetual(terms) => Self::Perpetual(terms),
        })
    }
}

/// A perpetual's calls and coupon resets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PerpetualTerms {
    /// The issuer's call on the first call date and at every interval after it.
    pub call: PerpetualCall,
    /// The coupon's resets to a benchmark plus a margin; absent for a fixed coupon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<CouponReset>,
}

/// An issuer call recurring from the first call date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PerpetualCall {
    /// Months between call dates, a whole number of coupon periods.
    pub interval_months: u32,
    /// Per 100 outstanding principal.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::decimal_serde::schema")
    )]
    pub price_per_100: Decimal,
    /// Nonpositive notice offset from each call date.
    pub notice_offset: DateOffset,
    /// Nonnegative cash settlement lag after a call date.
    pub settlement_lag: DateOffset,
    /// Cash settlement business-day rule.
    pub settlement_adjustment: BusinessDayConvention,
    /// Treatment of the coupon accrued at redemption.
    pub accrued: ExerciseAccrued,
}

/// A coupon that resets to a benchmark rate plus a fixed margin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CouponReset {
    /// The benchmark the coupon resets to, such as a five-year swap rate.
    pub benchmark_id: IndexId,
    /// Annualized decimal margin over the benchmark.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::decimal_serde::schema")
    )]
    pub margin: Decimal,
    /// The first reset: a coupon date on or after the first call.
    pub first_reset: Date,
    /// Months between resets, a whole number of coupon periods.
    pub interval_months: u32,
    /// Signed offset from a reset date to its fixing, on the reset calendar.
    pub fixing_offset: DateOffset,
}

/// One reset's date and the date its benchmark is fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetDates {
    /// The coupon date the new rate applies from.
    pub reset_date: Date,
    /// The benchmark's observation date.
    pub fixing_date: Date,
}

/// A checked perpetual.
#[derive(Debug, Clone)]
pub struct PerpetualContract {
    terms: BondTerms,
    perpetual: PerpetualTerms,
}

const MARGIN_LIMIT: Decimal = Decimal::ONE;

/// The terms as the dated bond redeemed on their schedule's end.
fn as_dated(terms: &BondTerms) -> BondTerms {
    BondTerms {
        schema_version: 1,
        redemption: BondRedemption::Dated,
        ..terms.clone()
    }
}

impl PerpetualContract {
    pub(super) fn new(
        terms: BondTerms,
        calendars: &BondCalendars<'_>,
    ) -> Result<Self, BondContractError> {
        let BondRedemption::Perpetual(perpetual) = terms.redemption.clone() else {
            return Err(invalid("redemption", "a perpetual's terms required"));
        };
        if terms.schema_version != 2 {
            return Err(invalid("schema_version", "a perpetual needs version 2"));
        }
        // Each period after the tail start counts 1/f year, as a regular 30/360 period
        // does; these two are also the 30/360 counts a dated yield reads.
        if !matches!(
            terms.day_count,
            DayCountConvention::Thirty360US | DayCountConvention::Thirty360E
        ) {
            return Err(unsupported(
                "perpetual day count",
                "30/360 US or 30E/360, whose regular periods each count 1/f year",
            ));
        }
        if !matches!(terms.coupon, BondCoupon::Fixed) {
            return Err(unsupported(
                "floating perpetual",
                "a perpetual's coupon is fixed, or resets through its reset terms",
            ));
        }
        let profiles = &terms.profiles;
        if [
            &profiles.coupon,
            &profiles.spread,
            &profiles.gearing,
            &profiles.notional,
        ]
        .iter()
        .any(|profile| profile.len() != 1)
        {
            return Err(unsupported(
                "perpetual step",
                "a perpetual's coupon and principal hold to its first call; step-ups and amortization are refused",
            ));
        }
        if profiles.spread[0].value != Decimal::ZERO || profiles.coupon[0].value <= Decimal::ZERO {
            return Err(invalid(
                "profiles",
                "a perpetual's coupon is a positive fixed rate with no spread",
            ));
        }
        if !terms.exercise.is_empty()
            || !terms.principal_prices.is_empty()
            || terms.redemption_multiplier != Decimal::ONE
        {
            return Err(invalid(
                "redemption",
                "a perpetual's calls are its perpetual terms; it has no other exercise, principal price or redemption",
            ));
        }
        let schedule = &terms.schedule;
        // Rolled by whole months from the first call, a coupon date never clamps to a
        // shorter month, so the grid is the one a schedule generated back from any later
        // coupon date has.
        let first_call = schedule.end_date;
        let is_regular_roll = if schedule.end_of_month {
            first_call.is_end_of_month() || first_call.day() <= 27
        } else {
            first_call.day() <= 28
        };
        if schedule.penultimate_date.is_some()
            || matches!(schedule.stub_type, StubType::ShortLast | StubType::LongLast)
            || !is_regular_roll
        {
            return Err(invalid(
                "schedule",
                "a perpetual's schedule is regular from its first call, on a day every month has, or month end",
            ));
        }
        let period = schedule.frequency.months_per_period();
        let is_whole_periods = |months: u32| months > 0 && months.is_multiple_of(period);
        if !is_whole_periods(perpetual.call.interval_months) {
            return Err(invalid(
                "call.interval_months",
                "a whole number of coupon periods",
            ));
        }
        if let Some(reset) = &perpetual.reset {
            if !is_whole_periods(reset.interval_months) || reset.margin.abs() > MARGIN_LIMIT {
                return Err(invalid(
                    "reset",
                    "a whole number of coupon periods, and a margin within ±1",
                ));
            }
        }
        // The schedule to the first call is checked as the dated bond redeemed on it.
        let stated = BondContract::new(as_dated(&terms), calendars)?;
        let contract = Self { terms, perpetual };
        contract.call_terms(vec![first_call]).validate(
            contract.terms.issue_date,
            first_call,
            calendars.exercise,
        )?;
        if let Some(reset) = &contract.perpetual.reset {
            if reset.first_reset < first_call
                || contract.coupon_date_on_or_after(reset.first_reset)? != reset.first_reset
            {
                return Err(invalid(
                    "reset.first_reset",
                    "a coupon date on or after the first call",
                ));
            }
            reset
                .fixing_offset
                .apply(reset.first_reset, calendars.reset)?;
        }
        // A schedule generated back from the next coupon date must hold the stated one.
        // Its resets set the benchmark that keeps the fixed coupon.
        let next = contract.regular_date(1)?;
        let keeps_fixed = contract.fixed_rate()
            - contract
                .perpetual
                .reset
                .as_ref()
                .map_or(Decimal::ZERO, |r| r.margin);
        let rates = vec![keeps_fixed; contract.reset_dates(next)?.len()];
        let continued = contract.dated_through(next, first_call, &rates, calendars)?;
        let dates = |period: &CouponPeriod| {
            (
                period.contractual_dates(),
                period.accrual_dates(),
                period.payment_date(),
                period.ex_coupon(),
            )
        };
        if !continued
            .periods()
            .take(stated.periods().count())
            .map(dates)
            .eq(stated.periods().map(dates))
        {
            return Err(invalid(
                "schedule",
                "the schedule does not continue regularly after the first call",
            ));
        }
        Ok(contract)
    }

    /// The full terms as supplied.
    pub fn terms(&self) -> &BondTerms {
        &self.terms
    }

    /// Its calls and resets.
    pub fn perpetual(&self) -> &PerpetualTerms {
        &self.perpetual
    }

    /// The first call date, where the stated schedule ends.
    pub fn first_call(&self) -> Date {
        self.terms.schedule.end_date
    }

    /// The fixed coupon rate: in force to the first reset, or for ever.
    pub fn fixed_rate(&self) -> Decimal {
        self.terms.profiles.coupon[0].value
    }

    fn period_months(&self) -> u32 {
        self.terms.schedule.frequency.months_per_period()
    }

    fn call_step(&self) -> u32 {
        self.perpetual.call.interval_months / self.period_months()
    }

    /// The regular coupon date `periods` periods after the first call.
    fn regular_date(&self, periods: u32) -> Result<Date, BondContractError> {
        let months = periods
            .checked_mul(self.period_months())
            .and_then(|m| i32::try_from(m).ok())
            .ok_or(CashflowError::BoundExceeded("perpetual coupon dates"))?;
        let first_call = self.first_call();
        let date = first_call
            .add_months(months)
            .map_err(|e| invalid("perpetual date", e.to_string()))?;
        if self.terms.schedule.end_of_month && first_call.is_end_of_month() {
            return Ok(date.end_of_month());
        }
        Ok(date)
    }

    /// Regular coupon periods from the first call to the first coupon date on or after
    /// `date`.
    fn periods_to(&self, date: Date) -> Result<u32, BondContractError> {
        let first_call = self.first_call();
        if date <= first_call {
            return Ok(0);
        }
        let months = (date.year() - first_call.year()) * 12 + date.month() as i32
            - first_call.month() as i32;
        let mut periods = u32::try_from(months)
            .map_err(|_| invalid("perpetual date", "before the first call"))?
            / self.period_months();
        while self.regular_date(periods)? < date {
            periods += 1;
            if periods as usize > MAX_SCHEDULE_PERIODS {
                return Err(CashflowError::BoundExceeded("perpetual coupon dates").into());
            }
        }
        Ok(periods)
    }

    /// The first regular coupon date on or after `date`, the first call at the earliest.
    pub fn coupon_date_on_or_after(&self, date: Date) -> Result<Date, BondContractError> {
        self.regular_date(self.periods_to(date)?)
    }

    /// Call dates after `after`, through `through`.
    pub fn call_dates(&self, after: Date, through: Date) -> Result<Vec<Date>, BondContractError> {
        let before = through
            .checked_add_days(1)
            .map_err(|e| invalid("perpetual date", e.to_string()))?;
        let mut dates = self.recurring(0, self.call_step(), before)?;
        dates.retain(|date| *date > after);
        Ok(dates)
    }

    // Reset dates before `before`.
    fn reset_dates(&self, before: Date) -> Result<Vec<Date>, BondContractError> {
        let Some(reset) = &self.perpetual.reset else {
            return Ok(vec![]);
        };
        let step = reset.interval_months / self.period_months();
        self.recurring(self.periods_to(reset.first_reset)?, step, before)
    }

    /// Each reset before `before`, with its fixing date on the reset calendar.
    pub fn resets(
        &self,
        before: Date,
        reset_calendar: &dyn Calendar,
    ) -> Result<Vec<ResetDates>, BondContractError> {
        let Some(reset) = &self.perpetual.reset else {
            return Ok(vec![]);
        };
        self.reset_dates(before)?
            .into_iter()
            .map(|reset_date| {
                Ok(ResetDates {
                    reset_date,
                    fixing_date: reset.fixing_offset.apply(reset_date, reset_calendar)?,
                })
            })
            .collect()
    }

    /// The first reset whose benchmark is fixed after `date`, for a coupon that resets.
    pub fn first_reset_fixed_after(
        &self,
        date: Date,
        reset_calendar: &dyn Calendar,
    ) -> Result<Option<ResetDates>, BondContractError> {
        let Some(reset) = &self.perpetual.reset else {
            return Ok(None);
        };
        let step = reset.interval_months / self.period_months();
        let mut periods = self.periods_to(reset.first_reset)?;
        for _ in 0..MAX_SCHEDULE_PERIODS {
            let reset_date = self.regular_date(periods)?;
            let fixing_date = reset.fixing_offset.apply(reset_date, reset_calendar)?;
            if fixing_date > date {
                return Ok(Some(ResetDates {
                    reset_date,
                    fixing_date,
                }));
            }
            periods = periods
                .checked_add(step)
                .ok_or(CashflowError::BoundExceeded("perpetual dates"))?;
        }
        Err(CashflowError::BoundExceeded("perpetual resets").into())
    }

    // Regular coupon dates `first`, `first + step`, ... periods after the first call, before
    // `before`.
    fn recurring(
        &self,
        first: u32,
        step: u32,
        before: Date,
    ) -> Result<Vec<Date>, BondContractError> {
        let mut dates = Vec::new();
        let mut periods = first;
        loop {
            let date = self.regular_date(periods)?;
            if date >= before {
                return Ok(dates);
            }
            if dates.len() == MAX_SCHEDULE_PERIODS {
                return Err(CashflowError::BoundExceeded("perpetual dates").into());
            }
            dates.push(date);
            periods = periods
                .checked_add(step)
                .ok_or(CashflowError::BoundExceeded("perpetual dates"))?;
        }
    }

    /// The coupon rate from a reset that sets `benchmark_rate`: the rate plus the margin.
    pub fn reset_coupon_rate(&self, benchmark_rate: Decimal) -> Result<Decimal, BondContractError> {
        let reset = self
            .perpetual
            .reset
            .as_ref()
            .ok_or_else(|| invalid("reset", "the perpetual's coupon does not reset"))?;
        benchmark_rate
            .checked_add(reset.margin)
            .ok_or_else(|| CashflowError::Arithmetic("reset coupon rate").into())
    }

    /// The coupon rate in force for ever once every reset holds `held_level`: the fixed
    /// rate when the coupon does not reset, and no level is read.
    pub fn perpetual_coupon_rate(
        &self,
        held_level: Option<Decimal>,
    ) -> Result<Decimal, BondContractError> {
        match (&self.perpetual.reset, held_level) {
            (None, None) => Ok(self.fixed_rate()),
            (Some(_), Some(level)) => self.reset_coupon_rate(level),
            _ => Err(invalid(
                "held_level",
                "a level held for a coupon that resets, and only then",
            )),
        }
    }

    /// The cash one regular coupon period pays at `rate`, rounded as the contract's cash
    /// is: the face times the rate over the coupon frequency.
    pub fn regular_coupon(&self, rate: Decimal) -> Result<Decimal, BondContractError> {
        let periods = Decimal::from(self.terms.schedule.frequency.periods_per_year());
        let amount = self
            .terms
            .face_value
            .checked_mul(rate)
            .and_then(|cash| cash.checked_div(periods))
            .ok_or(CashflowError::Arithmetic("regular coupon"))?;
        Ok(self.terms.rounding.apply(amount)?)
    }

    fn call_terms(&self, dates: Vec<Date>) -> ExerciseTerms {
        let call = &self.perpetual.call;
        ExerciseTerms {
            right: ExerciseRight::Call,
            style: ExerciseStyle::Bermudan { dates },
            price_per_100: call.price_per_100,
            notice_offset: call.notice_offset,
            settlement_lag: call.settlement_lag,
            settlement_adjustment: call.settlement_adjustment,
            accrued: call.accrued,
            protection_end: None,
        }
    }

    /// The dated bond that pays as this perpetual does through `end`, a regular coupon
    /// date after the first call, and is callable on each call date before it, through
    /// `last_call`. It redeems at par on `end`, which the perpetual never does, so its
    /// maturity cash is not the perpetual's: it composes a call's cash, and the coupons up
    /// to `end`.
    ///
    /// `benchmark_rates` holds the benchmark rate each reset before `end` sets, in date
    /// order.
    ///
    /// # Errors
    ///
    /// An invalid-terms error when `end` is not a regular coupon date after the first
    /// call, or `benchmark_rates` does not hold one rate for each reset before it; the
    /// dated bond's own admission errors.
    pub fn dated_through(
        &self,
        end: Date,
        last_call: Date,
        benchmark_rates: &[Decimal],
        calendars: &BondCalendars<'_>,
    ) -> Result<BondContract, BondContractError> {
        if end <= self.first_call() || self.coupon_date_on_or_after(end)? != end {
            return Err(invalid(
                "dated_through",
                "a regular coupon date after the first call",
            ));
        }
        let resets = self.reset_dates(end)?;
        if benchmark_rates.len() != resets.len() {
            return Err(invalid(
                "benchmark_rates",
                "one rate for each reset before the end date, in order",
            ));
        }
        let start = self.terms.schedule.start_date;
        let constant = |value| vec![DatedValue { start, end, value }];
        let mut coupon = vec![DatedValue {
            start,
            end: resets.first().copied().unwrap_or(end),
            value: self.fixed_rate(),
        }];
        for (i, (reset_date, rate)) in resets.iter().zip(benchmark_rates).enumerate() {
            coupon.push(DatedValue {
                start: *reset_date,
                end: resets.get(i + 1).copied().unwrap_or(end),
                value: self.reset_coupon_rate(*rate)?,
            });
        }
        let mut terms = as_dated(&self.terms);
        terms.schedule.end_date = end;
        terms.profiles = BondProfiles {
            coupon,
            spread: constant(Decimal::ZERO),
            gearing: constant(Decimal::ZERO),
            notional: constant(self.terms.face_value),
        };
        let mut calls = self.recurring(0, self.call_step(), end)?;
        calls.retain(|call| *call <= last_call);
        terms.exercise = if calls.is_empty() {
            vec![]
        } else {
            vec![self.call_terms(calls)]
        };
        BondContract::new(terms, calendars)
    }
}
