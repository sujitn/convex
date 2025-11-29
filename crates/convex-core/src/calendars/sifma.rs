//! SIFMA (Securities Industry and Financial Markets Association) calendar.
//!
//! The SIFMA calendar is the standard for US fixed income markets.
//! It differs slightly from the Federal Reserve calendar.

use super::bitmap::{HolidayBitmap, HolidayCalendarBuilder, WeekendType, MAX_YEAR, MIN_YEAR};
use super::Calendar;
use crate::types::Date;
use std::sync::OnceLock;

/// Static SIFMA calendar instance.
static SIFMA_CALENDAR: OnceLock<SIFMACalendar> = OnceLock::new();

/// SIFMA holiday calendar for US fixed income markets.
///
/// ## Holidays
///
/// - New Year's Day (January 1, observed)
/// - Martin Luther King Jr. Day (3rd Monday in January)
/// - Presidents' Day (3rd Monday in February)
/// - Good Friday (Friday before Easter)
/// - Memorial Day (Last Monday in May)
/// - Juneteenth (June 19, observed) - since 2021
/// - Independence Day (July 4, observed)
/// - Labor Day (1st Monday in September)
/// - Columbus Day (2nd Monday in October)
/// - Veterans Day (November 11, observed)
/// - Thanksgiving Day (4th Thursday in November)
/// - Christmas Day (December 25, observed)
///
/// ## Early Closes (not modeled as full holidays)
///
/// - Day before Independence Day (if July 4 is weekday)
/// - Day after Thanksgiving
/// - Christmas Eve (December 24)
#[derive(Debug, Clone)]
pub struct SIFMACalendar {
    bitmap: HolidayBitmap,
}

impl SIFMACalendar {
    /// Create a new SIFMA calendar.
    pub fn new() -> Self {
        Self {
            bitmap: build_sifma_holidays(),
        }
    }

    /// Get the global SIFMA calendar instance.
    ///
    /// This is more efficient than creating a new calendar each time.
    pub fn global() -> &'static SIFMACalendar {
        SIFMA_CALENDAR.get_or_init(SIFMACalendar::new)
    }
}

impl Default for SIFMACalendar {
    fn default() -> Self {
        Self::new()
    }
}

impl Calendar for SIFMACalendar {
    fn name(&self) -> &'static str {
        "SIFMA"
    }

    fn is_business_day(&self, date: Date) -> bool {
        self.bitmap.is_business_day(date.as_naive_date())
    }
}

/// Build the SIFMA holiday bitmap.
fn build_sifma_holidays() -> HolidayBitmap {
    HolidayCalendarBuilder::new("SIFMA")
        .weekend(WeekendType::SaturdaySunday)
        .year_range(MIN_YEAR, MAX_YEAR)
        // New Year's Day (January 1)
        .add_fixed_holiday(1, 1, true)
        // Martin Luther King Jr. Day (3rd Monday in January)
        .add_nth_weekday_holiday(1, chrono::Weekday::Mon, 3)
        // Presidents' Day (3rd Monday in February)
        .add_nth_weekday_holiday(2, chrono::Weekday::Mon, 3)
        // Good Friday (Friday before Easter)
        .add_easter_holiday(-2)
        // Memorial Day (Last Monday in May)
        .add_last_weekday_holiday(5, chrono::Weekday::Mon)
        // Juneteenth (June 19) - observed since 2021
        .add_fixed_holiday_from(6, 19, 2021, true)
        // Independence Day (July 4)
        .add_fixed_holiday(7, 4, true)
        // Labor Day (1st Monday in September)
        .add_nth_weekday_holiday(9, chrono::Weekday::Mon, 1)
        // Columbus Day (2nd Monday in October)
        .add_nth_weekday_holiday(10, chrono::Weekday::Mon, 2)
        // Veterans Day (November 11)
        .add_fixed_holiday(11, 11, true)
        // Thanksgiving Day (4th Thursday in November)
        .add_nth_weekday_holiday(11, chrono::Weekday::Thu, 4)
        // Christmas Day (December 25)
        .add_fixed_holiday(12, 25, true)
        .build()
}

/// US Government Securities calendar.
///
/// Similar to SIFMA but may have slight differences for Treasury operations.
/// Currently identical to SIFMA.
#[derive(Debug, Clone)]
pub struct USGovernmentCalendar {
    bitmap: HolidayBitmap,
}

impl USGovernmentCalendar {
    /// Create a new US Government calendar.
    pub fn new() -> Self {
        Self {
            bitmap: build_us_government_holidays(),
        }
    }
}

impl Default for USGovernmentCalendar {
    fn default() -> Self {
        Self::new()
    }
}

impl Calendar for USGovernmentCalendar {
    fn name(&self) -> &'static str {
        "US Government"
    }

    fn is_business_day(&self, date: Date) -> bool {
        self.bitmap.is_business_day(date.as_naive_date())
    }
}

/// Build US Government holiday bitmap.
/// Same as SIFMA but includes Good Friday as a holiday.
fn build_us_government_holidays() -> HolidayBitmap {
    // US Government securities calendar is essentially the same as SIFMA
    build_sifma_holidays()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sifma_new_years_2025() {
        let cal = SIFMACalendar::new();

        // 2025: Jan 1 is Wednesday - holiday
        let new_years = Date::from_ymd(2025, 1, 1).unwrap();
        assert!(!cal.is_business_day(new_years));

        // Jan 2 is Thursday - business day
        let jan2 = Date::from_ymd(2025, 1, 2).unwrap();
        assert!(cal.is_business_day(jan2));
    }

    #[test]
    fn test_sifma_mlk_day() {
        let cal = SIFMACalendar::new();

        // 2025: MLK Day is Jan 20 (3rd Monday)
        let mlk_day = Date::from_ymd(2025, 1, 20).unwrap();
        assert!(!cal.is_business_day(mlk_day));

        // Day before (Friday) and after (Tuesday) should be business days
        let before = Date::from_ymd(2025, 1, 17).unwrap();
        let after = Date::from_ymd(2025, 1, 21).unwrap();
        assert!(cal.is_business_day(before));
        assert!(cal.is_business_day(after));
    }

    #[test]
    fn test_sifma_good_friday() {
        let cal = SIFMACalendar::new();

        // 2025: Easter is April 20, Good Friday is April 18
        let good_friday = Date::from_ymd(2025, 4, 18).unwrap();
        assert!(!cal.is_business_day(good_friday));
    }

    #[test]
    fn test_sifma_memorial_day() {
        let cal = SIFMACalendar::new();

        // 2025: Memorial Day is May 26 (last Monday)
        let memorial_day = Date::from_ymd(2025, 5, 26).unwrap();
        assert!(!cal.is_business_day(memorial_day));
    }

    #[test]
    fn test_sifma_juneteenth() {
        let cal = SIFMACalendar::new();

        // 2025: Juneteenth is June 19 (Thursday)
        let juneteenth = Date::from_ymd(2025, 6, 19).unwrap();
        assert!(!cal.is_business_day(juneteenth));

        // 2020: Juneteenth was not yet a federal holiday
        let juneteenth_2020 = Date::from_ymd(2020, 6, 19).unwrap();
        assert!(cal.is_business_day(juneteenth_2020));
    }

    #[test]
    fn test_sifma_independence_day() {
        let cal = SIFMACalendar::new();

        // 2025: July 4 is Friday - holiday
        let july4 = Date::from_ymd(2025, 7, 4).unwrap();
        assert!(!cal.is_business_day(july4));

        // 2026: July 4 is Saturday, observed on Friday July 3
        let july4_2026 = Date::from_ymd(2026, 7, 4).unwrap();
        assert!(!cal.is_business_day(july4_2026)); // Saturday anyway
        let july3_2026 = Date::from_ymd(2026, 7, 3).unwrap();
        assert!(!cal.is_business_day(july3_2026)); // Observed
    }

    #[test]
    fn test_sifma_thanksgiving() {
        let cal = SIFMACalendar::new();

        // 2025: Thanksgiving is Nov 27 (4th Thursday)
        let thanksgiving = Date::from_ymd(2025, 11, 27).unwrap();
        assert!(!cal.is_business_day(thanksgiving));
    }

    #[test]
    fn test_sifma_christmas() {
        let cal = SIFMACalendar::new();

        // 2025: Christmas is Dec 25 (Thursday)
        let christmas = Date::from_ymd(2025, 12, 25).unwrap();
        assert!(!cal.is_business_day(christmas));
    }

    #[test]
    fn test_sifma_weekend() {
        let cal = SIFMACalendar::new();

        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        let sunday = Date::from_ymd(2025, 1, 5).unwrap();
        let monday = Date::from_ymd(2025, 1, 6).unwrap();

        assert!(!cal.is_business_day(saturday));
        assert!(!cal.is_business_day(sunday));
        assert!(cal.is_business_day(monday));
    }

    #[test]
    fn test_sifma_global_instance() {
        let cal = SIFMACalendar::global();
        let new_years = Date::from_ymd(2025, 1, 1).unwrap();
        assert!(!cal.is_business_day(new_years));
    }
}
