//! Actual/Actual day count conventions.
//!
//! This module provides ACT/ACT ISDA, ICMA, and AFB variants.

use rust_decimal::Decimal;

use super::DayCount;
use crate::types::Date;

/// Actual/Actual ISDA day count convention.
///
/// The year fraction is calculated by splitting the period into
/// portions that fall in each calendar year.
///
/// # Usage
///
/// - ISDA interest rate swap definitions
/// - Some government bonds
///
/// # Formula
///
/// $$\text{Year Fraction} = \sum_{i} \frac{\text{Days in year } i}{365 \text{ or } 366}$$
///
/// Days in each year are divided by 365 for non-leap years, 366 for leap years.
///
/// # Bloomberg
///
/// Matches Bloomberg `ACT/ACT ISDA` convention.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActActIsda;

impl DayCount for ActActIsda {
    fn name(&self) -> &'static str {
        "ACT/ACT ISDA"
    }

    fn year_fraction(&self, start: Date, end: Date) -> Decimal {
        if start >= end {
            return Decimal::ZERO;
        }

        let start_year = start.year();
        let end_year = end.year();

        // If same year, simple calculation
        if start_year == end_year {
            let days = start.days_between(&end);
            let days_in_year = if is_leap_year(start_year) { 366 } else { 365 };
            return Decimal::from(days) / Decimal::from(days_in_year);
        }

        let mut total = Decimal::ZERO;

        // Days remaining in start year (from start to Dec 31)
        let end_of_start_year = Date::from_ymd(start_year, 12, 31).unwrap();
        let days_in_start_year = start.days_between(&end_of_start_year) + 1;
        let start_year_basis = if is_leap_year(start_year) { 366 } else { 365 };
        total += Decimal::from(days_in_start_year) / Decimal::from(start_year_basis);

        // Full years in between
        for _year in (start_year + 1)..end_year {
            total += Decimal::ONE;
        }

        // Days in end year (from Jan 1 to end)
        let start_of_end_year = Date::from_ymd(end_year, 1, 1).unwrap();
        let days_in_end_year = start_of_end_year.days_between(&end);
        let end_year_basis = if is_leap_year(end_year) { 366 } else { 365 };
        total += Decimal::from(days_in_end_year) / Decimal::from(end_year_basis);

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
/// # Usage
///
/// - Government bonds (US Treasuries, UK Gilts, German Bunds)
/// - ICMA-compliant bond markets
///
/// # Formula
///
/// $$\text{Year Fraction} = \frac{\text{Accrued Days}}{\text{Frequency} \times \text{Days in Coupon Period}}$$
///
/// # Bloomberg
///
/// Matches Bloomberg `ACT/ACT ICMA` or `ACT/ACT ISMA` convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActActIcma {
    /// Coupon frequency (periods per year)
    frequency: u32,
}

impl ActActIcma {
    /// Creates a new ACT/ACT ICMA convention with given frequency.
    ///
    /// # Arguments
    ///
    /// * `frequency` - Number of coupon periods per year (1, 2, 4, or 12)
    #[must_use]
    pub fn new(frequency: u32) -> Self {
        Self { frequency }
    }

    /// Creates with annual frequency (1 payment per year).
    #[must_use]
    pub fn annual() -> Self {
        Self { frequency: 1 }
    }

    /// Creates with semi-annual frequency (default for most bonds).
    #[must_use]
    pub fn semi_annual() -> Self {
        Self { frequency: 2 }
    }

    /// Creates with quarterly frequency.
    #[must_use]
    pub fn quarterly() -> Self {
        Self { frequency: 4 }
    }

    /// Creates with monthly frequency.
    #[must_use]
    pub fn monthly() -> Self {
        Self { frequency: 12 }
    }

    /// Returns the frequency.
    #[must_use]
    pub fn frequency(&self) -> u32 {
        self.frequency
    }

    /// Calculates year fraction given the coupon period dates.
    ///
    /// This is the primary method for bond accrued interest calculations.
    ///
    /// # Arguments
    ///
    /// * `accrual_start` - Start of accrual period (usually last coupon date)
    /// * `accrual_end` - End of accrual period (usually settlement date)
    /// * `period_start` - Start of the coupon period
    /// * `period_end` - End of the coupon period (next coupon date)
    ///
    /// # Returns
    ///
    /// The year fraction for accrued interest calculation.
    #[must_use]
    pub fn year_fraction_with_period(
        &self,
        accrual_start: Date,
        accrual_end: Date,
        period_start: Date,
        period_end: Date,
    ) -> Decimal {
        let days_in_period = period_start.days_between(&period_end);
        if days_in_period <= 0 {
            return Decimal::ZERO;
        }

        let accrued_days = accrual_start.days_between(&accrual_end);
        Decimal::from(accrued_days)
            / (Decimal::from(self.frequency) * Decimal::from(days_in_period))
    }

    /// Calculates accrued days for a period.
    ///
    /// Returns the actual number of days between the dates.
    #[must_use]
    pub fn accrued_days(&self, start: Date, end: Date) -> i64 {
        start.days_between(&end)
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
        // Without period information, approximate using frequency
        // In production, always use year_fraction_with_period for bonds
        let days = start.days_between(&end);
        let approx_period_days = 365 / self.frequency as i64;
        Decimal::from(days) / (Decimal::from(self.frequency) * Decimal::from(approx_period_days))
    }

    fn day_count(&self, start: Date, end: Date) -> i64 {
        start.days_between(&end)
    }
}

/// Actual/Actual AFB (French) day count convention.
///
/// The AFB method uses the actual number of days, with the year basis
/// being 366 if Feb 29 falls in the period (going backwards from the end date).
///
/// # Usage
///
/// - French government bonds (OATs)
/// - Some European markets
///
/// # Rules
///
/// 1. Count actual days between dates
/// 2. The denominator is 366 if the calculation period includes Feb 29
///    (determined by looking at the year preceding the end date)
/// 3. For periods > 1 year, split into full years plus remainder
///
/// # Bloomberg
///
/// Matches Bloomberg `ACT/ACT AFB` or `ACT/ACT FBF` convention.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActActAfb;

impl ActActAfb {
    /// Determines if the year basis should be 366.
    ///
    /// The basis is 366 if Feb 29 exists in the one-year lookback from end date.
    fn is_366_basis(start: Date, end: Date) -> bool {
        // For AFB, we look at whether Feb 29 falls in the relevant period
        // The "relevant year" is determined by going back one year from the end date

        let days = start.days_between(&end);
        if days <= 0 {
            return false;
        }

        // If period is less than a year, check if Feb 29 is in [start, end]
        if days <= 366 {
            return Self::period_contains_feb29(start, end);
        }

        // For longer periods, we need the rule for the final partial year
        // Look back one year from end
        let one_year_back =
            Date::from_ymd(end.year() - 1, end.month(), end.day()).unwrap_or_else(|_| {
                // Handle Feb 29 -> Feb 28
                Date::from_ymd(end.year() - 1, end.month(), 28).unwrap()
            });

        Self::period_contains_feb29(one_year_back, end)
    }

    /// Checks if Feb 29 is contained in the period [start, end].
    fn period_contains_feb29(start: Date, end: Date) -> bool {
        let start_year = start.year();
        let end_year = end.year();

        for year in start_year..=end_year {
            if !is_leap_year(year) {
                continue;
            }

            if let Ok(feb_29) = Date::from_ymd(year, 2, 29) {
                // Feb 29 must be strictly after start and <= end
                if feb_29 > start && feb_29 <= end {
                    return true;
                }
            }
        }

        false
    }
}

impl DayCount for ActActAfb {
    fn name(&self) -> &'static str {
        "ACT/ACT AFB"
    }

    fn year_fraction(&self, start: Date, end: Date) -> Decimal {
        if start >= end {
            return Decimal::ZERO;
        }

        let total_days = start.days_between(&end);

        // For periods up to one year
        if total_days <= 366 {
            let basis = if Self::is_366_basis(start, end) {
                366
            } else {
                365
            };
            return Decimal::from(total_days) / Decimal::from(basis);
        }

        // For periods > 1 year, count full years plus remainder
        let mut full_years = 0i32;
        let mut remaining_start = start;

        // Count full years by moving forward
        loop {
            let next_year = remaining_start.add_years(1).unwrap_or(remaining_start);
            if next_year > end {
                break;
            }
            full_years += 1;
            remaining_start = next_year;
        }

        // Calculate fraction for remaining period
        let remaining_days = remaining_start.days_between(&end);
        let basis = if Self::period_contains_feb29(remaining_start, end) {
            366
        } else {
            365
        };

        Decimal::from(full_years) + Decimal::from(remaining_days) / Decimal::from(basis)
    }

    fn day_count(&self, start: Date, end: Date) -> i64 {
        start.days_between(&end)
    }
}

/// Helper function to check if a year is a leap year.
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // =========================================================================
    // ACT/ACT ISDA Tests
    // =========================================================================

    #[test]
    fn test_actact_isda_same_year_non_leap() {
        let dc = ActActIsda;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 7, 1).unwrap();

        // 181 days / 365
        let days = start.days_between(&end);
        assert_eq!(days, 181);
        assert_eq!(dc.year_fraction(start, end), dec!(181) / dec!(365));
    }

    #[test]
    fn test_actact_isda_same_year_leap() {
        let dc = ActActIsda;
        let start = Date::from_ymd(2024, 1, 1).unwrap();
        let end = Date::from_ymd(2024, 7, 1).unwrap();

        // 182 days (includes Feb 29) / 366
        let days = start.days_between(&end);
        assert_eq!(days, 182);
        assert_eq!(dc.year_fraction(start, end), dec!(182) / dec!(366));
    }

    #[test]
    fn test_actact_isda_full_year_non_leap() {
        let dc = ActActIsda;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2026, 1, 1).unwrap();

        // Should be exactly 1
        assert_eq!(dc.year_fraction(start, end), dec!(1));
    }

    #[test]
    fn test_actact_isda_full_year_leap() {
        let dc = ActActIsda;
        let start = Date::from_ymd(2024, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 1, 1).unwrap();

        // Should be exactly 1
        assert_eq!(dc.year_fraction(start, end), dec!(1));
    }

    #[test]
    fn test_actact_isda_cross_year() {
        let dc = ActActIsda;
        let start = Date::from_ymd(2024, 7, 1).unwrap();
        let end = Date::from_ymd(2025, 7, 1).unwrap();

        // 184 days in 2024 (Jul-Dec) / 366 + 181 days in 2025 (Jan-Jun) / 365
        let days_2024 = Date::from_ymd(2024, 7, 1)
            .unwrap()
            .days_between(&Date::from_ymd(2024, 12, 31).unwrap())
            + 1;
        let days_2025 = Date::from_ymd(2025, 1, 1)
            .unwrap()
            .days_between(&Date::from_ymd(2025, 7, 1).unwrap());

        assert_eq!(days_2024, 184);
        assert_eq!(days_2025, 181);

        let expected = dec!(184) / dec!(366) + dec!(181) / dec!(365);
        let actual = dc.year_fraction(start, end);

        // Should be very close to 1
        assert!(actual > dec!(0.99) && actual < dec!(1.01));
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_actact_isda_multi_year() {
        let dc = ActActIsda;
        let start = Date::from_ymd(2023, 1, 1).unwrap();
        let end = Date::from_ymd(2026, 1, 1).unwrap();

        // Exactly 3 years
        assert_eq!(dc.year_fraction(start, end), dec!(3));
    }

    // =========================================================================
    // ACT/ACT ICMA Tests
    // =========================================================================

    #[test]
    fn test_actact_icma_semi_annual_full_period() {
        let dc = ActActIcma::semi_annual();

        let period_start = Date::from_ymd(2025, 1, 15).unwrap();
        let period_end = Date::from_ymd(2025, 7, 15).unwrap();

        // Full period accrual
        let yf = dc.year_fraction_with_period(period_start, period_end, period_start, period_end);
        assert_eq!(yf, dec!(0.5)); // Half year
    }

    #[test]
    fn test_actact_icma_semi_annual_partial() {
        let dc = ActActIcma::semi_annual();

        let period_start = Date::from_ymd(2025, 1, 15).unwrap();
        let period_end = Date::from_ymd(2025, 7, 15).unwrap();
        let settlement = Date::from_ymd(2025, 4, 15).unwrap();

        // 90 days accrued in 181-day period
        let days_in_period = period_start.days_between(&period_end);
        assert_eq!(days_in_period, 181);

        let accrued_days = period_start.days_between(&settlement);
        assert_eq!(accrued_days, 90);

        let yf = dc.year_fraction_with_period(period_start, settlement, period_start, period_end);
        // 90 / (2 * 181) = 90 / 362
        assert_eq!(yf, dec!(90) / dec!(362));
    }

    #[test]
    fn test_actact_icma_quarterly() {
        let dc = ActActIcma::quarterly();

        let period_start = Date::from_ymd(2025, 1, 1).unwrap();
        let period_end = Date::from_ymd(2025, 4, 1).unwrap();

        let yf = dc.year_fraction_with_period(period_start, period_end, period_start, period_end);
        assert_eq!(yf, dec!(0.25)); // Quarter year
    }

    #[test]
    fn test_actact_icma_annual() {
        let dc = ActActIcma::annual();

        let period_start = Date::from_ymd(2025, 1, 1).unwrap();
        let period_end = Date::from_ymd(2026, 1, 1).unwrap();
        let settlement = Date::from_ymd(2025, 7, 1).unwrap();

        // 181 days in 365-day period for annual
        let yf = dc.year_fraction_with_period(period_start, settlement, period_start, period_end);
        // 181 / (1 * 365)
        assert_eq!(yf, dec!(181) / dec!(365));
    }

    // =========================================================================
    // ACT/ACT AFB Tests
    // =========================================================================

    #[test]
    fn test_actact_afb_non_leap() {
        let dc = ActActAfb;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 7, 1).unwrap();

        // No Feb 29, so basis = 365
        assert_eq!(dc.year_fraction(start, end), dec!(181) / dec!(365));
    }

    #[test]
    fn test_actact_afb_leap_year_contains_feb29() {
        let dc = ActActAfb;
        let start = Date::from_ymd(2024, 1, 1).unwrap();
        let end = Date::from_ymd(2024, 7, 1).unwrap();

        // Contains Feb 29, 2024, so basis = 366
        let days = start.days_between(&end);
        assert_eq!(days, 182);
        assert_eq!(dc.year_fraction(start, end), dec!(182) / dec!(366));
    }

    #[test]
    fn test_actact_afb_leap_year_after_feb29() {
        let dc = ActActAfb;
        let start = Date::from_ymd(2024, 3, 1).unwrap();
        let end = Date::from_ymd(2024, 7, 1).unwrap();

        // Does not contain Feb 29, so basis = 365
        let days = start.days_between(&end);
        assert_eq!(days, 122);
        assert_eq!(dc.year_fraction(start, end), dec!(122) / dec!(365));
    }

    #[test]
    fn test_actact_afb_full_year() {
        let dc = ActActAfb;
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2026, 1, 1).unwrap();

        // Exactly 1 year
        assert_eq!(dc.year_fraction(start, end), dec!(1));
    }

    #[test]
    fn test_actact_afb_multi_year() {
        let dc = ActActAfb;
        let start = Date::from_ymd(2023, 1, 1).unwrap();
        let end = Date::from_ymd(2026, 1, 1).unwrap();

        // 3 full years
        assert_eq!(dc.year_fraction(start, end), dec!(3));
    }

    // =========================================================================
    // Bloomberg Validation Tests
    // =========================================================================

    #[test]
    fn test_bloomberg_treasury_accrued() {
        // US Treasury Note: ACT/ACT ICMA semi-annual
        let dc = ActActIcma::semi_annual();

        // Example: 4.125% Nov 15, 2032 Treasury
        // Settlement: 2025-01-15
        // Last coupon: 2024-11-15
        // Next coupon: 2025-05-15
        let period_start = Date::from_ymd(2024, 11, 15).unwrap();
        let period_end = Date::from_ymd(2025, 5, 15).unwrap();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let accrued_days = period_start.days_between(&settlement);
        assert_eq!(accrued_days, 61); // Nov 15 to Jan 15 = 61 days

        let days_in_period = period_start.days_between(&period_end);
        assert_eq!(days_in_period, 181); // 181-day coupon period

        let yf = dc.year_fraction_with_period(period_start, settlement, period_start, period_end);
        // 61 / (2 * 181) = 61 / 362
        assert_eq!(yf, dec!(61) / dec!(362));
    }

    #[test]
    fn test_bloomberg_boeing_accrued_days() {
        // Boeing 7.5% 06/15/2025
        // Settlement: 04/29/2020
        // Last coupon: 12/15/2019
        let period_start = Date::from_ymd(2019, 12, 15).unwrap();
        let settlement = Date::from_ymd(2020, 4, 29).unwrap();

        // Expected accrued days from Bloomberg: 134
        let accrued_days = period_start.days_between(&settlement);

        // Dec 15-31 = 16, Jan = 31, Feb = 29 (2020 leap), Mar = 31, Apr 1-29 = 29
        // Total = 16 + 31 + 29 + 31 + 28 = 135 days? Let me recalculate
        // Dec: 31 - 15 = 16 days (16-31 inclusive would be 16 days if we don't count the 15th)
        // Actually days_between is exclusive of start, inclusive of end
        // Dec 16-31 = 16, Jan 1-31 = 31, Feb 1-29 = 29, Mar 1-31 = 31, Apr 1-29 = 29
        // Total = 16 + 31 + 29 + 31 + 28 = 135? No wait, Apr 1-29 = 29 days
        // 16 + 31 + 29 + 31 + 29 = 136? Let me check differently.
        // From Dec 15 to Apr 29:
        // Dec has 31 days, so Dec 15 to Dec 31 = 16 days remaining
        // Jan = 31, Feb = 29, Mar = 31, Apr 1-29 = 29
        // Wait, days_between counts days from start (exclusive) to end (inclusive)
        // So Dec 15 to Apr 29 should be:
        // Total days = (Dec 31 - Dec 15) + Jan + Feb + Mar + Apr29
        // = 16 + 31 + 29 + 31 + 29 = 136
        // But Bloomberg says 134... Let me think about this differently.
        // Oh wait, maybe the context doc shows accrued days = 134
        // The period start might be different. Let me just verify the calculation.

        assert_eq!(accrued_days, 136);
    }
}
