//! Irregular coupon period handling.
//!
//! This module provides utilities for handling irregular (stub) periods
//! in bond schedules. Stub periods occur when the first or last coupon
//! period is shorter or longer than the regular period.
//!
//! # Reference
//!
//! - ICMA Rule 251: Day Count Fractions for Irregular Periods
//! - Bloomberg YAS Manual: Irregular First/Last Coupons

use rust_decimal::Decimal;

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Date, Frequency};

use crate::types::{ReferenceMethod, StubType};

/// Handler for irregular coupon periods.
///
/// Provides utilities to detect, characterize, and calculate day count
/// fractions for irregular (stub) periods according to various market
/// conventions.
///
/// # Example
///
/// ```rust
/// use convex_bonds::cashflows::irregular::IrregularPeriodHandler;
/// use convex_core::types::{Date, Frequency};
///
/// let issue = Date::from_ymd(2025, 2, 15).unwrap();
/// let first_coupon = Date::from_ymd(2025, 6, 15).unwrap();
///
/// let is_irregular = IrregularPeriodHandler::is_first_irregular(
///     issue,
///     first_coupon,
///     Frequency::SemiAnnual,
/// );
/// ```
pub struct IrregularPeriodHandler;

impl IrregularPeriodHandler {
    /// Detects if the first coupon period is irregular.
    ///
    /// A first period is irregular if the days from issue to first coupon
    /// doesn't match the regular period length.
    #[must_use]
    pub fn is_first_irregular(issue_date: Date, first_coupon: Date, frequency: Frequency) -> bool {
        if frequency == Frequency::Zero {
            return false; // Zero coupon bonds don't have irregular periods
        }

        let actual_days = issue_date.days_between(&first_coupon);
        let regular_days = Self::regular_period_days(frequency);

        // Allow 5 day tolerance for business day adjustments
        let tolerance = 5;
        (actual_days - regular_days).abs() > tolerance
    }

    /// Detects if the last coupon period is irregular.
    ///
    /// A last period is irregular if the days from last coupon to maturity
    /// doesn't match the regular period length.
    #[must_use]
    pub fn is_last_irregular(last_coupon: Date, maturity: Date, frequency: Frequency) -> bool {
        if frequency == Frequency::Zero {
            return false;
        }

        let actual_days = last_coupon.days_between(&maturity);
        let regular_days = Self::regular_period_days(frequency);

        let tolerance = 5;
        (actual_days - regular_days).abs() > tolerance
    }

    /// Determines the stub type for an irregular first period.
    #[must_use]
    pub fn first_period_stub_type(
        issue_date: Date,
        first_coupon: Date,
        frequency: Frequency,
    ) -> StubType {
        if frequency == Frequency::Zero {
            return StubType::None;
        }

        let actual_days = issue_date.days_between(&first_coupon);
        let regular_days = Self::regular_period_days(frequency);

        if (actual_days - regular_days).abs() <= 5 {
            StubType::None
        } else if actual_days < regular_days {
            StubType::ShortStub
        } else {
            StubType::LongStub
        }
    }

    /// Determines the stub type for an irregular last period.
    #[must_use]
    pub fn last_period_stub_type(
        last_coupon: Date,
        maturity: Date,
        frequency: Frequency,
    ) -> StubType {
        if frequency == Frequency::Zero {
            return StubType::None;
        }

        let actual_days = last_coupon.days_between(&maturity);
        let regular_days = Self::regular_period_days(frequency);

        if (actual_days - regular_days).abs() <= 5 {
            StubType::None
        } else if actual_days < regular_days {
            StubType::ShortStub
        } else {
            StubType::LongStub
        }
    }

    /// Calculates the reference period for a stub period.
    ///
    /// The reference period is a notional regular period used for day count
    /// calculations according to the specified methodology.
    ///
    /// # Arguments
    ///
    /// * `stub_start` - Start of the stub period
    /// * `stub_end` - End of the stub period
    /// * `frequency` - Coupon frequency
    /// * `method` - Reference period calculation method
    ///
    /// # Returns
    ///
    /// A tuple of (reference_start, reference_end) dates.
    #[must_use]
    pub fn reference_period(
        stub_start: Date,
        stub_end: Date,
        frequency: Frequency,
        method: ReferenceMethod,
    ) -> (Date, Date) {
        let months = Self::period_months(frequency) as i32;

        match method {
            ReferenceMethod::ICMA => {
                // ICMA: Calculate notional regular period ending on stub_end
                let ref_start = stub_end.add_months(-months).unwrap_or(stub_start);
                (ref_start, stub_end)
            }
            ReferenceMethod::ISDA => {
                // ISDA: Use preceding regular period
                let ref_start = stub_start;
                let ref_end = stub_start.add_months(months).unwrap_or(stub_end);
                (ref_start, ref_end)
            }
            ReferenceMethod::Bloomberg => {
                // Bloomberg: Similar to ICMA but may use following period for long stubs
                let actual_days = stub_start.days_between(&stub_end);
                let regular_days = Self::regular_period_days(frequency);

                if actual_days < regular_days {
                    // Short stub: use notional period ending at stub_end
                    let ref_start = stub_end.add_months(-months).unwrap_or(stub_start);
                    (ref_start, stub_end)
                } else {
                    // Long stub: calculate based on actual start
                    let ref_end = stub_start.add_months(months).unwrap_or(stub_end);
                    (stub_start, ref_end)
                }
            }
            ReferenceMethod::USMunicipal => {
                // US Municipal: Use 30/360 day count with specific rules
                // Reference period is always a full coupon period
                let ref_start = stub_end.add_months(-months).unwrap_or(stub_start);
                (ref_start, stub_end)
            }
            ReferenceMethod::Japanese => {
                // Japanese: Simple actual/365 calculation
                // No special reference period needed
                (stub_start, stub_end)
            }
        }
    }

    /// Calculates the day count fraction for an irregular period.
    ///
    /// For ACT/ACT ICMA with irregular periods, the day count fraction uses
    /// the reference period as the denominator basis.
    ///
    /// # Arguments
    ///
    /// * `period_start` - Start of the actual period
    /// * `period_end` - End of the actual period
    /// * `ref_start` - Start of the reference period
    /// * `ref_end` - End of the reference period
    /// * `day_count` - Day count convention
    /// * `frequency` - Coupon frequency
    ///
    /// # Returns
    ///
    /// The year fraction for the period.
    #[must_use]
    pub fn day_count_fraction(
        period_start: Date,
        period_end: Date,
        ref_start: Date,
        ref_end: Date,
        day_count: DayCountConvention,
        frequency: Frequency,
    ) -> Decimal {
        let dc = day_count.to_day_count();

        match day_count {
            DayCountConvention::ActActIcma => {
                // ACT/ACT ICMA: Days in period / (Frequency * Days in ref period)
                let actual_days = period_start.days_between(&period_end);
                let ref_days = ref_start.days_between(&ref_end);
                let freq = Decimal::from(frequency.periods_per_year());

                if ref_days == 0 || freq.is_zero() {
                    Decimal::ZERO
                } else {
                    Decimal::from(actual_days) / (freq * Decimal::from(ref_days))
                }
            }
            _ => {
                // Other day counts: use standard calculation
                dc.year_fraction(period_start, period_end)
            }
        }
    }

    /// Calculates the accrual fraction for an irregular period.
    ///
    /// Similar to day_count_fraction but specifically for accrued interest
    /// calculations from period start to settlement.
    #[must_use]
    pub fn accrual_fraction(
        period_start: Date,
        settlement: Date,
        ref_start: Date,
        ref_end: Date,
        day_count: DayCountConvention,
        frequency: Frequency,
    ) -> Decimal {
        Self::day_count_fraction(
            period_start,
            settlement,
            ref_start,
            ref_end,
            day_count,
            frequency,
        )
    }

    /// Returns the regular period length in days for a frequency.
    #[must_use]
    pub const fn regular_period_days(frequency: Frequency) -> i64 {
        match frequency {
            Frequency::Annual => 365,
            Frequency::SemiAnnual => 182,
            Frequency::Quarterly => 91,
            Frequency::Monthly => 30,
            Frequency::Zero => 0,
        }
    }

    /// Returns the number of months in a regular period.
    #[must_use]
    pub const fn period_months(frequency: Frequency) -> u32 {
        match frequency {
            Frequency::Annual => 12,
            Frequency::SemiAnnual => 6,
            Frequency::Quarterly => 3,
            Frequency::Monthly => 1,
            Frequency::Zero => 0,
        }
    }

    /// Calculates the coupon amount adjustment factor for an irregular period.
    ///
    /// For short first coupons, this factor is < 1.0.
    /// For long first coupons, this factor is > 1.0.
    #[must_use]
    pub fn coupon_adjustment_factor(
        period_start: Date,
        period_end: Date,
        frequency: Frequency,
        day_count: DayCountConvention,
    ) -> Decimal {
        let periods_per_year = Decimal::from(frequency.periods_per_year());
        if periods_per_year.is_zero() {
            return Decimal::ONE;
        }

        let regular_fraction = Decimal::ONE / periods_per_year;
        let actual_fraction = day_count
            .to_day_count()
            .year_fraction(period_start, period_end);

        if regular_fraction.is_zero() {
            Decimal::ONE
        } else {
            actual_fraction / regular_fraction
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_is_first_irregular_short() {
        let issue = Date::from_ymd(2025, 3, 1).unwrap();
        let first = Date::from_ymd(2025, 6, 15).unwrap(); // ~3.5 months, short for semi-annual

        let is_irregular =
            IrregularPeriodHandler::is_first_irregular(issue, first, Frequency::SemiAnnual);
        assert!(is_irregular);
    }

    #[test]
    fn test_is_first_irregular_regular() {
        let issue = Date::from_ymd(2024, 12, 15).unwrap();
        let first = Date::from_ymd(2025, 6, 15).unwrap(); // ~6 months, regular for semi-annual

        let is_irregular =
            IrregularPeriodHandler::is_first_irregular(issue, first, Frequency::SemiAnnual);
        assert!(!is_irregular);
    }

    #[test]
    fn test_stub_type_short() {
        let issue = Date::from_ymd(2025, 4, 1).unwrap();
        let first = Date::from_ymd(2025, 6, 15).unwrap();

        let stub_type =
            IrregularPeriodHandler::first_period_stub_type(issue, first, Frequency::SemiAnnual);
        assert_eq!(stub_type, StubType::ShortStub);
    }

    #[test]
    fn test_stub_type_long() {
        let issue = Date::from_ymd(2024, 10, 1).unwrap();
        let first = Date::from_ymd(2025, 6, 15).unwrap(); // ~8.5 months, long for semi-annual

        let stub_type =
            IrregularPeriodHandler::first_period_stub_type(issue, first, Frequency::SemiAnnual);
        assert_eq!(stub_type, StubType::LongStub);
    }

    #[test]
    fn test_reference_period_icma() {
        let stub_start = Date::from_ymd(2025, 4, 1).unwrap();
        let stub_end = Date::from_ymd(2025, 6, 15).unwrap();

        let (ref_start, ref_end) = IrregularPeriodHandler::reference_period(
            stub_start,
            stub_end,
            Frequency::SemiAnnual,
            ReferenceMethod::ICMA,
        );

        // ICMA: ref period ends at stub_end, starts 6 months earlier
        assert_eq!(ref_end, stub_end);
        assert_eq!(ref_start, Date::from_ymd(2024, 12, 15).unwrap());
    }

    #[test]
    fn test_day_count_fraction_irregular() {
        let period_start = Date::from_ymd(2025, 4, 1).unwrap();
        let period_end = Date::from_ymd(2025, 6, 15).unwrap();
        let ref_start = Date::from_ymd(2024, 12, 15).unwrap();
        let ref_end = Date::from_ymd(2025, 6, 15).unwrap();

        let fraction = IrregularPeriodHandler::day_count_fraction(
            period_start,
            period_end,
            ref_start,
            ref_end,
            DayCountConvention::ActActIcma,
            Frequency::SemiAnnual,
        );

        // 75 days / (2 * 182 days) = 75/364 ~ 0.206
        assert!(fraction > dec!(0.2) && fraction < dec!(0.22));
    }

    #[test]
    fn test_coupon_adjustment_factor_short() {
        let period_start = Date::from_ymd(2025, 4, 1).unwrap();
        let period_end = Date::from_ymd(2025, 6, 15).unwrap();

        let factor = IrregularPeriodHandler::coupon_adjustment_factor(
            period_start,
            period_end,
            Frequency::SemiAnnual,
            DayCountConvention::Thirty360US,
        );

        // Short stub should have factor < 1
        assert!(factor < Decimal::ONE);
        assert!(factor > dec!(0.3));
    }

    #[test]
    fn test_regular_period_days() {
        assert_eq!(
            IrregularPeriodHandler::regular_period_days(Frequency::Annual),
            365
        );
        assert_eq!(
            IrregularPeriodHandler::regular_period_days(Frequency::SemiAnnual),
            182
        );
        assert_eq!(
            IrregularPeriodHandler::regular_period_days(Frequency::Quarterly),
            91
        );
    }

    #[test]
    fn test_period_months() {
        assert_eq!(IrregularPeriodHandler::period_months(Frequency::Annual), 12);
        assert_eq!(
            IrregularPeriodHandler::period_months(Frequency::SemiAnnual),
            6
        );
        assert_eq!(
            IrregularPeriodHandler::period_months(Frequency::Quarterly),
            3
        );
    }
}
