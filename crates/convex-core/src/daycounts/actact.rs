//! Actual/Actual day count conventions.

use rust_decimal::Decimal;

use super::DayCount;
use crate::types::Date;

/// Actual/Actual ISDA day count convention.
///
/// The year fraction is calculated by splitting the period into
/// portions that fall in leap years vs non-leap years.
///
/// # Formula
///
/// $$\text{Year Fraction} = \frac{\text{Days in non-leap year}}{365} + \frac{\text{Days in leap year}}{366}$$
#[derive(Debug, Clone, Copy, Default)]
pub struct ActActIsda;

impl DayCount for ActActIsda {
    fn name(&self) -> &'static str {
        "ACT/ACT ISDA"
    }

    fn year_fraction(&self, start: Date, end: Date) -> Decimal {
        if start >= end {
            return Decimal::ZERO;
        }

        let mut total = Decimal::ZERO;
        let mut current = start;

        // Process year by year
        while current.year() < end.year() {
            let year_end = Date::from_ymd(current.year(), 12, 31).unwrap();
            let days_in_year = current.days_in_year();
            let days = current.days_between(&year_end) + 1; // Include Dec 31

            total += Decimal::from(days) / Decimal::from(days_in_year);

            current = Date::from_ymd(current.year() + 1, 1, 1).unwrap();
        }

        // Handle remaining portion in the final year
        if current < end {
            let days = current.days_between(&end);
            let days_in_year = current.days_in_year();
            total += Decimal::from(days) / Decimal::from(days_in_year);
        }

        total
    }

    fn day_count(&self, start: Date, end: Date) -> i64 {
        start.days_between(&end)
    }
}

/// Actual/Actual ICMA day count convention.
///
/// The year fraction depends on the coupon frequency and the
/// actual number of days in the coupon period.
///
/// # Formula
///
/// $$\text{Year Fraction} = \frac{\text{Days}}{\text{Frequency} \times \text{Days in Period}}$$
#[derive(Debug, Clone, Copy)]
pub struct ActActIcma {
    /// Coupon frequency (periods per year)
    frequency: u32,
}

impl ActActIcma {
    /// Creates a new ACT/ACT ICMA convention with given frequency.
    #[must_use]
    pub fn new(frequency: u32) -> Self {
        Self { frequency }
    }

    /// Creates with semi-annual frequency (default for bonds).
    #[must_use]
    pub fn semi_annual() -> Self {
        Self { frequency: 2 }
    }

    /// Calculates year fraction given the period dates.
    ///
    /// # Arguments
    ///
    /// * `start` - Accrual start date
    /// * `end` - Accrual end date
    /// * `period_start` - Start of the coupon period
    /// * `period_end` - End of the coupon period
    #[must_use]
    pub fn year_fraction_with_period(
        &self,
        start: Date,
        end: Date,
        period_start: Date,
        period_end: Date,
    ) -> Decimal {
        let days_in_period = period_start.days_between(&period_end);
        if days_in_period == 0 {
            return Decimal::ZERO;
        }

        let accrued_days = start.days_between(&end);
        Decimal::from(accrued_days)
            / (Decimal::from(self.frequency) * Decimal::from(days_in_period))
    }
}

impl Default for ActActIcma {
    fn default() -> Self {
        Self::semi_annual()
    }
}

impl DayCount for ActActIcma {
    fn name(&self) -> &'static str {
        "ACT/ACT ICMA"
    }

    fn year_fraction(&self, start: Date, end: Date) -> Decimal {
        // Without period information, fall back to simple calculation
        // In practice, you'd use year_fraction_with_period
        let days = start.days_between(&end);
        Decimal::from(days) / Decimal::from(365)
    }

    fn day_count(&self, start: Date, end: Date) -> i64 {
        start.days_between(&end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_actact_isda_non_leap() {
        let dc = ActActIsda;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2026, 1, 1).unwrap();

        // Full non-leap year
        assert_eq!(dc.year_fraction(start, end), dec!(1));
    }

    #[test]
    fn test_actact_isda_leap() {
        let dc = ActActIsda;
        let start = Date::from_ymd(2024, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 1, 1).unwrap();

        // Full leap year
        assert_eq!(dc.year_fraction(start, end), dec!(1));
    }

    #[test]
    fn test_actact_isda_cross_year() {
        let dc = ActActIsda;
        let start = Date::from_ymd(2024, 7, 1).unwrap();
        let end = Date::from_ymd(2025, 7, 1).unwrap();

        // Crosses from leap year to non-leap year
        let yf = dc.year_fraction(start, end);
        assert!(yf > dec!(0.99) && yf < dec!(1.01));
    }

    #[test]
    fn test_actact_icma_with_period() {
        let dc = ActActIcma::semi_annual();

        let period_start = Date::from_ymd(2025, 1, 15).unwrap();
        let period_end = Date::from_ymd(2025, 7, 15).unwrap();
        let start = Date::from_ymd(2025, 1, 15).unwrap();
        let end = Date::from_ymd(2025, 4, 15).unwrap();

        let yf = dc.year_fraction_with_period(start, end, period_start, period_end);

        // 90 days out of ~181 day period, freq=2
        // yf = 90 / (2 * 181) ≈ 0.2486
        assert!(yf > dec!(0.24) && yf < dec!(0.26));
    }
}
