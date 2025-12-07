//! UK Bank Holidays calendar.
//!
//! This calendar includes all England and Wales bank holidays,
//! which are the relevant holidays for UK Gilt trading.

use super::bitmap::{HolidayBitmap, HolidayCalendarBuilder, WeekendType, MAX_YEAR, MIN_YEAR};
use super::Calendar;
use crate::types::Date;
use chrono::{Datelike, NaiveDate};
use std::sync::OnceLock;

/// Static UK calendar instance.
static UK_CALENDAR: OnceLock<UKCalendar> = OnceLock::new();

/// UK Bank Holidays calendar for Gilt trading.
///
/// ## Holidays
///
/// - New Year's Day (January 1, substitute if weekend)
/// - Good Friday
/// - Easter Monday
/// - Early May Bank Holiday (1st Monday in May)
/// - Spring Bank Holiday (Last Monday in May)
/// - Summer Bank Holiday (Last Monday in August)
/// - Christmas Day (December 25, substitute if weekend)
/// - Boxing Day (December 26, substitute if weekend)
///
/// ## Special Holidays
///
/// Additional one-off bank holidays (e.g., royal events) are included
/// for specific years where known.
#[derive(Debug, Clone)]
pub struct UKCalendar {
    bitmap: HolidayBitmap,
}

impl UKCalendar {
    /// Create a new UK calendar.
    pub fn new() -> Self {
        Self {
            bitmap: build_uk_holidays(),
        }
    }

    /// Get the global UK calendar instance.
    pub fn global() -> &'static UKCalendar {
        UK_CALENDAR.get_or_init(UKCalendar::new)
    }
}

impl Default for UKCalendar {
    fn default() -> Self {
        Self::new()
    }
}

impl Calendar for UKCalendar {
    fn name(&self) -> &'static str {
        "UK Bank Holidays"
    }

    fn is_business_day(&self, date: Date) -> bool {
        self.bitmap.is_business_day(date.as_naive_date())
    }
}

/// Build the UK holiday bitmap.
fn build_uk_holidays() -> HolidayBitmap {
    HolidayCalendarBuilder::new("UK Bank Holidays")
        .weekend(WeekendType::SaturdaySunday)
        .year_range(MIN_YEAR, MAX_YEAR)
        // New Year's Day (with UK-style substitute)
        .add_custom_holidays(uk_new_years)
        // Good Friday
        .add_easter_holiday(-2)
        // Easter Monday
        .add_easter_holiday(1)
        // Early May Bank Holiday (1st Monday in May)
        .add_custom_holidays(early_may_bank_holiday)
        // Spring Bank Holiday (Last Monday in May)
        .add_custom_holidays(spring_bank_holiday)
        // Summer Bank Holiday (Last Monday in August)
        .add_last_weekday_holiday(8, chrono::Weekday::Mon)
        // Christmas and Boxing Day (with UK-style substitute)
        .add_custom_holidays(uk_christmas)
        // Special one-off holidays
        .add_custom_holidays(special_uk_holidays)
        .build()
}

/// UK New Year's Day with substitute handling.
fn uk_new_years(year: i32) -> Vec<NaiveDate> {
    let mut holidays = Vec::new();

    if let Some(date) = NaiveDate::from_ymd_opt(year, 1, 1) {
        match date.weekday() {
            chrono::Weekday::Sat => {
                // Substitute is Monday Jan 3
                if let Some(sub) = NaiveDate::from_ymd_opt(year, 1, 3) {
                    holidays.push(sub);
                }
            }
            chrono::Weekday::Sun => {
                // Substitute is Monday Jan 2
                if let Some(sub) = NaiveDate::from_ymd_opt(year, 1, 2) {
                    holidays.push(sub);
                }
            }
            _ => {
                holidays.push(date);
            }
        }
    }

    holidays
}

/// Early May Bank Holiday - usually 1st Monday in May.
/// Exception: In 2020 it was moved to May 8 for VE Day 75th anniversary.
fn early_may_bank_holiday(year: i32) -> Vec<NaiveDate> {
    let mut holidays = Vec::new();

    if year == 2020 {
        // VE Day 75th anniversary - moved to May 8
        if let Some(date) = NaiveDate::from_ymd_opt(2020, 5, 8) {
            holidays.push(date);
        }
    } else {
        // 1st Monday in May
        if let Some(date) = super::bitmap::nth_weekday_of_month(year, 5, chrono::Weekday::Mon, 1) {
            holidays.push(date);
        }
    }

    holidays
}

/// Spring Bank Holiday - usually last Monday in May.
/// Exception: In 2022 it was moved to June 2 for Queen's Platinum Jubilee.
fn spring_bank_holiday(year: i32) -> Vec<NaiveDate> {
    let mut holidays = Vec::new();

    if year == 2022 {
        // Moved to June 2 for Platinum Jubilee
        if let Some(date) = NaiveDate::from_ymd_opt(2022, 6, 2) {
            holidays.push(date);
        }
    } else {
        // Last Monday in May
        if let Some(date) = super::bitmap::last_weekday_of_month(year, 5, chrono::Weekday::Mon) {
            holidays.push(date);
        }
    }

    holidays
}

/// UK Christmas and Boxing Day with substitute handling.
fn uk_christmas(year: i32) -> Vec<NaiveDate> {
    let mut holidays = Vec::new();

    let christmas = NaiveDate::from_ymd_opt(year, 12, 25);
    let boxing_day = NaiveDate::from_ymd_opt(year, 12, 26);

    match (christmas, boxing_day) {
        (Some(xmas), Some(box_day)) => {
            match xmas.weekday() {
                chrono::Weekday::Sat => {
                    // Christmas on Sat: substitute Mon Dec 27
                    // Boxing Day on Sun: substitute Tue Dec 28
                    if let Some(d) = NaiveDate::from_ymd_opt(year, 12, 27) {
                        holidays.push(d);
                    }
                    if let Some(d) = NaiveDate::from_ymd_opt(year, 12, 28) {
                        holidays.push(d);
                    }
                }
                chrono::Weekday::Sun => {
                    // Christmas on Sun: substitute Mon Dec 27
                    // Boxing Day on Mon: as-is
                    if let Some(d) = NaiveDate::from_ymd_opt(year, 12, 27) {
                        holidays.push(d);
                    }
                    holidays.push(box_day);
                }
                chrono::Weekday::Fri => {
                    // Christmas on Fri: as-is
                    // Boxing Day on Sat: substitute Mon Dec 28
                    holidays.push(xmas);
                    if let Some(d) = NaiveDate::from_ymd_opt(year, 12, 28) {
                        holidays.push(d);
                    }
                }
                _ => {
                    // Both fall on weekdays
                    holidays.push(xmas);
                    holidays.push(box_day);
                }
            }
        }
        _ => {}
    }

    holidays
}

/// Special one-off UK bank holidays.
fn special_uk_holidays(year: i32) -> Vec<NaiveDate> {
    let mut holidays = Vec::new();

    match year {
        // 2011: Royal Wedding (William and Kate)
        2011 => {
            if let Some(d) = NaiveDate::from_ymd_opt(2011, 4, 29) {
                holidays.push(d);
            }
        }
        // 2012: Queen's Diamond Jubilee
        2012 => {
            if let Some(d) = NaiveDate::from_ymd_opt(2012, 6, 5) {
                holidays.push(d);
            }
        }
        // 2022: Queen's Platinum Jubilee (extra day June 3)
        // Note: Spring Bank Holiday was moved to June 2
        2022 => {
            if let Some(d) = NaiveDate::from_ymd_opt(2022, 6, 3) {
                holidays.push(d);
            }
            // Queen Elizabeth II Funeral
            if let Some(d) = NaiveDate::from_ymd_opt(2022, 9, 19) {
                holidays.push(d);
            }
        }
        // 2023: King Charles III Coronation
        2023 => {
            if let Some(d) = NaiveDate::from_ymd_opt(2023, 5, 8) {
                holidays.push(d);
            }
        }
        _ => {}
    }

    holidays
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uk_new_years() {
        let cal = UKCalendar::new();

        // 2025: Jan 1 is Wednesday - holiday
        let new_years = Date::from_ymd(2025, 1, 1).unwrap();
        assert!(!cal.is_business_day(new_years));
    }

    #[test]
    fn test_uk_new_years_substitute() {
        let cal = UKCalendar::new();

        // 2028: Jan 1 is Saturday, substitute is Monday Jan 3
        let jan1_2028 = Date::from_ymd(2028, 1, 1).unwrap();
        assert!(!cal.is_business_day(jan1_2028)); // Weekend
        let jan3_2028 = Date::from_ymd(2028, 1, 3).unwrap();
        assert!(!cal.is_business_day(jan3_2028)); // Substitute holiday
    }

    #[test]
    fn test_uk_good_friday() {
        let cal = UKCalendar::new();

        // 2025: Good Friday is April 18
        let good_friday = Date::from_ymd(2025, 4, 18).unwrap();
        assert!(!cal.is_business_day(good_friday));
    }

    #[test]
    fn test_uk_easter_monday() {
        let cal = UKCalendar::new();

        // 2025: Easter Monday is April 21
        let easter_monday = Date::from_ymd(2025, 4, 21).unwrap();
        assert!(!cal.is_business_day(easter_monday));
    }

    #[test]
    fn test_uk_early_may_bank_holiday() {
        let cal = UKCalendar::new();

        // 2025: 1st Monday in May is May 5
        let early_may = Date::from_ymd(2025, 5, 5).unwrap();
        assert!(!cal.is_business_day(early_may));

        // 2020: Moved to May 8 for VE Day
        let ve_day_2020 = Date::from_ymd(2020, 5, 8).unwrap();
        assert!(!cal.is_business_day(ve_day_2020));
    }

    #[test]
    fn test_uk_spring_bank_holiday() {
        let cal = UKCalendar::new();

        // 2025: Last Monday in May is May 26
        let spring_bh = Date::from_ymd(2025, 5, 26).unwrap();
        assert!(!cal.is_business_day(spring_bh));

        // 2022: Moved to June 2 for Platinum Jubilee
        let jubilee_2022 = Date::from_ymd(2022, 6, 2).unwrap();
        assert!(!cal.is_business_day(jubilee_2022));
    }

    #[test]
    fn test_uk_summer_bank_holiday() {
        let cal = UKCalendar::new();

        // 2025: Last Monday in August is Aug 25
        let summer_bh = Date::from_ymd(2025, 8, 25).unwrap();
        assert!(!cal.is_business_day(summer_bh));
    }

    #[test]
    fn test_uk_christmas() {
        let cal = UKCalendar::new();

        // 2025: Dec 25 is Thursday, Dec 26 is Friday
        let christmas = Date::from_ymd(2025, 12, 25).unwrap();
        let boxing_day = Date::from_ymd(2025, 12, 26).unwrap();
        assert!(!cal.is_business_day(christmas));
        assert!(!cal.is_business_day(boxing_day));
    }

    #[test]
    fn test_uk_christmas_substitute() {
        let cal = UKCalendar::new();

        // 2027: Christmas on Saturday, Boxing Day on Sunday
        // Substitutes are Dec 27 (Mon) and Dec 28 (Tue)
        let dec27_2027 = Date::from_ymd(2027, 12, 27).unwrap();
        let dec28_2027 = Date::from_ymd(2027, 12, 28).unwrap();
        assert!(!cal.is_business_day(dec27_2027));
        assert!(!cal.is_business_day(dec28_2027));
    }

    #[test]
    fn test_uk_special_holidays() {
        let cal = UKCalendar::new();

        // 2022: Queen's Funeral (Sept 19)
        let funeral = Date::from_ymd(2022, 9, 19).unwrap();
        assert!(!cal.is_business_day(funeral));

        // 2023: King Charles Coronation (May 8)
        let coronation = Date::from_ymd(2023, 5, 8).unwrap();
        assert!(!cal.is_business_day(coronation));
    }

    #[test]
    fn test_uk_weekend() {
        let cal = UKCalendar::new();

        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        let sunday = Date::from_ymd(2025, 1, 5).unwrap();
        let monday = Date::from_ymd(2025, 1, 6).unwrap();

        assert!(!cal.is_business_day(saturday));
        assert!(!cal.is_business_day(sunday));
        assert!(cal.is_business_day(monday));
    }
}
