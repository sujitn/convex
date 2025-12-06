//! Overnight rate compounding calculations.
//!
//! Implements ARRC-standard methodologies for calculating compounded
//! overnight rates over interest periods.

use rust_decimal::Decimal;

use convex_core::calendars::Calendar;
use convex_core::types::Date;

use super::IndexFixingStore;
use crate::types::{RateIndex, SOFRConvention};

/// Overnight rate compounding calculator.
///
/// Provides methods for calculating compounded overnight rates
/// using ARRC (Alternative Reference Rates Committee) standard methodologies.
///
/// # Supported Conventions
///
/// - **Compounded in Arrears**: Daily rates compounded over the period
/// - **Simple Average**: Arithmetic mean of daily rates
/// - **Observation Shift**: Shift observation window relative to interest period
/// - **Lookback**: Use rates from N days prior
/// - **Lockout**: Freeze rate for final N days of period
#[derive(Debug, Clone)]
pub struct OvernightCompounding;

impl OvernightCompounding {
    /// Calculates compounded overnight rate for a period using daily fixings.
    ///
    /// Implements the ARRC-standard compounding formula:
    /// ```text
    /// Compounded Rate = [(∏(1 + rᵢ × dᵢ/360) - 1] × 360/D
    /// ```
    /// where:
    /// - rᵢ = overnight rate for day i
    /// - dᵢ = number of days rate i applies (1 for business days, more for weekends)
    /// - D = total days in the period
    ///
    /// # Arguments
    ///
    /// * `store` - The index fixing store
    /// * `index` - The rate index (SOFR, SONIA, etc.)
    /// * `period_start` - Start of the interest period
    /// * `period_end` - End of the interest period
    /// * `convention` - SOFR convention (lookback, lockout, etc.)
    /// * `calendar` - Business day calendar
    ///
    /// # Returns
    ///
    /// The annualized compounded rate for the period, or None if fixings are missing.
    pub fn compounded_rate(
        store: &IndexFixingStore,
        index: &RateIndex,
        period_start: Date,
        period_end: Date,
        convention: &SOFRConvention,
        calendar: &dyn Calendar,
    ) -> Option<Decimal> {
        match convention {
            SOFRConvention::CompoundedInArrears {
                lookback_days,
                observation_shift,
                lockout_days,
            } => Self::compound_in_arrears(
                store,
                index,
                period_start,
                period_end,
                *lookback_days,
                *observation_shift,
                *lockout_days,
                calendar,
            ),
            SOFRConvention::SimpleAverage { lookback_days } => {
                Self::simple_average(store, index, period_start, period_end, *lookback_days, calendar)
            }
            _ => None, // Term SOFR and CompoundedInAdvance don't use daily compounding
        }
    }

    /// Compounded in arrears calculation.
    #[allow(clippy::too_many_arguments)]
    fn compound_in_arrears(
        store: &IndexFixingStore,
        index: &RateIndex,
        period_start: Date,
        period_end: Date,
        lookback_days: u32,
        observation_shift: bool,
        lockout_days: Option<u32>,
        calendar: &dyn Calendar,
    ) -> Option<Decimal> {
        let mut compounded = 1.0_f64;
        let mut total_days = 0_i64;
        let mut current = period_start;

        // Determine lockout start date
        let lockout_start = lockout_days.map(|lock| {
            calendar.add_business_days(period_end, -(lock as i32))
        });

        // Track the locked rate if in lockout period
        let mut locked_rate: Option<f64> = None;

        while current < period_end {
            let next = calendar.add_business_days(current, 1);
            let weight_days = current.days_between(&next);

            // Determine observation date
            let observation_date = if observation_shift {
                calendar.add_business_days(current, -(lookback_days as i32))
            } else {
                current
            };

            // Check if we're in lockout period
            let rate_date = if let Some(lock_start) = lockout_start {
                if current >= lock_start {
                    // In lockout - use the rate from lockout start
                    if locked_rate.is_none() {
                        let lock_obs = if observation_shift {
                            calendar.add_business_days(lock_start, -(lookback_days as i32))
                        } else {
                            lock_start
                        };
                        locked_rate = store
                            .get_fixing(index, lock_obs)
                            .map(|r| r.to_string().parse::<f64>().unwrap_or(0.0));
                    }
                    lock_start
                } else {
                    observation_date
                }
            } else {
                observation_date
            };

            // Get the rate
            let rate = if locked_rate.is_some() {
                locked_rate.unwrap()
            } else {
                store
                    .get_fixing(index, rate_date)
                    .map(|r| r.to_string().parse::<f64>().unwrap_or(0.0))?
            };

            // Compound: (1 + rate × days/360)
            compounded *= 1.0 + rate * weight_days as f64 / 360.0;
            total_days += weight_days;
            current = next;
        }

        if total_days == 0 {
            return Some(Decimal::ZERO);
        }

        // Annualize: ((compounded - 1) × 360 / total_days)
        let annualized = (compounded - 1.0) * 360.0 / total_days as f64;
        Decimal::try_from(annualized).ok()
    }

    /// Simple average calculation.
    fn simple_average(
        store: &IndexFixingStore,
        index: &RateIndex,
        period_start: Date,
        period_end: Date,
        lookback_days: u32,
        calendar: &dyn Calendar,
    ) -> Option<Decimal> {
        let mut sum = 0.0_f64;
        let mut count = 0_u32;
        let mut current = period_start;

        while current < period_end {
            let observation_date = calendar.add_business_days(current, -(lookback_days as i32));

            let rate = store
                .get_fixing(index, observation_date)
                .map(|r| r.to_string().parse::<f64>().unwrap_or(0.0))?;

            sum += rate;
            count += 1;
            current = calendar.add_business_days(current, 1);
        }

        if count == 0 {
            return Some(Decimal::ZERO);
        }

        Decimal::try_from(sum / count as f64).ok()
    }

    /// Returns all fixing dates needed for a period.
    ///
    /// This is useful for knowing which historical fixings are required
    /// before calculating a compounded rate.
    pub fn required_fixing_dates(
        period_start: Date,
        period_end: Date,
        convention: &SOFRConvention,
        calendar: &dyn Calendar,
    ) -> Vec<Date> {
        let mut dates = Vec::new();

        let (lookback_days, obs_shift) = match convention {
            SOFRConvention::CompoundedInArrears {
                lookback_days,
                observation_shift,
                ..
            } => (*lookback_days, *observation_shift),
            SOFRConvention::SimpleAverage { lookback_days } => (*lookback_days, true),
            _ => return dates,
        };

        let mut current = period_start;
        while current < period_end {
            let obs_date = if obs_shift {
                calendar.add_business_days(current, -(lookback_days as i32))
            } else {
                current
            };
            dates.push(obs_date);
            current = calendar.add_business_days(current, 1);
        }

        // Remove duplicates and sort
        dates.sort();
        dates.dedup();
        dates
    }

    /// Calculates the accrual factor for a period using the standard 360-day basis.
    #[must_use]
    pub fn accrual_factor(period_start: Date, period_end: Date) -> f64 {
        let days = period_start.days_between(&period_end);
        days as f64 / 360.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    use convex_core::calendars::WeekendCalendar;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn make_store_with_flat_sofr(rate: Decimal, start: Date, days: i32) -> IndexFixingStore {
        let mut store = IndexFixingStore::new();
        let mut current = start;
        for _ in 0..days {
            store.add_fixing(current, RateIndex::SOFR, rate);
            current = current + 1;
        }
        store
    }

    #[test]
    fn test_compounded_in_arrears_flat_rate() {
        let calendar = WeekendCalendar;
        let store = make_store_with_flat_sofr(dec!(0.05), date(2024, 1, 1), 100);

        let convention = SOFRConvention::CompoundedInArrears {
            lookback_days: 0,
            observation_shift: false,
            lockout_days: None,
        };

        // 30-day period at flat 5%
        let period_start = date(2024, 1, 2);
        let period_end = date(2024, 2, 1);

        let rate = OvernightCompounding::compounded_rate(
            &store,
            &RateIndex::SOFR,
            period_start,
            period_end,
            &convention,
            &calendar,
        );

        // Flat 5% compounded should be close to 5%
        let r = rate.unwrap();
        assert!((r - dec!(0.05)).abs() < dec!(0.0005)); // Within 5 bps
    }

    #[test]
    fn test_simple_average_flat_rate() {
        let calendar = WeekendCalendar;
        let store = make_store_with_flat_sofr(dec!(0.05), date(2024, 1, 1), 100);

        let convention = SOFRConvention::SimpleAverage { lookback_days: 0 };

        let period_start = date(2024, 1, 2);
        let period_end = date(2024, 2, 1);

        let rate = OvernightCompounding::compounded_rate(
            &store,
            &RateIndex::SOFR,
            period_start,
            period_end,
            &convention,
            &calendar,
        );

        // Simple average of flat 5% = 5%
        assert_eq!(rate, Some(dec!(0.05)));
    }

    #[test]
    fn test_required_fixing_dates() {
        let calendar = WeekendCalendar;
        let convention = SOFRConvention::CompoundedInArrears {
            lookback_days: 2,
            observation_shift: true,
            lockout_days: None,
        };

        let dates = OvernightCompounding::required_fixing_dates(
            date(2024, 1, 8), // Monday
            date(2024, 1, 12), // Friday (4 business days)
            &convention,
            &calendar,
        );

        // Should have observation dates shifted back 2 business days
        assert!(!dates.is_empty());
        // First observation should be 2 business days before Jan 8
        assert!(dates.contains(&date(2024, 1, 4))); // Thursday
    }

    #[test]
    fn test_accrual_factor() {
        let factor = OvernightCompounding::accrual_factor(date(2024, 1, 1), date(2024, 4, 1));
        // 91 days / 360 = 0.2527...
        assert!((factor - 0.2527).abs() < 0.01);
    }
}
