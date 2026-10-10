//! Reuse the shared anchored schedule, then apply explicit CDS date conventions.
use crate::{checked_date, invalid, CdsCalendars, CdsError, CdsMaturityRule, CdsTerms};
use convex_core::{
    calendars::checked,
    cashflows::{CouponSchedule, DateRules, MAX_CONTRACT_DAYS},
    daycounts::DayCountConvention,
    types::Frequency,
    Date,
};
use rust_decimal::Decimal;
use serde::Serialize;

/// One premium conditional on survival. It is never a realized cash payment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PremiumPeriod {
    /// Unadjusted contractual coupon boundary.
    pub contractual_start: Date,
    /// Unadjusted contractual coupon boundary/maturity.
    pub contractual_end: Date,
    /// Actual inclusive accrual start; the first supplied start is unadjusted.
    pub accrual_start: Date,
    /// Exclusive accrual end. Final premium accrues THROUGH unadjusted maturity.
    pub accrual_end: Date,
    /// Following-adjusted cash date; can precede final exclusive accrual end.
    pub payment_date: Date,
}
/// Private checked state cannot bypass construction via JSON deserialization.
#[derive(Debug, Clone)]
pub struct CdsContract {
    terms: CdsTerms,
    periods: Vec<PremiumPeriod>,
}
impl CdsTerms {
    /// Build pure economics from actual supplied, exact-version calendars.
    pub fn build(&self, calendars: &CdsCalendars<'_>) -> Result<CdsContract, CdsError> {
        for d in [
            self.trade_date,
            self.step_in_date,
            self.protection_start_date,
            self.cash_settlement_date,
        ] {
            checked_date(d)?;
        }
        if calendars.premium.pin != &self.premium_calendar
            || calendars.settlement.pin != &self.settlement_calendar
        {
            return Err(invalid(
                "calendars",
                "exact supplied resource versions differ from terms",
            ));
        }
        self.discount_curve.validate()?;
        self.survival_curve.validate()?;
        let n = self.notional.decimal();
        let c = self.coupon_decimal.decimal();
        if n <= Decimal::ZERO || n > Decimal::from(1_000_000_000_000_000_000_u64) {
            return Err(invalid("notional", "must be positive and <= 1e18"));
        }
        if c < Decimal::ZERO || c > Decimal::ONE {
            return Err(invalid("coupon_decimal", "must be in [0,1]"));
        }
        if !(0.0..=1.0).contains(&self.reference.recovery_fraction.get()) {
            return Err(invalid("recovery_fraction", "must be in [0,1]"));
        }
        if self
            .initial_clean_upfront_fraction
            .is_some_and(|u| u.decimal().abs() > Decimal::from(10))
        {
            return Err(invalid(
                "initial_clean_upfront_fraction",
                "magnitude exceeds 10",
            ));
        }
        if self.step_in_date
            != self
                .trade_date
                .checked_add_days(1)
                .map_err(convex_core::cashflows::CashflowError::from)?
        {
            return Err(invalid(
                "step_in_date",
                "standard end-of-day trade requires T+1 calendar day",
            ));
        }
        let expected = checked::add_business_days(
            self.trade_date,
            self.settlement_lag_business_days,
            calendars.settlement.calendar,
        )
        .map_err(convex_core::cashflows::CashflowError::from)?;
        if self.settlement_lag_business_days < 0
            || self.cash_settlement_date != expected
            || !calendars.settlement.calendar.is_business_day(expected)
        {
            return Err(invalid(
                "cash_settlement_date",
                "does not match explicit nonnegative business-day lag",
            ));
        }
        let s = &self.premium_schedule;
        if s.frequency != Frequency::Quarterly || s.end_of_month {
            return Err(CdsError::Unsupported(
                "non-quarterly or EOM premium schedule",
            ));
        }
        if self.maturity_rule == CdsMaturityRule::QuarterlyTwentieth
            && (s.end_date.day() != 20 || ![3, 6, 9, 12].contains(&s.end_date.month()))
        {
            return Err(invalid(
                "maturity_rule",
                "requires an unadjusted quarterly twentieth",
            ));
        }
        if self.protection_start_date != self.step_in_date.max(s.start_date)
            || self.protection_start_date > s.end_date
        {
            return Err(invalid(
                "protection_start_date",
                "must equal max(step-in,premium start) and not exceed maturity",
            ));
        }
        if self.trade_date.days_between(&s.end_date) > MAX_CONTRACT_DAYS {
            return Err(
                convex_core::cashflows::CashflowError::BoundExceeded("credit horizon").into(),
            );
        }
        if !matches!(
            self.premium_day_count,
            DayCountConvention::Act360 | DayCountConvention::Act365Fixed
        ) {
            return Err(CdsError::Unsupported(
                "premium day count outside ACT/360 and ACT/365F",
            ));
        }
        let schedule =
            CouponSchedule::generate(s, &DateRules::default(), calendars.premium.calendar)?;
        let grid = schedule.contractual_dates();
        let mut periods = Vec::with_capacity(schedule.periods().len());
        let mut start = s.start_date;
        for (i, pair) in grid.windows(2).enumerate() {
            let payment = checked::business_day_on_or_after(pair[1], calendars.premium.calendar)
                .map_err(convex_core::cashflows::CashflowError::from)?;
            let end = if i + 1 == schedule.periods().len() {
                pair[1]
                    .checked_add_days(1)
                    .map_err(convex_core::cashflows::CashflowError::from)?
            } else {
                payment
            };
            checked_date(payment)?;
            checked_date(end)?;
            if end <= start
                || periods
                    .last()
                    .is_some_and(|p: &PremiumPeriod| p.payment_date >= payment)
            {
                return Err(invalid(
                    "premium_schedule",
                    "adjusted accrual/payment boundaries must increase",
                ));
            }
            periods.push(PremiumPeriod {
                contractual_start: pair[0],
                contractual_end: pair[1],
                accrual_start: start,
                accrual_end: end,
                payment_date: payment,
            });
            start = end;
        }
        Ok(CdsContract {
            terms: self.clone(),
            periods,
        })
    }
}
impl CdsContract {
    /// Bind an explicit hypothetical recovery assumption to this complete checked
    /// contract. Recovery changes no schedule or cash-state input; the original
    /// terms, legal reference pins and actual calendar-derived periods survive.
    /// The full pricer must still admit the exact scenario credit role and grid.
    pub fn with_recovery_fraction(
        &self,
        fraction: convex_core::contracts::FiniteF64,
    ) -> Result<Self, CdsError> {
        if !(0.0..=1.0).contains(&fraction.get()) {
            return Err(invalid("recovery_fraction", "must be in [0,1]"));
        }
        let mut scenario = self.clone();
        scenario.terms.reference.recovery_fraction = fraction;
        Ok(scenario)
    }

    /// Original complete immutable supplied economics.
    pub fn terms(&self) -> &CdsTerms {
        &self.terms
    }
    /// Actual premium/accrual/payment dates, including final maturity day.
    pub fn premium_periods(&self) -> &[PremiumPeriod] {
        &self.periods
    }
    pub(crate) fn denominator(&self) -> f64 {
        match self.terms.premium_day_count {
            DayCountConvention::Act360 => 360.0,
            _ => 365.0,
        }
    }
    pub(crate) fn accrued_days(&self, stepin: Date) -> i64 {
        self.periods
            .iter()
            .find(|p| p.accrual_start <= stepin && stepin < p.accrual_end)
            .map_or(0, |p| p.accrual_start.days_between(&stepin))
    }
}
/// Last unadjusted quarterly twentieth on or before an actual date.
pub fn cds_roll_on_or_before(date: Date) -> Result<Date, CdsError> {
    checked_date(date)?;
    let month = ((date.month() - 1) / 3 + 1) * 3;
    let candidate = Date::from_ymd(date.year(), month, 20)
        .map_err(convex_core::cashflows::CashflowError::from)?;
    let result = if candidate <= date {
        candidate
    } else {
        candidate
            .add_months(-3)
            .map_err(convex_core::cashflows::CashflowError::from)?
    };
    checked_date(result)?;
    Ok(result)
}
/// First unadjusted quarterly twentieth strictly after an actual date.
pub fn cds_roll_after(date: Date) -> Result<Date, CdsError> {
    let result = cds_roll_on_or_before(date)?
        .add_months(3)
        .map_err(convex_core::cashflows::CashflowError::from)?;
    checked_date(result)?;
    Ok(result)
}
