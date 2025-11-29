//! Japan (TSE/JGB) calendar.
//!
//! This calendar includes Japanese national holidays
//! relevant for JGB trading and Tokyo Stock Exchange.

use super::bitmap::{HolidayBitmap, HolidayCalendarBuilder, WeekendType, MAX_YEAR, MIN_YEAR};
use super::Calendar;
use crate::types::Date;
use chrono::{Datelike, NaiveDate};
use std::sync::OnceLock;

/// Static Japan calendar instance.
static JAPAN_CALENDAR: OnceLock<JapanCalendar> = OnceLock::new();

/// Japan calendar for JGB trading.
///
/// ## Holidays
///
/// - New Year's Day (January 1)
/// - New Year's Holiday (January 2-3)
/// - Coming of Age Day (2nd Monday in January)
/// - National Foundation Day (February 11)
/// - Emperor's Birthday (February 23, since 2020; Dec 23 before 2019)
/// - Vernal Equinox Day (~March 20-21)
/// - Showa Day (April 29)
/// - Constitution Memorial Day (May 3)
/// - Greenery Day (May 4)
/// - Children's Day (May 5)
/// - Marine Day (3rd Monday in July)
/// - Mountain Day (August 11, since 2016)
/// - Respect for the Aged Day (3rd Monday in September)
/// - Autumnal Equinox Day (~September 22-23)
/// - Sports Day (2nd Monday in October)
/// - Culture Day (November 3)
/// - Labour Thanksgiving Day (November 23)
///
/// ## Special Rules
///
/// - If a holiday falls on Sunday, the following Monday is a holiday (furikae kyujitsu)
/// - If two holidays are separated by one day, that day becomes a holiday (kokumin no kyujitsu)
#[derive(Debug, Clone)]
pub struct JapanCalendar {
    bitmap: HolidayBitmap,
}

impl JapanCalendar {
    /// Create a new Japan calendar.
    pub fn new() -> Self {
        Self {
            bitmap: build_japan_holidays(),
        }
    }

    /// Get the global Japan calendar instance.
    pub fn global() -> &'static JapanCalendar {
        JAPAN_CALENDAR.get_or_init(JapanCalendar::new)
    }
}

impl Default for JapanCalendar {
    fn default() -> Self {
        Self::new()
    }
}

impl Calendar for JapanCalendar {
    fn name(&self) -> &'static str {
        "Japan"
    }

    fn is_business_day(&self, date: Date) -> bool {
        self.bitmap.is_business_day(date.as_naive_date())
    }
}

/// Build the Japan holiday bitmap.
fn build_japan_holidays() -> HolidayBitmap {
    HolidayCalendarBuilder::new("Japan")
        .weekend(WeekendType::SaturdaySunday)
        .year_range(MIN_YEAR, MAX_YEAR)
        .add_custom_holidays(japan_holidays_for_year)
        .build()
}

/// Generate all Japan holidays for a given year.
fn japan_holidays_for_year(year: i32) -> Vec<NaiveDate> {
    let mut holidays = Vec::new();

    // New Year's Holidays (January 1-3)
    for day in 1..=3 {
        if let Some(d) = NaiveDate::from_ymd_opt(year, 1, day) {
            holidays.push(d);
        }
    }

    // Coming of Age Day (2nd Monday in January)
    if let Some(d) = super::bitmap::nth_weekday_of_month(year, 1, chrono::Weekday::Mon, 2) {
        holidays.push(d);
    }

    // National Foundation Day (February 11)
    if let Some(d) = NaiveDate::from_ymd_opt(year, 2, 11) {
        holidays.push(d);
    }

    // Emperor's Birthday
    if year >= 2020 {
        // February 23 (Emperor Naruhito)
        if let Some(d) = NaiveDate::from_ymd_opt(year, 2, 23) {
            holidays.push(d);
        }
    } else if year >= 1989 && year <= 2018 {
        // December 23 (Emperor Akihito)
        if let Some(d) = NaiveDate::from_ymd_opt(year, 12, 23) {
            holidays.push(d);
        }
    }

    // Vernal Equinox Day (around March 20-21)
    let vernal_day = calculate_vernal_equinox(year);
    if let Some(d) = NaiveDate::from_ymd_opt(year, 3, vernal_day) {
        holidays.push(d);
    }

    // Showa Day (April 29)
    if let Some(d) = NaiveDate::from_ymd_opt(year, 4, 29) {
        holidays.push(d);
    }

    // Constitution Memorial Day (May 3)
    if let Some(d) = NaiveDate::from_ymd_opt(year, 5, 3) {
        holidays.push(d);
    }

    // Greenery Day (May 4)
    if let Some(d) = NaiveDate::from_ymd_opt(year, 5, 4) {
        holidays.push(d);
    }

    // Children's Day (May 5)
    if let Some(d) = NaiveDate::from_ymd_opt(year, 5, 5) {
        holidays.push(d);
    }

    // Marine Day (3rd Monday in July; July 23 in 2020, July 22 in 2021 for Olympics)
    match year {
        2020 => {
            if let Some(d) = NaiveDate::from_ymd_opt(2020, 7, 23) {
                holidays.push(d);
            }
        }
        2021 => {
            if let Some(d) = NaiveDate::from_ymd_opt(2021, 7, 22) {
                holidays.push(d);
            }
        }
        _ => {
            if let Some(d) =
                super::bitmap::nth_weekday_of_month(year, 7, chrono::Weekday::Mon, 3)
            {
                holidays.push(d);
            }
        }
    }

    // Mountain Day (August 11, since 2016; moved for Olympics in 2020/2021)
    if year >= 2016 {
        match year {
            2020 => {
                if let Some(d) = NaiveDate::from_ymd_opt(2020, 8, 10) {
                    holidays.push(d);
                }
            }
            2021 => {
                if let Some(d) = NaiveDate::from_ymd_opt(2021, 8, 8) {
                    holidays.push(d);
                }
            }
            _ => {
                if let Some(d) = NaiveDate::from_ymd_opt(year, 8, 11) {
                    holidays.push(d);
                }
            }
        }
    }

    // Respect for the Aged Day (3rd Monday in September)
    if let Some(d) = super::bitmap::nth_weekday_of_month(year, 9, chrono::Weekday::Mon, 3) {
        holidays.push(d);
    }

    // Autumnal Equinox Day (around September 22-23)
    let autumnal_day = calculate_autumnal_equinox(year);
    if let Some(d) = NaiveDate::from_ymd_opt(year, 9, autumnal_day) {
        holidays.push(d);
    }

    // Sports Day (2nd Monday in October; July 24 in 2020, July 23 in 2021 for Olympics)
    match year {
        2020 => {
            if let Some(d) = NaiveDate::from_ymd_opt(2020, 7, 24) {
                holidays.push(d);
            }
        }
        2021 => {
            if let Some(d) = NaiveDate::from_ymd_opt(2021, 7, 23) {
                holidays.push(d);
            }
        }
        _ => {
            if let Some(d) =
                super::bitmap::nth_weekday_of_month(year, 10, chrono::Weekday::Mon, 2)
            {
                holidays.push(d);
            }
        }
    }

    // Culture Day (November 3)
    if let Some(d) = NaiveDate::from_ymd_opt(year, 11, 3) {
        holidays.push(d);
    }

    // Labour Thanksgiving Day (November 23)
    if let Some(d) = NaiveDate::from_ymd_opt(year, 11, 23) {
        holidays.push(d);
    }

    // Special holidays
    add_special_holidays(&mut holidays, year);

    // Apply furikae kyujitsu (substitute holiday when holiday falls on Sunday)
    apply_substitute_holidays(&mut holidays, year);

    holidays
}

/// Calculate vernal equinox day for a given year.
/// Uses an approximation formula.
fn calculate_vernal_equinox(year: i32) -> u32 {
    // Simplified formula: 20.8431 + 0.242194 * (year - 1980) - floor((year - 1980) / 4)
    let y = (year - 1980) as f64;
    let day = 20.8431 + 0.242194 * y - (y / 4.0).floor();
    day as u32
}

/// Calculate autumnal equinox day for a given year.
/// Uses an approximation formula.
fn calculate_autumnal_equinox(year: i32) -> u32 {
    // Simplified formula: 23.2488 + 0.242194 * (year - 1980) - floor((year - 1980) / 4)
    let y = (year - 1980) as f64;
    let day = 23.2488 + 0.242194 * y - (y / 4.0).floor();
    day as u32
}

/// Add special one-off holidays.
fn add_special_holidays(holidays: &mut Vec<NaiveDate>, year: i32) {
    match year {
        // 2019: Emperor Akihito's abdication day and Emperor Naruhito's enthronement
        2019 => {
            // April 30: Showa day bridge
            if let Some(d) = NaiveDate::from_ymd_opt(2019, 4, 30) {
                holidays.push(d);
            }
            // May 1: New Emperor's first day
            if let Some(d) = NaiveDate::from_ymd_opt(2019, 5, 1) {
                holidays.push(d);
            }
            // May 2: Bridge day
            if let Some(d) = NaiveDate::from_ymd_opt(2019, 5, 2) {
                holidays.push(d);
            }
            // October 22: Emperor's enthronement ceremony
            if let Some(d) = NaiveDate::from_ymd_opt(2019, 10, 22) {
                holidays.push(d);
            }
        }
        _ => {}
    }
}

/// Apply furikae kyujitsu (substitute holiday) rule.
/// If a holiday falls on Sunday, the following Monday is a holiday.
fn apply_substitute_holidays(holidays: &mut Vec<NaiveDate>, _year: i32) {
    let mut substitutes = Vec::new();

    for &holiday in holidays.iter() {
        if holiday.weekday() == chrono::Weekday::Sun {
            // Find the next non-holiday weekday
            let mut substitute = holiday;
            loop {
                substitute = substitute.succ_opt().unwrap_or(substitute);
                if substitute.weekday() != chrono::Weekday::Sun
                    && !holidays.contains(&substitute)
                    && !substitutes.contains(&substitute)
                {
                    substitutes.push(substitute);
                    break;
                }
            }
        }
    }

    holidays.extend(substitutes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_japan_new_years() {
        let cal = JapanCalendar::new();

        // 2025: Jan 1-3 are holidays
        let jan1 = Date::from_ymd(2025, 1, 1).unwrap();
        let jan2 = Date::from_ymd(2025, 1, 2).unwrap();
        let jan3 = Date::from_ymd(2025, 1, 3).unwrap();

        assert!(!cal.is_business_day(jan1));
        assert!(!cal.is_business_day(jan2));
        assert!(!cal.is_business_day(jan3));
    }

    #[test]
    fn test_japan_coming_of_age_day() {
        let cal = JapanCalendar::new();

        // 2025: 2nd Monday in January is Jan 13
        let coming_of_age = Date::from_ymd(2025, 1, 13).unwrap();
        assert!(!cal.is_business_day(coming_of_age));
    }

    #[test]
    fn test_japan_national_foundation_day() {
        let cal = JapanCalendar::new();

        // February 11
        let foundation_day = Date::from_ymd(2025, 2, 11).unwrap();
        assert!(!cal.is_business_day(foundation_day));
    }

    #[test]
    fn test_japan_emperor_birthday() {
        let cal = JapanCalendar::new();

        // 2025: February 23
        let emperor_bday = Date::from_ymd(2025, 2, 23).unwrap();
        assert!(!cal.is_business_day(emperor_bday));

        // 2018: December 23 (Emperor Akihito)
        let emperor_bday_2018 = Date::from_ymd(2018, 12, 23).unwrap();
        assert!(!cal.is_business_day(emperor_bday_2018));
    }

    #[test]
    fn test_japan_golden_week() {
        let cal = JapanCalendar::new();

        // 2025: Golden Week
        let showa_day = Date::from_ymd(2025, 4, 29).unwrap();
        let constitution_day = Date::from_ymd(2025, 5, 3).unwrap();
        let greenery_day = Date::from_ymd(2025, 5, 4).unwrap();
        let children_day = Date::from_ymd(2025, 5, 5).unwrap();

        assert!(!cal.is_business_day(showa_day));
        assert!(!cal.is_business_day(constitution_day));
        assert!(!cal.is_business_day(greenery_day));
        assert!(!cal.is_business_day(children_day));
    }

    #[test]
    fn test_japan_marine_day() {
        let cal = JapanCalendar::new();

        // 2025: 3rd Monday in July is July 21
        let marine_day = Date::from_ymd(2025, 7, 21).unwrap();
        assert!(!cal.is_business_day(marine_day));
    }

    #[test]
    fn test_japan_mountain_day() {
        let cal = JapanCalendar::new();

        // 2025: August 11
        let mountain_day = Date::from_ymd(2025, 8, 11).unwrap();
        assert!(!cal.is_business_day(mountain_day));
    }

    #[test]
    fn test_japan_respect_aged_day() {
        let cal = JapanCalendar::new();

        // 2025: 3rd Monday in September is Sept 15
        let respect_day = Date::from_ymd(2025, 9, 15).unwrap();
        assert!(!cal.is_business_day(respect_day));
    }

    #[test]
    fn test_japan_culture_day() {
        let cal = JapanCalendar::new();

        // November 3
        let culture_day = Date::from_ymd(2025, 11, 3).unwrap();
        assert!(!cal.is_business_day(culture_day));
    }

    #[test]
    fn test_japan_labour_thanksgiving() {
        let cal = JapanCalendar::new();

        // November 23
        let labour_day = Date::from_ymd(2025, 11, 23).unwrap();
        assert!(!cal.is_business_day(labour_day));
    }

    #[test]
    fn test_japan_weekend() {
        let cal = JapanCalendar::new();

        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        let sunday = Date::from_ymd(2025, 1, 5).unwrap();
        let monday = Date::from_ymd(2025, 1, 6).unwrap();

        assert!(!cal.is_business_day(saturday));
        assert!(!cal.is_business_day(sunday));
        assert!(cal.is_business_day(monday));
    }

    #[test]
    fn test_japan_substitute_holiday() {
        let cal = JapanCalendar::new();

        // When Culture Day (Nov 3) falls on Sunday, Monday is a substitute holiday
        // Nov 3, 2024 is Sunday, so Nov 4, 2024 is substitute
        let culture_day_2024 = Date::from_ymd(2024, 11, 3).unwrap();
        let substitute_2024 = Date::from_ymd(2024, 11, 4).unwrap();

        assert!(!cal.is_business_day(culture_day_2024)); // Sunday anyway
        assert!(!cal.is_business_day(substitute_2024)); // Substitute holiday
    }
}
