//! Business day calendars and conventions.
//!
//! This module provides:
//! - Business day calendars for different markets
//! - Business day adjustment conventions
//! - Holiday detection and date rolling

use chrono::Datelike;

mod conventions;
mod us_calendar;

pub use conventions::BusinessDayConvention;
pub use us_calendar::USCalendar;

use crate::error::ConvexResult;
use crate::types::Date;

/// Trait for business day calendars.
///
/// Calendars determine which days are business days vs holidays
/// for a specific market or jurisdiction.
pub trait Calendar: Send + Sync {
    /// Returns the name of the calendar.
    fn name(&self) -> &'static str;

    /// Returns true if the date is a business day.
    fn is_business_day(&self, date: Date) -> bool;

    /// Returns true if the date is a holiday.
    fn is_holiday(&self, date: Date) -> bool {
        !self.is_business_day(date)
    }

    /// Adjusts a date according to the given business day convention.
    fn adjust(&self, date: Date, convention: BusinessDayConvention) -> ConvexResult<Date> {
        conventions::adjust(date, convention, self)
    }

    /// Advances a date by a number of business days.
    fn add_business_days(&self, date: Date, days: i32) -> Date {
        let mut result = date;
        let mut remaining = days.abs();
        let direction: i64 = if days >= 0 { 1 } else { -1 };

        while remaining > 0 {
            result = result.add_days(direction);
            if self.is_business_day(result) {
                remaining -= 1;
            }
        }

        result
    }

    /// Returns the next business day on or after the given date.
    fn next_business_day(&self, date: Date) -> Date {
        let mut result = date;
        while !self.is_business_day(result) {
            result = result.add_days(1);
        }
        result
    }

    /// Returns the previous business day on or before the given date.
    fn previous_business_day(&self, date: Date) -> Date {
        let mut result = date;
        while !self.is_business_day(result) {
            result = result.add_days(-1);
        }
        result
    }

    /// Counts business days between two dates (exclusive of start, inclusive of end).
    fn business_days_between(&self, start: Date, end: Date) -> i32 {
        let mut count = 0;
        let mut current = start.add_days(1);

        while current <= end {
            if self.is_business_day(current) {
                count += 1;
            }
            current = current.add_days(1);
        }

        count
    }
}

/// A simple weekend-only calendar (no holidays).
///
/// Useful for testing or when holiday data is not available.
#[derive(Debug, Clone, Copy, Default)]
pub struct WeekendCalendar;

impl Calendar for WeekendCalendar {
    fn name(&self) -> &'static str {
        "Weekend Only"
    }

    fn is_business_day(&self, date: Date) -> bool {
        let weekday = date.as_naive_date().weekday();
        !matches!(
            weekday,
            chrono::Weekday::Sat | chrono::Weekday::Sun
        )
    }
}

/// Calendar that combines multiple calendars (joint holidays).
pub struct JointCalendar {
    calendars: Vec<Box<dyn Calendar>>,
    name: String,
}

impl JointCalendar {
    /// Creates a new joint calendar from multiple calendars.
    pub fn new(calendars: Vec<Box<dyn Calendar>>) -> Self {
        let name = calendars
            .iter()
            .map(|c| c.name())
            .collect::<Vec<_>>()
            .join(" + ");

        Self { calendars, name }
    }
}

impl Calendar for JointCalendar {
    fn name(&self) -> &'static str {
        // This is a limitation - we can't return a dynamic string
        "Joint Calendar"
    }

    fn is_business_day(&self, date: Date) -> bool {
        // Business day only if ALL calendars consider it a business day
        self.calendars.iter().all(|cal| cal.is_business_day(date))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weekend_calendar() {
        let cal = WeekendCalendar;

        // Monday
        let monday = Date::from_ymd(2025, 1, 6).unwrap();
        assert!(cal.is_business_day(monday));

        // Saturday
        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        assert!(!cal.is_business_day(saturday));

        // Sunday
        let sunday = Date::from_ymd(2025, 1, 5).unwrap();
        assert!(!cal.is_business_day(sunday));
    }

    #[test]
    fn test_add_business_days() {
        let cal = WeekendCalendar;

        // Friday + 1 business day = Monday
        let friday = Date::from_ymd(2025, 1, 3).unwrap();
        let result = cal.add_business_days(friday, 1);
        assert_eq!(result, Date::from_ymd(2025, 1, 6).unwrap());
    }

    #[test]
    fn test_business_days_between() {
        let cal = WeekendCalendar;

        // Monday to Friday = 4 business days (Tue, Wed, Thu, Fri)
        let monday = Date::from_ymd(2025, 1, 6).unwrap();
        let friday = Date::from_ymd(2025, 1, 10).unwrap();

        assert_eq!(cal.business_days_between(monday, friday), 4);
    }
}
