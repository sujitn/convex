//! Date type for financial calculations.

use chrono::{Datelike, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, Sub};

use crate::error::{ConvexError, ConvexResult};

/// A calendar date for financial calculations.
///
/// This is a newtype wrapper around `chrono::NaiveDate` providing
/// financial-specific operations and ensuring type safety.
///
/// # Example
///
/// ```rust
/// use convex_core::types::Date;
///
/// let date = Date::from_ymd(2025, 6, 15).unwrap();
/// let future = date.add_months(6).unwrap();
/// assert_eq!(future.year(), 2025);
/// assert_eq!(future.month(), 12);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Date(NaiveDate);

impl Date {
    /// Creates a new date from year, month, and day.
    ///
    /// # Errors
    ///
    /// Returns `ConvexError::InvalidDate` if the date is invalid.
    pub fn from_ymd(year: i32, month: u32, day: u32) -> ConvexResult<Self> {
        NaiveDate::from_ymd_opt(year, month, day)
            .map(Date)
            .ok_or_else(|| ConvexError::invalid_date(format!("{year}-{month:02}-{day:02}")))
    }

    /// Creates a date from an ISO 8601 string (YYYY-MM-DD).
    ///
    /// # Errors
    ///
    /// Returns `ConvexError::InvalidDate` if the string is not a valid date.
    pub fn parse(s: &str) -> ConvexResult<Self> {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Date)
            .map_err(|_| ConvexError::invalid_date(format!("Cannot parse: {s}")))
    }

    /// Returns today's date.
    #[must_use]
    pub fn today() -> Self {
        Date(chrono::Local::now().date_naive())
    }

    /// Returns the year component.
    #[must_use]
    pub fn year(&self) -> i32 {
        self.0.year()
    }

    /// Returns the month component (1-12).
    #[must_use]
    pub fn month(&self) -> u32 {
        self.0.month()
    }

    /// Returns the day component (1-31).
    #[must_use]
    pub fn day(&self) -> u32 {
        self.0.day()
    }

    /// Returns the day of year (1-366).
    #[must_use]
    pub fn day_of_year(&self) -> u32 {
        self.0.ordinal()
    }

    /// Checks if the year is a leap year.
    #[must_use]
    pub fn is_leap_year(&self) -> bool {
        self.0.leap_year()
    }

    /// Returns the number of days in the date's month.
    #[must_use]
    pub fn days_in_month(&self) -> u32 {
        match self.month() {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if self.is_leap_year() => 29,
            2 => 28,
            _ => unreachable!(),
        }
    }

    /// Returns the number of days in the date's year.
    #[must_use]
    pub fn days_in_year(&self) -> u32 {
        if self.is_leap_year() {
            366
        } else {
            365
        }
    }

    /// Adds a number of days to the date.
    #[must_use]
    pub fn add_days(&self, days: i64) -> Self {
        Date(self.0 + chrono::Duration::days(days))
    }

    /// Adds a number of months to the date.
    ///
    /// If the resulting day would be invalid (e.g., Jan 31 + 1 month),
    /// it rolls back to the last valid day of the month.
    ///
    /// # Errors
    ///
    /// Returns `ConvexError::InvalidDate` if the result is out of range.
    pub fn add_months(&self, months: i32) -> ConvexResult<Self> {
        let total_months = self.year() * 12 + self.month() as i32 - 1 + months;
        let new_year = total_months / 12;
        let new_month = (total_months % 12 + 1) as u32;

        // Clamp day to valid range for new month
        let max_day = days_in_month(new_year, new_month);
        let new_day = self.day().min(max_day);

        Self::from_ymd(new_year, new_month, new_day)
    }

    /// Adds a number of years to the date.
    ///
    /// # Errors
    ///
    /// Returns `ConvexError::InvalidDate` if the result is invalid.
    pub fn add_years(&self, years: i32) -> ConvexResult<Self> {
        let new_year = self.year() + years;
        let max_day = days_in_month(new_year, self.month());
        let new_day = self.day().min(max_day);

        Self::from_ymd(new_year, self.month(), new_day)
    }

    /// Calculates the number of calendar days between two dates.
    #[must_use]
    pub fn days_between(&self, other: &Date) -> i64 {
        (other.0 - self.0).num_days()
    }

    /// Returns the underlying `NaiveDate`.
    #[must_use]
    pub fn as_naive_date(&self) -> NaiveDate {
        self.0
    }

    /// Returns the end of month for the current date.
    #[must_use]
    pub fn end_of_month(&self) -> Self {
        Date(
            NaiveDate::from_ymd_opt(self.year(), self.month(), self.days_in_month())
                .expect("end of month should always be valid"),
        )
    }

    /// Checks if the date is the end of month.
    #[must_use]
    pub fn is_end_of_month(&self) -> bool {
        self.day() == self.days_in_month()
    }

    /// Returns the day of week.
    #[must_use]
    pub fn weekday(&self) -> Weekday {
        self.0.weekday()
    }

    /// Checks if the date is a weekend (Saturday or Sunday).
    #[must_use]
    pub fn is_weekend(&self) -> bool {
        matches!(self.weekday(), Weekday::Sat | Weekday::Sun)
    }

    /// Checks if the date is a weekday (Monday through Friday).
    #[must_use]
    pub fn is_weekday(&self) -> bool {
        !self.is_weekend()
    }

    /// Returns the next weekday (skipping weekends).
    ///
    /// If the date is already a weekday, returns itself.
    #[must_use]
    pub fn next_weekday(&self) -> Self {
        let mut date = *self;
        while date.is_weekend() {
            date = date.add_days(1);
        }
        date
    }

    /// Returns the previous weekday (skipping weekends).
    ///
    /// If the date is already a weekday, returns itself.
    #[must_use]
    pub fn prev_weekday(&self) -> Self {
        let mut date = *self;
        while date.is_weekend() {
            date = date.add_days(-1);
        }
        date
    }

    /// Adds business days (weekdays only) to the date.
    ///
    /// Positive values move forward, negative values move backward.
    #[must_use]
    pub fn add_business_days(&self, days: i32) -> Self {
        if days == 0 {
            return *self;
        }

        let direction = if days > 0 { 1i64 } else { -1i64 };
        let mut remaining = days.abs();
        let mut current = *self;

        while remaining > 0 {
            current = current.add_days(direction);
            if current.is_weekday() {
                remaining -= 1;
            }
        }

        current
    }

    /// Calculates the number of business days between two dates.
    ///
    /// Returns positive if `other` is after `self`, negative otherwise.
    #[must_use]
    pub fn business_days_between(&self, other: &Date) -> i64 {
        if self == other {
            return 0;
        }

        let (start, end, sign) = if self < other {
            (*self, *other, 1i64)
        } else {
            (*other, *self, -1i64)
        };

        let mut count = 0i64;
        let mut current = start.add_days(1);

        while current <= end {
            if current.is_weekday() {
                count += 1;
            }
            current = current.add_days(1);
        }

        count * sign
    }

    /// Returns the first day of the month.
    #[must_use]
    pub fn start_of_month(&self) -> Self {
        Date(
            NaiveDate::from_ymd_opt(self.year(), self.month(), 1)
                .expect("first of month should always be valid"),
        )
    }

    /// Returns the first day of the year.
    #[must_use]
    pub fn start_of_year(&self) -> Self {
        Date(
            NaiveDate::from_ymd_opt(self.year(), 1, 1)
                .expect("first of year should always be valid"),
        )
    }

    /// Returns the last day of the year.
    #[must_use]
    pub fn end_of_year(&self) -> Self {
        Date(
            NaiveDate::from_ymd_opt(self.year(), 12, 31)
                .expect("last of year should always be valid"),
        )
    }

    /// Returns the minimum of two dates.
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        if self <= other {
            self
        } else {
            other
        }
    }

    /// Returns the maximum of two dates.
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        if self >= other {
            self
        } else {
            other
        }
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.format("%Y-%m-%d"))
    }
}

impl From<NaiveDate> for Date {
    fn from(date: NaiveDate) -> Self {
        Date(date)
    }
}

impl From<Date> for NaiveDate {
    fn from(date: Date) -> Self {
        date.0
    }
}

impl Add<i64> for Date {
    type Output = Self;

    /// Adds days to a date.
    fn add(self, days: i64) -> Self::Output {
        self.add_days(days)
    }
}

impl Sub<i64> for Date {
    type Output = Self;

    /// Subtracts days from a date.
    fn sub(self, days: i64) -> Self::Output {
        self.add_days(-days)
    }
}

impl Sub<Date> for Date {
    type Output = i64;

    /// Returns the number of days between two dates.
    fn sub(self, other: Date) -> Self::Output {
        other.days_between(&self)
    }
}

/// Helper function to get days in a month for a given year.
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => panic!("Invalid month: {month}"),
    }
}

/// Helper function to check if a year is a leap year.
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_creation() {
        let date = Date::from_ymd(2025, 6, 15).unwrap();
        assert_eq!(date.year(), 2025);
        assert_eq!(date.month(), 6);
        assert_eq!(date.day(), 15);
    }

    #[test]
    fn test_invalid_date() {
        assert!(Date::from_ymd(2025, 2, 30).is_err());
        assert!(Date::from_ymd(2025, 13, 1).is_err());
    }

    #[test]
    fn test_add_months() {
        let date = Date::from_ymd(2025, 1, 31).unwrap();
        let result = date.add_months(1).unwrap();
        assert_eq!(result.month(), 2);
        assert_eq!(result.day(), 28); // Rolled back to last valid day
    }

    #[test]
    fn test_leap_year() {
        assert!(Date::from_ymd(2024, 1, 1).unwrap().is_leap_year());
        assert!(!Date::from_ymd(2025, 1, 1).unwrap().is_leap_year());
        assert!(!Date::from_ymd(2100, 1, 1).unwrap().is_leap_year());
        assert!(Date::from_ymd(2000, 1, 1).unwrap().is_leap_year());
    }

    #[test]
    fn test_days_between() {
        let d1 = Date::from_ymd(2025, 1, 1).unwrap();
        let d2 = Date::from_ymd(2025, 1, 31).unwrap();
        assert_eq!(d1.days_between(&d2), 30);
    }

    #[test]
    fn test_parse() {
        let date = Date::parse("2025-06-15").unwrap();
        assert_eq!(date.year(), 2025);
        assert_eq!(date.month(), 6);
        assert_eq!(date.day(), 15);
    }

    #[test]
    fn test_weekday_detection() {
        // Monday
        let monday = Date::from_ymd(2025, 1, 6).unwrap();
        assert!(monday.is_weekday());
        assert!(!monday.is_weekend());
        assert_eq!(monday.weekday(), Weekday::Mon);

        // Saturday
        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        assert!(!saturday.is_weekday());
        assert!(saturday.is_weekend());
        assert_eq!(saturday.weekday(), Weekday::Sat);

        // Sunday
        let sunday = Date::from_ymd(2025, 1, 5).unwrap();
        assert!(!sunday.is_weekday());
        assert!(sunday.is_weekend());
        assert_eq!(sunday.weekday(), Weekday::Sun);
    }

    #[test]
    fn test_next_weekday() {
        // Friday -> Friday (no change)
        let friday = Date::from_ymd(2025, 1, 3).unwrap();
        assert_eq!(friday.next_weekday(), friday);

        // Saturday -> Monday
        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        let monday = Date::from_ymd(2025, 1, 6).unwrap();
        assert_eq!(saturday.next_weekday(), monday);

        // Sunday -> Monday
        let sunday = Date::from_ymd(2025, 1, 5).unwrap();
        assert_eq!(sunday.next_weekday(), monday);
    }

    #[test]
    fn test_prev_weekday() {
        // Monday -> Monday (no change)
        let monday = Date::from_ymd(2025, 1, 6).unwrap();
        assert_eq!(monday.prev_weekday(), monday);

        // Saturday -> Friday
        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        let friday = Date::from_ymd(2025, 1, 3).unwrap();
        assert_eq!(saturday.prev_weekday(), friday);

        // Sunday -> Friday
        let sunday = Date::from_ymd(2025, 1, 5).unwrap();
        assert_eq!(sunday.prev_weekday(), friday);
    }

    #[test]
    fn test_add_business_days() {
        // Starting from Monday Jan 6, 2025
        let monday = Date::from_ymd(2025, 1, 6).unwrap();

        // Add 5 business days -> next Monday
        let next_monday = Date::from_ymd(2025, 1, 13).unwrap();
        assert_eq!(monday.add_business_days(5), next_monday);

        // Add 1 business day -> Tuesday
        let tuesday = Date::from_ymd(2025, 1, 7).unwrap();
        assert_eq!(monday.add_business_days(1), tuesday);

        // Subtract 1 business day -> Friday
        let prev_friday = Date::from_ymd(2025, 1, 3).unwrap();
        assert_eq!(monday.add_business_days(-1), prev_friday);
    }

    #[test]
    fn test_business_days_between() {
        // Monday to Friday = 4 business days
        let monday = Date::from_ymd(2025, 1, 6).unwrap();
        let friday = Date::from_ymd(2025, 1, 10).unwrap();
        assert_eq!(monday.business_days_between(&friday), 4);

        // Friday to Monday = 1 business day
        assert_eq!(friday.business_days_between(&monday), -4);

        // Same day = 0
        assert_eq!(monday.business_days_between(&monday), 0);
    }

    #[test]
    fn test_date_arithmetic_operators() {
        let d1 = Date::from_ymd(2025, 1, 1).unwrap();

        // Add days
        let d2 = d1 + 10;
        assert_eq!(d2.day(), 11);

        // Subtract days
        let d3 = d2 - 5;
        assert_eq!(d3.day(), 6);

        // Subtract dates
        assert_eq!(d2 - d1, 10);
    }

    #[test]
    fn test_start_end_of_period() {
        let date = Date::from_ymd(2025, 6, 15).unwrap();

        assert_eq!(date.start_of_month(), Date::from_ymd(2025, 6, 1).unwrap());
        assert_eq!(date.end_of_month(), Date::from_ymd(2025, 6, 30).unwrap());
        assert_eq!(date.start_of_year(), Date::from_ymd(2025, 1, 1).unwrap());
        assert_eq!(date.end_of_year(), Date::from_ymd(2025, 12, 31).unwrap());
    }

    #[test]
    fn test_min_max() {
        let d1 = Date::from_ymd(2025, 1, 1).unwrap();
        let d2 = Date::from_ymd(2025, 6, 15).unwrap();

        assert_eq!(d1.min(d2), d1);
        assert_eq!(d1.max(d2), d2);
        assert_eq!(d2.min(d1), d1);
        assert_eq!(d2.max(d1), d2);
    }

    #[test]
    fn test_display() {
        let date = Date::from_ymd(2025, 6, 15).unwrap();
        assert_eq!(format!("{}", date), "2025-06-15");
    }

    #[test]
    fn test_serde() {
        let date = Date::from_ymd(2025, 6, 15).unwrap();
        let json = serde_json::to_string(&date).unwrap();
        let parsed: Date = serde_json::from_str(&json).unwrap();
        assert_eq!(date, parsed);
    }
}
