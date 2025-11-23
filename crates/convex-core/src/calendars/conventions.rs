//! Business day adjustment conventions.

use serde::{Deserialize, Serialize};

use super::Calendar;
use crate::error::ConvexResult;
use crate::types::Date;

/// Business day adjustment conventions.
///
/// These conventions specify how to adjust a date that falls
/// on a non-business day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum BusinessDayConvention {
    /// No adjustment - use the date as-is even if not a business day.
    Unadjusted,

    /// Move to the following business day.
    #[default]
    Following,

    /// Move to the following business day, unless it crosses a month boundary,
    /// in which case move to the preceding business day.
    ModifiedFollowing,

    /// Move to the preceding business day.
    Preceding,

    /// Move to the preceding business day, unless it crosses a month boundary,
    /// in which case move to the following business day.
    ModifiedPreceding,

    /// Move to the nearest business day (following or preceding, whichever is closer).
    Nearest,

    /// End of month convention - if the start date is EOM, adjusted date should be EOM.
    EndOfMonth,
}

impl std::fmt::Display for BusinessDayConvention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            BusinessDayConvention::Unadjusted => "Unadjusted",
            BusinessDayConvention::Following => "Following",
            BusinessDayConvention::ModifiedFollowing => "Modified Following",
            BusinessDayConvention::Preceding => "Preceding",
            BusinessDayConvention::ModifiedPreceding => "Modified Preceding",
            BusinessDayConvention::Nearest => "Nearest",
            BusinessDayConvention::EndOfMonth => "End of Month",
        };
        write!(f, "{name}")
    }
}

/// Adjusts a date according to the given business day convention.
pub fn adjust<C: Calendar + ?Sized>(
    date: Date,
    convention: BusinessDayConvention,
    calendar: &C,
) -> ConvexResult<Date> {
    if calendar.is_business_day(date) && convention != BusinessDayConvention::EndOfMonth {
        return Ok(date);
    }

    match convention {
        BusinessDayConvention::Unadjusted => Ok(date),

        BusinessDayConvention::Following => Ok(following(date, calendar)),

        BusinessDayConvention::ModifiedFollowing => {
            let adjusted = following(date, calendar);
            if adjusted.month() != date.month() {
                // Crossed month boundary, go preceding instead
                Ok(preceding(date, calendar))
            } else {
                Ok(adjusted)
            }
        }

        BusinessDayConvention::Preceding => Ok(preceding(date, calendar)),

        BusinessDayConvention::ModifiedPreceding => {
            let adjusted = preceding(date, calendar);
            if adjusted.month() != date.month() {
                // Crossed month boundary, go following instead
                Ok(following(date, calendar))
            } else {
                Ok(adjusted)
            }
        }

        BusinessDayConvention::Nearest => {
            let fwd = following(date, calendar);
            let back = preceding(date, calendar);

            let fwd_days = date.days_between(&fwd);
            let back_days = back.days_between(&date);

            if fwd_days <= back_days {
                Ok(fwd)
            } else {
                Ok(back)
            }
        }

        BusinessDayConvention::EndOfMonth => {
            let eom = date.end_of_month();
            if calendar.is_business_day(eom) {
                Ok(eom)
            } else {
                Ok(preceding(eom, calendar))
            }
        }
    }
}

/// Returns the next business day on or after the given date.
fn following<C: Calendar + ?Sized>(mut date: Date, calendar: &C) -> Date {
    while !calendar.is_business_day(date) {
        date = date.add_days(1);
    }
    date
}

/// Returns the previous business day on or before the given date.
fn preceding<C: Calendar + ?Sized>(mut date: Date, calendar: &C) -> Date {
    while !calendar.is_business_day(date) {
        date = date.add_days(-1);
    }
    date
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendars::WeekendCalendar;

    #[test]
    fn test_following() {
        let cal = WeekendCalendar;

        // Saturday should roll to Monday
        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        let adjusted = adjust(saturday, BusinessDayConvention::Following, &cal).unwrap();

        assert_eq!(adjusted, Date::from_ymd(2025, 1, 6).unwrap());
    }

    #[test]
    fn test_preceding() {
        let cal = WeekendCalendar;

        // Saturday should roll to Friday
        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        let adjusted = adjust(saturday, BusinessDayConvention::Preceding, &cal).unwrap();

        assert_eq!(adjusted, Date::from_ymd(2025, 1, 3).unwrap());
    }

    #[test]
    fn test_modified_following() {
        let cal = WeekendCalendar;

        // Sunday Jan 5 should roll to Monday Jan 6 (same month)
        let sunday = Date::from_ymd(2025, 1, 5).unwrap();
        let adjusted = adjust(sunday, BusinessDayConvention::ModifiedFollowing, &cal).unwrap();
        assert_eq!(adjusted, Date::from_ymd(2025, 1, 6).unwrap());
    }

    #[test]
    fn test_unadjusted() {
        let cal = WeekendCalendar;

        let saturday = Date::from_ymd(2025, 1, 4).unwrap();
        let adjusted = adjust(saturday, BusinessDayConvention::Unadjusted, &cal).unwrap();

        assert_eq!(adjusted, saturday);
    }

    #[test]
    fn test_business_day_unchanged() {
        let cal = WeekendCalendar;

        let monday = Date::from_ymd(2025, 1, 6).unwrap();
        let adjusted = adjust(monday, BusinessDayConvention::Following, &cal).unwrap();

        assert_eq!(adjusted, monday);
    }
}
