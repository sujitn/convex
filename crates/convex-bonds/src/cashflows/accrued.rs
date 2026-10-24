//! Checked constant-coupon period accrual over supplied dates and cash units.
//! These helpers do not infer payment/entitlement state or a market convention.
//! Full step profiles, delayed payments and complete reference meshes use BondContract.

use crate::{BondError, BondResult};
use convex_core::{
    calendars::{checked, Calendar},
    cashflows::{act365l_year_fraction, icma_year_fraction, ReferencePeriod, MAX_CONTRACT_DAYS},
    daycounts::DayCountConvention,
    types::{Date, Frequency},
};
use rust_decimal::Decimal;

fn invalid(reason: &str) -> BondError {
    BondError::invalid_spec(reason)
}
fn arithmetic() -> BondError {
    BondError::pricing_failed("accrued Decimal overflow/nonzero underflow")
}
fn interval(start: Date, end: Date, allow_zero: bool) -> BondResult<()> {
    if !(1..=9999).contains(&start.year())
        || !(1..=9999).contains(&end.year())
        || end < start
        || (!allow_zero && end == start)
        || start.days_between(&end) > MAX_CONTRACT_DAYS
    {
        return Err(invalid("invalid or excessive accrued date interval"));
    }
    Ok(())
}
fn multiply(left: Decimal, right: Decimal) -> BondResult<Decimal> {
    let value = left.checked_mul(right).ok_or_else(arithmetic)?;
    if left != Decimal::ZERO && right != Decimal::ZERO && value == Decimal::ZERO {
        return Err(arithmetic());
    }
    Ok(value)
}
fn cash(face: Decimal, rate: Decimal, fraction: Decimal) -> BondResult<Decimal> {
    multiply(multiply(face, rate)?, fraction)
}
fn supported(day_count: DayCountConvention) -> BondResult<()> {
    match day_count {
        DayCountConvention::Act360
        | DayCountConvention::Act365Fixed
        | DayCountConvention::ActActIsda
        | DayCountConvention::ActActIcma
        | DayCountConvention::Act365Leap
        | DayCountConvention::Thirty360US
        | DayCountConvention::Thirty360E => Ok(()),
        _ => Err(invalid(
            "accrual convention requires its complete original contract/maturity context",
        )),
    }
}
fn regular(start: Date, end: Date, frequency: Frequency) -> BondResult<()> {
    interval(start, end, false)?;
    let months = frequency.months_per_period() as i32;
    if frequency.is_zero()
        || months == 0
        || !(start.add_months(months).is_ok_and(|d| d == end)
            || end.add_months(-months).is_ok_and(|d| d == start))
    {
        return Err(invalid(
            "explicit regular ICMA reference period/frequency required",
        ));
    }
    Ok(())
}
struct CouponContext {
    start: Date,
    end: Date,
    rate: Decimal,
    face: Decimal,
    day_count: DayCountConvention,
    frequency: Frequency,
}
impl CouponContext {
    fn new(
        start: Date,
        end: Date,
        rate: Decimal,
        face: Decimal,
        day_count: DayCountConvention,
        frequency: Frequency,
    ) -> BondResult<Self> {
        interval(start, end, false)?;
        if face <= Decimal::ZERO {
            return Err(invalid("positive original accrued face required"));
        }
        supported(day_count)?;
        Ok(Self {
            start,
            end,
            rate,
            face,
            day_count,
            frequency,
        })
    }
    fn amount(
        &self,
        start: Date,
        end: Date,
        reference: Option<(Date, Date)>,
    ) -> BondResult<Decimal> {
        interval(start, end, true)?;
        if start < self.start || end > self.end {
            return Err(invalid("accrual outside original period"));
        }
        // An explicit no-coupon frequency has zero coupon accrual, after all
        // dates, original face and convention admission have been validated.
        if self.frequency.is_zero() {
            return Ok(Decimal::ZERO);
        }
        let fraction = match self.day_count {
            DayCountConvention::ActActIcma => {
                let (left, right) =
                    reference.ok_or_else(|| invalid("original ICMA reference bounds required"))?;
                regular(left, right, self.frequency)?;
                if left > self.start || right < self.end {
                    return Err(invalid("single ICMA reference does not cover original period; use complete reference mesh"));
                }
                icma_year_fraction(
                    start,
                    end,
                    self.frequency,
                    &[ReferencePeriod {
                        start: left,
                        end: right,
                    }],
                )
                .map_err(|e| invalid(&e.to_string()))?
            }
            DayCountConvention::Act365Leap => {
                act365l_year_fraction(start, end, self.start, self.end, self.frequency)
                    .map_err(|e| invalid(&e.to_string()))?
            }
            _ => self.day_count.to_day_count().year_fraction(start, end),
        };
        cash(self.face, self.rate, fraction)
    }
}

/// Checked constant-coupon period interest; actual paid/entitlement state is separate.
pub struct AccruedInterestCalculator;
impl AccruedInterestCalculator {
    /// Interest from the original period start through settlement (including end).
    /// Context-free conventions use annual day fractions. ICMA treats the supplied
    /// boundaries as an explicit regular reference period; stubs need irregular_period.
    pub fn standard(
        settlement: Date,
        last_coupon: Date,
        next_coupon: Date,
        coupon_rate: Decimal,
        face_value: Decimal,
        day_count: DayCountConvention,
        frequency: Frequency,
    ) -> BondResult<Decimal> {
        CouponContext::new(
            last_coupon,
            next_coupon,
            coupon_rate,
            face_value,
            day_count,
            frequency,
        )?
        .amount(last_coupon, settlement, Some((last_coupon, next_coupon)))
    }
    /// Remaining-interest ex-coupon convention at/on the actual supplied cutoff.
    /// Counts business days backwards from next_coupon, excluding that anchor.
    /// This does not infer a market calendar, payment delay or recorded payment.
    #[allow(clippy::too_many_arguments)]
    pub fn ex_dividend(
        settlement: Date,
        last_coupon: Date,
        next_coupon: Date,
        coupon_rate: Decimal,
        face_value: Decimal,
        day_count: DayCountConvention,
        frequency: Frequency,
        ex_div_days: u32,
        calendar: &dyn Calendar,
    ) -> BondResult<Decimal> {
        let context = CouponContext::new(
            last_coupon,
            next_coupon,
            coupon_rate,
            face_value,
            day_count,
            frequency,
        )?;
        interval(last_coupon, settlement, true)?;
        if settlement > next_coupon {
            return Err(invalid("settlement after original accrual period"));
        }
        let days = i32::try_from(ex_div_days)
            .map_err(|_| invalid("ex-dividend offset exceeds integer bound"))?;
        let ex_date = checked::add_business_days(next_coupon, -days, calendar)
            .map_err(|e| invalid(&e.to_string()))?;
        let reference = Some((last_coupon, next_coupon));
        if settlement >= ex_date {
            let amount = context.amount(settlement, next_coupon, reference)?;
            Decimal::ZERO.checked_sub(amount).ok_or_else(arithmetic)
        } else {
            context.amount(last_coupon, settlement, reference)
        }
    }
    /// Single-reference stub interest. Full original period/end and complete
    /// supplied reference coverage are checked, including a zero requested interval.
    #[allow(clippy::too_many_arguments)]
    pub fn irregular_period(
        settlement: Date,
        period_start: Date,
        period_end: Date,
        ref_period_start: Date,
        ref_period_end: Date,
        coupon_rate: Decimal,
        face_value: Decimal,
        day_count: DayCountConvention,
        frequency: Frequency,
    ) -> BondResult<Decimal> {
        let context = CouponContext::new(
            period_start,
            period_end,
            coupon_rate,
            face_value,
            day_count,
            frequency,
        )?;
        interval(ref_period_start, ref_period_end, false)?;
        if ref_period_start > period_start || ref_period_end < period_end {
            return Err(invalid("reference does not cover original accrual period"));
        }
        context.amount(
            period_start,
            settlement,
            Some((ref_period_start, ref_period_end)),
        )
    }
    /// Annual-rate interest over supplied dates for an explicitly context-free law.
    /// ICMA, ACT/365L and maturity-dependent conventions cannot use this signature.
    pub fn using_year_fraction(
        settlement: Date,
        last_coupon: Date,
        coupon_rate: Decimal,
        face_value: Decimal,
        day_count: DayCountConvention,
    ) -> BondResult<Decimal> {
        interval(last_coupon, settlement, true)?;
        if face_value <= Decimal::ZERO {
            return Err(invalid("positive original accrued face required"));
        }
        match day_count {
            DayCountConvention::Act360
            | DayCountConvention::Act365Fixed
            | DayCountConvention::ActActIsda
            | DayCountConvention::Thirty360US
            | DayCountConvention::Thirty360E => cash(
                face_value,
                coupon_rate,
                day_count
                    .to_day_count()
                    .year_fraction(last_coupon, settlement),
            ),
            _ => Err(invalid(
                "year-fraction accrual requires original coupon/reference/maturity context",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CalendarId;
    use rust_decimal_macros::dec;

    #[test]
    fn test_standard_accrued_30_360() {
        // Boeing 7.5% due 06/15/2025
        // Settlement: 04/29/2020
        // Last coupon: 12/15/2019
        // Next coupon: 06/15/2020
        let settlement = Date::from_ymd(2020, 4, 29).unwrap();
        let last_coupon = Date::from_ymd(2019, 12, 15).unwrap();
        let next_coupon = Date::from_ymd(2020, 6, 15).unwrap();

        let accrued = AccruedInterestCalculator::standard(
            settlement,
            last_coupon,
            next_coupon,
            dec!(0.075),     // 7.5%
            dec!(1_000_000), // $1M face
            DayCountConvention::Thirty360US,
            Frequency::SemiAnnual,
        )
        .unwrap();

        // Expected: 134 days accrued out of 180 day period
        // Coupon per period = 1,000,000 * 0.075 / 2 = 37,500
        // Accrued = 37,500 * 134 / 180 = 27,916.67 (approx 26,986.11 with precise calc)
        // Note: The Bloomberg value is 26,986.11 - let's verify our calculation

        // 30/360 US: Dec 15 to Apr 29
        // Dec: 15 days (15 to 30)
        // Jan: 30 days
        // Feb: 30 days
        // Mar: 30 days
        // Apr: 29 days
        // Total: 15 + 30 + 30 + 30 + 29 = 134 days

        // Period: Dec 15 to Jun 15 = 180 days (30/360)
        // Accrued = 37,500 * 134/180 = 27,916.6667

        assert!(accrued > dec!(27000));
        assert!(accrued < dec!(28000));
    }

    #[test]
    fn test_accrued_zero_coupon() {
        let settlement = Date::from_ymd(2025, 4, 15).unwrap();
        let last_coupon = Date::from_ymd(2020, 1, 15).unwrap();
        let next_coupon = Date::from_ymd(2030, 1, 15).unwrap();

        let accrued = AccruedInterestCalculator::standard(
            settlement,
            last_coupon,
            next_coupon,
            dec!(0.05),
            dec!(100),
            DayCountConvention::Thirty360US,
            Frequency::Zero,
        )
        .unwrap();

        assert_eq!(accrued, Decimal::ZERO);
    }

    #[test]
    fn test_ex_dividend_normal_period() {
        // Settlement before ex-div date - normal accrued
        let settlement = Date::from_ymd(2025, 4, 15).unwrap();
        let last_coupon = Date::from_ymd(2025, 1, 15).unwrap();
        let next_coupon = Date::from_ymd(2025, 7, 15).unwrap();
        let calendar = CalendarId::weekend_only().try_to_calendar().unwrap();

        let accrued = AccruedInterestCalculator::ex_dividend(
            settlement,
            last_coupon,
            next_coupon,
            dec!(0.04),
            dec!(100),
            DayCountConvention::ActActIcma,
            Frequency::SemiAnnual,
            7, // 7 business days ex-div
            calendar.as_ref(),
        )
        .unwrap();

        assert!(accrued > Decimal::ZERO);
    }

    #[test]
    fn test_ex_dividend_in_ex_period() {
        // Settlement in ex-dividend period (7 business days before coupon)
        // Next coupon: July 15, 2025 (Tuesday)
        // 7 business days before = July 3, 2025 (Thursday) - ex-div date
        // Settlement on July 10, 2025 = in ex-dividend period
        let settlement = Date::from_ymd(2025, 7, 10).unwrap();
        let last_coupon = Date::from_ymd(2025, 1, 15).unwrap();
        let next_coupon = Date::from_ymd(2025, 7, 15).unwrap();
        let calendar = CalendarId::weekend_only().try_to_calendar().unwrap();

        let accrued = AccruedInterestCalculator::ex_dividend(
            settlement,
            last_coupon,
            next_coupon,
            dec!(0.04),
            dec!(100),
            DayCountConvention::ActActIcma,
            Frequency::SemiAnnual,
            7,
            calendar.as_ref(),
        )
        .unwrap();

        // Should be negative in ex-dividend period
        assert!(accrued < Decimal::ZERO);
    }

    #[test]
    fn test_irregular_period_short_first() {
        // Short first period (stub)
        let settlement = Date::from_ymd(2025, 5, 15).unwrap();
        let period_start = Date::from_ymd(2025, 3, 1).unwrap(); // Odd start
        let period_end = Date::from_ymd(2025, 6, 15).unwrap();
        let ref_start = Date::from_ymd(2024, 12, 15).unwrap(); // Regular period start
        let ref_end = Date::from_ymd(2025, 6, 15).unwrap(); // Regular period end

        let accrued = AccruedInterestCalculator::irregular_period(
            settlement,
            period_start,
            period_end,
            ref_start,
            ref_end,
            dec!(0.05),
            dec!(100),
            DayCountConvention::ActActIcma,
            Frequency::SemiAnnual,
        )
        .unwrap();

        assert!(accrued > Decimal::ZERO);
        assert!(accrued < dec!(2.5)); // Less than full period coupon
    }

    #[test]
    fn test_using_year_fraction() {
        let settlement = Date::from_ymd(2025, 4, 15).unwrap();
        let last_coupon = Date::from_ymd(2025, 1, 15).unwrap();

        let accrued = AccruedInterestCalculator::using_year_fraction(
            settlement,
            last_coupon,
            dec!(0.05),
            dec!(100),
            DayCountConvention::Act360,
        )
        .unwrap();

        // 90 days / 360 = 0.25
        // 100 * 0.05 * 0.25 = 1.25
        assert!(accrued > dec!(1.2));
        assert!(accrued < dec!(1.3));
    }
}
