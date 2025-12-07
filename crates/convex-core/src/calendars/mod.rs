//! Business day calendars and conventions.
//!
//! This module provides:
//! - Business day calendars for different markets
//! - Business day adjustment conventions
//! - Holiday detection and date rolling
//!
//! # Available Calendars
//!
//! | Calendar | Description | Usage |
//! |----------|-------------|-------|
//! | `SIFMACalendar` | US fixed income (bond market) | Corporate bonds, Munis |
//! | `USGovernmentCalendar` | US Treasury securities | Treasuries |
//! | `Target2Calendar` | Eurozone payments | EUR swaps, Bunds |
//! | `UKCalendar` | UK bank holidays | Gilts |
//! | `JapanCalendar` | Japan holidays | JGBs |
//! | `WeekendCalendar` | Weekend only (no holidays) | Testing |
//!
//! # Performance
//!
//! All calendars use bitmap storage for O(1) holiday lookups:
//! - `is_business_day()`: < 10ns
//! - `is_holiday()`: < 10ns
//! - Memory: ~12KB per calendar
//!
//! # Example
//!
//! ```
//! use convex_core::calendars::{Calendar, SIFMACalendar};
//! use convex_core::types::Date;
//!
//! let cal = SIFMACalendar::new();
//!
//! let new_years = Date::from_ymd(2025, 1, 1).unwrap();
//! assert!(!cal.is_business_day(new_years)); // Holiday
//!
//! let jan2 = Date::from_ymd(2025, 1, 2).unwrap();
//! assert!(cal.is_business_day(jan2)); // Business day
//! ```

use chrono::Datelike;

mod bitmap;
mod conventions;
mod dynamic;
mod japan;
mod sifma;
mod target2;
mod uk;
mod us_calendar;

// Re-export bitmap types
pub use bitmap::{
    easter_sunday, last_weekday_of_month, nth_weekday_of_month, observed_date, HolidayBitmap,
    HolidayCalendarBuilder, WeekendType, MAX_YEAR, MIN_YEAR,
};

// Re-export dynamic calendar types
pub use dynamic::{CalendarData, CustomCalendarBuilder, DynamicCalendar};

// Re-export conventions
pub use conventions::BusinessDayConvention;

// Re-export calendar implementations
pub use japan::JapanCalendar;
pub use sifma::{SIFMACalendar, USGovernmentCalendar};
pub use target2::Target2Calendar;
pub use uk::UKCalendar;
pub use us_calendar::USCalendar;

use crate::error::ConvexResult;
use crate::types::Date;

/// Trait for business day calendars.
///
/// Calendars determine which days are business days vs holidays
/// for a specific market or jurisdiction.
///
/// # Performance
///
/// All implementations should aim for O(1) `is_business_day()` performance.
/// The bitmap-based calendars achieve this with ~10ns lookup times.
pub trait Calendar: Send + Sync {
    /// Returns the name of the calendar.
    fn name(&self) -> &'static str;

    /// Returns true if the date is a business day.
    ///
    /// A business day is neither a weekend nor a holiday.
    fn is_business_day(&self, date: Date) -> bool;

    /// Returns true if the date is a holiday (excluding weekends).
    fn is_holiday(&self, date: Date) -> bool {
        !self.is_business_day(date)
    }

    /// Adjusts a date according to the given business day convention.
    fn adjust(&self, date: Date, convention: BusinessDayConvention) -> ConvexResult<Date> {
        conventions::adjust(date, convention, self)
    }

    /// Advances a date by a number of business days.
    ///
    /// Positive values move forward, negative values move backward.
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

    /// Calculate settlement date from trade date.
    ///
    /// # Arguments
    ///
    /// * `trade_date` - The trade execution date
    /// * `settlement_days` - Number of business days to settlement (e.g., 1 for T+1)
    ///
    /// # Returns
    ///
    /// The settlement date
    fn settlement_date(&self, trade_date: Date, settlement_days: u32) -> Date {
        self.add_business_days(trade_date, settlement_days as i32)
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
        !matches!(weekday, chrono::Weekday::Sat | chrono::Weekday::Sun)
    }
}

/// Calendar that combines multiple calendars (joint holidays).
///
/// A date is a business day only if it's a business day in ALL component calendars.
/// This is useful for cross-border transactions.
///
/// # Example
///
/// ```
/// use convex_core::calendars::{JointCalendar, SIFMACalendar, Target2Calendar, Calendar};
/// use convex_core::types::Date;
///
/// let us_eur = JointCalendar::new(vec![
///     Box::new(SIFMACalendar::new()),
///     Box::new(Target2Calendar::new()),
/// ]);
///
/// // Check if a date is a business day in both US and EUR markets
/// let date = Date::from_ymd(2025, 1, 1).unwrap();
/// assert!(!us_eur.is_business_day(date)); // New Year's in both
/// ```
pub struct JointCalendar {
    calendars: Vec<Box<dyn Calendar>>,
    #[allow(dead_code)]
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

    #[test]
    fn test_settlement_date() {
        let cal = SIFMACalendar::new();

        // T+1 settlement from Wednesday Jan 15
        let trade_date = Date::from_ymd(2025, 1, 15).unwrap();
        let settle = cal.settlement_date(trade_date, 1);
        assert_eq!(settle, Date::from_ymd(2025, 1, 16).unwrap());

        // T+2 settlement from Friday Jan 17
        let trade_date = Date::from_ymd(2025, 1, 17).unwrap();
        let settle = cal.settlement_date(trade_date, 2);
        assert_eq!(settle, Date::from_ymd(2025, 1, 22).unwrap()); // Skip weekend + MLK day
    }

    #[test]
    fn test_joint_calendar() {
        let us_eur = JointCalendar::new(vec![
            Box::new(SIFMACalendar::new()),
            Box::new(Target2Calendar::new()),
        ]);

        // New Year's is a holiday in both
        let new_years = Date::from_ymd(2025, 1, 1).unwrap();
        assert!(!us_eur.is_business_day(new_years));

        // MLK Day is US only
        let mlk_day = Date::from_ymd(2025, 1, 20).unwrap();
        assert!(!us_eur.is_business_day(mlk_day)); // Holiday in joint calendar

        // May 1 is TARGET2 only
        let labour_day = Date::from_ymd(2025, 5, 1).unwrap();
        assert!(!us_eur.is_business_day(labour_day)); // Holiday in joint calendar
    }

    #[test]
    fn test_next_business_day() {
        let cal = SIFMACalendar::new();

        // From Saturday, next business day is Monday
        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        let next = cal.next_business_day(saturday);
        assert_eq!(next, Date::from_ymd(2025, 1, 6).unwrap());

        // From a business day, returns same day
        let monday = Date::from_ymd(2025, 1, 6).unwrap();
        let next = cal.next_business_day(monday);
        assert_eq!(next, monday);
    }

    #[test]
    fn test_previous_business_day() {
        let cal = SIFMACalendar::new();

        // From Sunday, previous business day is Friday
        let sunday = Date::from_ymd(2025, 1, 5).unwrap();
        let prev = cal.previous_business_day(sunday);
        assert_eq!(prev, Date::from_ymd(2025, 1, 3).unwrap());

        // From a business day, returns same day
        let friday = Date::from_ymd(2025, 1, 3).unwrap();
        let prev = cal.previous_business_day(friday);
        assert_eq!(prev, friday);
    }
}
