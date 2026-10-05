//! Bounded business-day offsets for caller-supplied calendars.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::Calendar;
use crate::{ConvexError, ConvexResult, Date};

/// Maximum calendar days inspected by one checked business-day operation.
pub const MAX_CALENDAR_SEARCH_DAYS: usize = 3660;
/// Maximum requested business-day offset in either direction.
pub const MAX_BUSINESS_DAY_OFFSET: i32 = 366;

/// Find a business day, including `date`, in direction +1 or -1.
/// Custom calendar callbacks must themselves terminate and must not panic.
pub fn business_day_on_or_after<C: Calendar + ?Sized>(
    date: Date,
    calendar: &C,
) -> ConvexResult<Date> {
    search(date, 1, calendar)
}

/// Find a business day on or before the supplied date, with the same bound.
pub fn business_day_on_or_before<C: Calendar + ?Sized>(
    date: Date,
    calendar: &C,
) -> ConvexResult<Date> {
    search(date, -1, calendar)
}

fn search<C: Calendar + ?Sized>(
    mut date: Date,
    direction: i64,
    calendar: &C,
) -> ConvexResult<Date> {
    for _ in 0..MAX_CALENDAR_SEARCH_DAYS {
        if calendar.is_business_day(date) {
            return Ok(date);
        }
        date = date.checked_add_days(direction)?;
    }
    Err(ConvexError::CalendarError {
        reason: "no business day within checked search bound".into(),
    })
}

/// Advance by business days, excluding the initial date. Zero preserves it.
/// Returns an error for an excessive offset, exhausted search or date overflow.
pub fn add_business_days<C: Calendar + ?Sized>(
    mut date: Date,
    days: i32,
    calendar: &C,
) -> ConvexResult<Date> {
    if !(-MAX_BUSINESS_DAY_OFFSET..=MAX_BUSINESS_DAY_OFFSET).contains(&days) {
        return Err(ConvexError::CalendarError {
            reason: "business-day offset exceeds 366".into(),
        });
    }
    let mut remaining = days.unsigned_abs();
    if remaining == 0 {
        return Ok(date);
    }
    let direction = if days > 0 { 1 } else { -1 };
    for _ in 0..MAX_CALENDAR_SEARCH_DAYS {
        date = date.checked_add_days(direction)?;
        if calendar.is_business_day(date) {
            remaining -= 1;
            if remaining == 0 {
                return Ok(date);
            }
        }
    }
    Err(ConvexError::CalendarError {
        reason: "business-day offset exhausted search bound".into(),
    })
}
