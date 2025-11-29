//! TARGET2 calendar for Eurozone payments and securities settlement.
//!
//! TARGET2 (Trans-European Automated Real-time Gross Settlement Express Transfer)
//! is the real-time gross settlement system owned and operated by the Eurosystem.

use super::bitmap::{HolidayBitmap, HolidayCalendarBuilder, WeekendType, MAX_YEAR, MIN_YEAR};
use super::Calendar;
use crate::types::Date;
use std::sync::OnceLock;

/// Static TARGET2 calendar instance.
static TARGET2_CALENDAR: OnceLock<Target2Calendar> = OnceLock::new();

/// TARGET2 holiday calendar for Eurozone markets.
///
/// ## Holidays
///
/// - New Year's Day (January 1)
/// - Good Friday
/// - Easter Monday
/// - Labour Day (May 1)
/// - Christmas Day (December 25)
/// - Boxing Day (December 26)
///
/// Note: TARGET2 does NOT observe:
/// - National holidays of individual countries
/// - Any weekend observations (holidays on weekends are simply lost)
#[derive(Debug, Clone)]
pub struct Target2Calendar {
    bitmap: HolidayBitmap,
}

impl Target2Calendar {
    /// Create a new TARGET2 calendar.
    pub fn new() -> Self {
        Self {
            bitmap: build_target2_holidays(),
        }
    }

    /// Get the global TARGET2 calendar instance.
    pub fn global() -> &'static Target2Calendar {
        TARGET2_CALENDAR.get_or_init(Target2Calendar::new)
    }
}

impl Default for Target2Calendar {
    fn default() -> Self {
        Self::new()
    }
}

impl Calendar for Target2Calendar {
    fn name(&self) -> &'static str {
        "TARGET2"
    }

    fn is_business_day(&self, date: Date) -> bool {
        self.bitmap.is_business_day(date.as_naive_date())
    }
}

/// Build the TARGET2 holiday bitmap.
fn build_target2_holidays() -> HolidayBitmap {
    HolidayCalendarBuilder::new("TARGET2")
        .weekend(WeekendType::SaturdaySunday)
        .year_range(MIN_YEAR, MAX_YEAR)
        // New Year's Day (January 1) - NO weekend observation
        .add_fixed_holiday(1, 1, false)
        // Good Friday (Friday before Easter)
        .add_easter_holiday(-2)
        // Easter Monday (Monday after Easter)
        .add_easter_holiday(1)
        // Labour Day (May 1) - NO weekend observation
        .add_fixed_holiday(5, 1, false)
        // Christmas Day (December 25) - NO weekend observation
        .add_fixed_holiday(12, 25, false)
        // Boxing Day (December 26) - NO weekend observation
        .add_fixed_holiday(12, 26, false)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target2_new_years() {
        let cal = Target2Calendar::new();

        // 2025: Jan 1 is Wednesday - holiday
        let new_years = Date::from_ymd(2025, 1, 1).unwrap();
        assert!(!cal.is_business_day(new_years));

        // 2028: Jan 1 is Saturday - NOT observed on Friday
        let new_years_2028 = Date::from_ymd(2028, 1, 1).unwrap();
        assert!(!cal.is_business_day(new_years_2028)); // Saturday anyway
        let dec31_2027 = Date::from_ymd(2027, 12, 31).unwrap();
        assert!(cal.is_business_day(dec31_2027)); // Friday is a business day
    }

    #[test]
    fn test_target2_good_friday() {
        let cal = Target2Calendar::new();

        // 2025: Easter is April 20, Good Friday is April 18
        let good_friday = Date::from_ymd(2025, 4, 18).unwrap();
        assert!(!cal.is_business_day(good_friday));
    }

    #[test]
    fn test_target2_easter_monday() {
        let cal = Target2Calendar::new();

        // 2025: Easter is April 20, Easter Monday is April 21
        let easter_monday = Date::from_ymd(2025, 4, 21).unwrap();
        assert!(!cal.is_business_day(easter_monday));
    }

    #[test]
    fn test_target2_labour_day() {
        let cal = Target2Calendar::new();

        // 2025: May 1 is Thursday
        let labour_day = Date::from_ymd(2025, 5, 1).unwrap();
        assert!(!cal.is_business_day(labour_day));
    }

    #[test]
    fn test_target2_christmas() {
        let cal = Target2Calendar::new();

        // 2025: Dec 25 is Thursday
        let christmas = Date::from_ymd(2025, 12, 25).unwrap();
        assert!(!cal.is_business_day(christmas));

        // 2025: Dec 26 is Friday
        let boxing_day = Date::from_ymd(2025, 12, 26).unwrap();
        assert!(!cal.is_business_day(boxing_day));
    }

    #[test]
    fn test_target2_weekend() {
        let cal = Target2Calendar::new();

        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        let sunday = Date::from_ymd(2025, 1, 5).unwrap();
        let monday = Date::from_ymd(2025, 1, 6).unwrap();

        assert!(!cal.is_business_day(saturday));
        assert!(!cal.is_business_day(sunday));
        assert!(cal.is_business_day(monday));
    }

    #[test]
    fn test_target2_regular_business_day() {
        let cal = Target2Calendar::new();

        // Random business days
        let jan15 = Date::from_ymd(2025, 1, 15).unwrap();
        let mar10 = Date::from_ymd(2025, 3, 10).unwrap();

        assert!(cal.is_business_day(jan15));
        assert!(cal.is_business_day(mar10));
    }

    #[test]
    fn test_target2_global_instance() {
        let cal = Target2Calendar::global();
        let new_years = Date::from_ymd(2025, 1, 1).unwrap();
        assert!(!cal.is_business_day(new_years));
    }
}
