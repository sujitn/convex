//! US Federal Reserve and SIFMA calendars.

use chrono::Datelike;

use super::Calendar;
use crate::types::Date;

/// US Federal Reserve calendar for government securities.
///
/// Includes all US Federal holidays observed by the bond market.
#[derive(Debug, Clone, Copy, Default)]
pub struct USCalendar;

impl USCalendar {
    /// Returns true if the date is a US Federal holiday.
    fn is_federal_holiday(&self, date: Date) -> bool {
        let year = date.year();
        let month = date.month();
        let day = date.day();

        // Check fixed holidays first
        match (month, day) {
            // New Year's Day (Jan 1, or observed)
            (1, 1) => return true,
            (12, 31) if date.as_naive_date().weekday() == chrono::Weekday::Fri => return true,
            (1, 2) if date.as_naive_date().weekday() == chrono::Weekday::Mon => return true,

            // Juneteenth (Jun 19, or observed) - since 2021
            (6, 19) if year >= 2021 => return true,
            (6, 18) if year >= 2021 && date.as_naive_date().weekday() == chrono::Weekday::Fri => {
                return true
            }
            (6, 20) if year >= 2021 && date.as_naive_date().weekday() == chrono::Weekday::Mon => {
                return true
            }

            // Independence Day (Jul 4, or observed)
            (7, 4) => return true,
            (7, 3) if date.as_naive_date().weekday() == chrono::Weekday::Fri => return true,
            (7, 5) if date.as_naive_date().weekday() == chrono::Weekday::Mon => return true,

            // Veterans Day (Nov 11, or observed)
            (11, 11) => return true,
            (11, 10) if date.as_naive_date().weekday() == chrono::Weekday::Fri => return true,
            (11, 12) if date.as_naive_date().weekday() == chrono::Weekday::Mon => return true,

            // Christmas Day (Dec 25, or observed)
            (12, 25) => return true,
            (12, 24) if date.as_naive_date().weekday() == chrono::Weekday::Fri => return true,
            (12, 26) if date.as_naive_date().weekday() == chrono::Weekday::Mon => return true,

            _ => {}
        }

        // Check floating holidays (nth weekday of month)

        // MLK Day: 3rd Monday in January
        if month == 1 && is_nth_weekday(date, chrono::Weekday::Mon, 3) {
            return true;
        }

        // Presidents Day: 3rd Monday in February
        if month == 2 && is_nth_weekday(date, chrono::Weekday::Mon, 3) {
            return true;
        }

        // Memorial Day: Last Monday in May
        if month == 5 && is_last_weekday(date, chrono::Weekday::Mon) {
            return true;
        }

        // Labor Day: 1st Monday in September
        if month == 9 && is_nth_weekday(date, chrono::Weekday::Mon, 1) {
            return true;
        }

        // Columbus Day: 2nd Monday in October
        if month == 10 && is_nth_weekday(date, chrono::Weekday::Mon, 2) {
            return true;
        }

        // Thanksgiving: 4th Thursday in November
        if month == 11 && is_nth_weekday(date, chrono::Weekday::Thu, 4) {
            return true;
        }

        false
    }
}

impl Calendar for USCalendar {
    fn name(&self) -> &'static str {
        "US Federal Reserve"
    }

    fn is_business_day(&self, date: Date) -> bool {
        let weekday = date.as_naive_date().weekday();

        // Check weekend first
        if matches!(weekday, chrono::Weekday::Sat | chrono::Weekday::Sun) {
            return false;
        }

        // Check holidays
        !self.is_federal_holiday(date)
    }
}

/// Returns true if date is the nth occurrence of weekday in its month.
fn is_nth_weekday(date: Date, weekday: chrono::Weekday, n: u32) -> bool {
    if date.as_naive_date().weekday() != weekday {
        return false;
    }

    let day = date.day();
    let occurrence = (day - 1) / 7 + 1;
    occurrence == n
}

/// Returns true if date is the last occurrence of weekday in its month.
fn is_last_weekday(date: Date, weekday: chrono::Weekday) -> bool {
    if date.as_naive_date().weekday() != weekday {
        return false;
    }

    // Check if adding 7 days would go to next month
    let next_week = date.add_days(7);
    next_week.month() != date.month()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_us_weekend() {
        let cal = USCalendar;

        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        let sunday = Date::from_ymd(2025, 1, 5).unwrap();
        let monday = Date::from_ymd(2025, 1, 6).unwrap();

        assert!(!cal.is_business_day(saturday));
        assert!(!cal.is_business_day(sunday));
        assert!(cal.is_business_day(monday));
    }

    #[test]
    fn test_us_new_years() {
        let cal = USCalendar;

        // 2025: Jan 1 is Wednesday - holiday
        let new_years = Date::from_ymd(2025, 1, 1).unwrap();
        assert!(!cal.is_business_day(new_years));
    }

    #[test]
    fn test_us_mlk_day() {
        let cal = USCalendar;

        // 2025: MLK Day is Jan 20 (3rd Monday)
        let mlk_day = Date::from_ymd(2025, 1, 20).unwrap();
        assert!(!cal.is_business_day(mlk_day));

        // Day before and after should be business days
        let before = Date::from_ymd(2025, 1, 17).unwrap(); // Friday
        let after = Date::from_ymd(2025, 1, 21).unwrap(); // Tuesday

        assert!(cal.is_business_day(before));
        assert!(cal.is_business_day(after));
    }

    #[test]
    fn test_us_thanksgiving() {
        let cal = USCalendar;

        // 2025: Thanksgiving is Nov 27 (4th Thursday)
        let thanksgiving = Date::from_ymd(2025, 11, 27).unwrap();
        assert!(!cal.is_business_day(thanksgiving));
    }

    #[test]
    fn test_us_christmas() {
        let cal = USCalendar;

        let christmas = Date::from_ymd(2025, 12, 25).unwrap();
        assert!(!cal.is_business_day(christmas));
    }
}
