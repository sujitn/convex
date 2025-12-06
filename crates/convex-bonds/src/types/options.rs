//! Embedded option schedules for callable and puttable bonds.
//!
//! Provides structures for representing call and put schedules with various
//! exercise styles and pricing conventions.

use convex_core::Date;
use serde::{Deserialize, Serialize};

/// Type of call provision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CallType {
    /// American-style: callable on any date within period
    American,
    /// European-style: callable only on specific dates
    European,
    /// Bermudan-style: callable on specific dates within period
    Bermudan,
    /// Make-whole call: redemption at treasury + spread
    MakeWhole,
    /// Par call: callable at par on or after a specific date
    ParCall,
    /// Mandatory call: issuer must call on specific trigger
    Mandatory,
}

impl CallType {
    /// Returns true if this is a continuous exercise style.
    #[must_use]
    pub fn is_continuous(&self) -> bool {
        matches!(self, CallType::American)
    }

    /// Returns true if this call type requires a model (not just discounting).
    #[must_use]
    pub fn requires_model(&self) -> bool {
        matches!(
            self,
            CallType::American | CallType::Bermudan | CallType::European
        )
    }
}

/// Type of put provision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PutType {
    /// American-style: puttable on any date within period
    American,
    /// European-style: puttable only on specific dates
    European,
    /// Bermudan-style: puttable on specific dates within period
    Bermudan,
    /// Change of control put: triggered by ownership change
    ChangeOfControl,
    /// Death put (survivor's option): triggered by holder's death
    DeathPut,
}

impl PutType {
    /// Returns true if this is a continuous exercise style.
    #[must_use]
    pub fn is_continuous(&self) -> bool {
        matches!(self, PutType::American)
    }

    /// Returns true if this put type requires a model.
    #[must_use]
    pub fn requires_model(&self) -> bool {
        matches!(
            self,
            PutType::American | PutType::Bermudan | PutType::European
        )
    }
}

/// A single entry in a call schedule.
///
/// Represents when an issuer can call the bond and at what price.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallEntry {
    /// First date on which this call price applies
    pub start_date: Date,
    /// Last date for this call price (None = until next entry or maturity)
    pub end_date: Option<Date>,
    /// Call price as percentage of par (e.g., 102.0 = 102%)
    pub call_price: f64,
    /// Notice period in days (if applicable)
    pub notice_days: Option<u32>,
}

impl CallEntry {
    /// Creates a new call entry.
    #[must_use]
    pub fn new(start_date: Date, call_price: f64) -> Self {
        Self {
            start_date,
            end_date: None,
            call_price,
            notice_days: None,
        }
    }

    /// Sets the end date for this call period.
    #[must_use]
    pub fn with_end_date(mut self, end_date: Date) -> Self {
        self.end_date = Some(end_date);
        self
    }

    /// Sets the notice period in days.
    #[must_use]
    pub fn with_notice_days(mut self, days: u32) -> Self {
        self.notice_days = Some(days);
        self
    }

    /// Returns true if this call entry is active on the given date.
    #[must_use]
    pub fn is_active_on(&self, date: Date) -> bool {
        date >= self.start_date && self.end_date.map_or(true, |end| date <= end)
    }

    /// Returns the call price as a decimal (e.g., 1.02 for 102%).
    #[must_use]
    pub fn price_decimal(&self) -> f64 {
        self.call_price / 100.0
    }
}

/// A single entry in a put schedule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PutEntry {
    /// First date on which this put price applies
    pub start_date: Date,
    /// Last date for this put price (None = until next entry or maturity)
    pub end_date: Option<Date>,
    /// Put price as percentage of par (e.g., 100.0 = par)
    pub put_price: f64,
    /// Notice period in days (if applicable)
    pub notice_days: Option<u32>,
}

impl PutEntry {
    /// Creates a new put entry.
    #[must_use]
    pub fn new(start_date: Date, put_price: f64) -> Self {
        Self {
            start_date,
            end_date: None,
            put_price,
            notice_days: None,
        }
    }

    /// Sets the end date for this put period.
    #[must_use]
    pub fn with_end_date(mut self, end_date: Date) -> Self {
        self.end_date = Some(end_date);
        self
    }

    /// Sets the notice period in days.
    #[must_use]
    pub fn with_notice_days(mut self, days: u32) -> Self {
        self.notice_days = Some(days);
        self
    }

    /// Returns true if this put entry is active on the given date.
    #[must_use]
    pub fn is_active_on(&self, date: Date) -> bool {
        date >= self.start_date && self.end_date.map_or(true, |end| date <= end)
    }

    /// Returns the put price as a decimal.
    #[must_use]
    pub fn price_decimal(&self) -> f64 {
        self.put_price / 100.0
    }
}

/// Call schedule for a callable bond.
///
/// Contains the type of call provision and the schedule of call dates/prices.
///
/// # Example
///
/// ```
/// use convex_bonds::types::{CallSchedule, CallType, CallEntry};
/// use convex_core::Date;
///
/// let schedule = CallSchedule::new(CallType::American)
///     .with_entry(CallEntry::new(Date::from_ymd(2025, 1, 15).unwrap(), 102.0))
///     .with_entry(CallEntry::new(Date::from_ymd(2026, 1, 15).unwrap(), 101.0))
///     .with_entry(CallEntry::new(Date::from_ymd(2027, 1, 15).unwrap(), 100.0));
///
/// assert!(schedule.is_callable_on(Date::from_ymd(2025, 6, 15).unwrap()));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallSchedule {
    /// Type of call provision
    pub call_type: CallType,
    /// Schedule entries (should be sorted by start_date)
    pub entries: Vec<CallEntry>,
    /// Protection period end date (bond cannot be called before this)
    pub protection_end: Option<Date>,
    /// Make-whole spread in basis points (for make-whole calls)
    pub make_whole_spread: Option<f64>,
}

impl CallSchedule {
    /// Creates a new call schedule with the given type.
    #[must_use]
    pub fn new(call_type: CallType) -> Self {
        Self {
            call_type,
            entries: Vec::new(),
            protection_end: None,
            make_whole_spread: None,
        }
    }

    /// Creates a make-whole call schedule with the given spread.
    #[must_use]
    pub fn make_whole(spread_bps: f64) -> Self {
        Self {
            call_type: CallType::MakeWhole,
            entries: Vec::new(),
            protection_end: None,
            make_whole_spread: Some(spread_bps),
        }
    }

    /// Adds a call entry to the schedule.
    #[must_use]
    pub fn with_entry(mut self, entry: CallEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Sets the protection period end date.
    #[must_use]
    pub fn with_protection(mut self, protection_end: Date) -> Self {
        self.protection_end = Some(protection_end);
        self
    }

    /// Sets the make-whole spread.
    #[must_use]
    pub fn with_make_whole_spread(mut self, spread_bps: f64) -> Self {
        self.make_whole_spread = Some(spread_bps);
        self
    }

    /// Returns true if the bond is callable on the given date.
    #[must_use]
    pub fn is_callable_on(&self, date: Date) -> bool {
        // Check protection period
        if let Some(protection_end) = self.protection_end {
            if date < protection_end {
                return false;
            }
        }

        // Check if any entry is active
        self.entries.iter().any(|e| e.is_active_on(date))
    }

    /// Returns the call price on the given date, if callable.
    #[must_use]
    pub fn call_price_on(&self, date: Date) -> Option<f64> {
        if !self.is_callable_on(date) {
            return None;
        }

        // Find the active entry (last one with start_date <= date)
        self.entries
            .iter()
            .filter(|e| e.start_date <= date)
            .last()
            .map(|e| e.call_price)
    }

    /// Returns the first call date.
    #[must_use]
    pub fn first_call_date(&self) -> Option<Date> {
        // Consider protection period
        let first_entry = self.entries.first().map(|e| e.start_date)?;

        match self.protection_end {
            Some(protection) if protection > first_entry => Some(protection),
            _ => Some(first_entry),
        }
    }

    /// Returns the first call price.
    #[must_use]
    pub fn first_call_price(&self) -> Option<f64> {
        self.entries.first().map(|e| e.call_price)
    }

    /// Sorts entries by start date.
    pub fn sort_entries(&mut self) {
        self.entries.sort_by_key(|e| e.start_date);
    }
}

/// Put schedule for a puttable bond.
///
/// Contains the type of put provision and the schedule of put dates/prices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PutSchedule {
    /// Type of put provision
    pub put_type: PutType,
    /// Schedule entries (should be sorted by start_date)
    pub entries: Vec<PutEntry>,
}

impl PutSchedule {
    /// Creates a new put schedule with the given type.
    #[must_use]
    pub fn new(put_type: PutType) -> Self {
        Self {
            put_type,
            entries: Vec::new(),
        }
    }

    /// Adds a put entry to the schedule.
    #[must_use]
    pub fn with_entry(mut self, entry: PutEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Returns true if the bond is puttable on the given date.
    #[must_use]
    pub fn is_puttable_on(&self, date: Date) -> bool {
        self.entries.iter().any(|e| e.is_active_on(date))
    }

    /// Returns the put price on the given date, if puttable.
    #[must_use]
    pub fn put_price_on(&self, date: Date) -> Option<f64> {
        if !self.is_puttable_on(date) {
            return None;
        }

        self.entries
            .iter()
            .filter(|e| e.start_date <= date)
            .last()
            .map(|e| e.put_price)
    }

    /// Returns the first put date.
    #[must_use]
    pub fn first_put_date(&self) -> Option<Date> {
        self.entries.first().map(|e| e.start_date)
    }

    /// Returns the first put price.
    #[must_use]
    pub fn first_put_price(&self) -> Option<f64> {
        self.entries.first().map(|e| e.put_price)
    }

    /// Sorts entries by start date.
    pub fn sort_entries(&mut self) {
        self.entries.sort_by_key(|e| e.start_date);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    #[test]
    fn test_call_type() {
        assert!(CallType::American.is_continuous());
        assert!(!CallType::European.is_continuous());
        assert!(CallType::American.requires_model());
        assert!(!CallType::Mandatory.requires_model());
    }

    #[test]
    fn test_put_type() {
        assert!(PutType::American.is_continuous());
        assert!(!PutType::European.is_continuous());
        assert!(PutType::Bermudan.requires_model());
    }

    #[test]
    fn test_call_entry() {
        let entry = CallEntry::new(date(2025, 1, 15), 102.0)
            .with_end_date(date(2025, 12, 31))
            .with_notice_days(30);

        assert!(entry.is_active_on(date(2025, 6, 15)));
        assert!(!entry.is_active_on(date(2024, 6, 15)));
        assert!(!entry.is_active_on(date(2026, 1, 15)));
        assert!((entry.price_decimal() - 1.02).abs() < 1e-10);
    }

    #[test]
    fn test_call_schedule() {
        let schedule = CallSchedule::new(CallType::American)
            .with_protection(date(2024, 1, 15))
            .with_entry(CallEntry::new(date(2023, 1, 15), 103.0))
            .with_entry(CallEntry::new(date(2024, 1, 15), 102.0))
            .with_entry(CallEntry::new(date(2025, 1, 15), 101.0));

        // Before protection period
        assert!(!schedule.is_callable_on(date(2023, 6, 15)));

        // After protection period
        assert!(schedule.is_callable_on(date(2024, 6, 15)));
        assert_eq!(schedule.call_price_on(date(2024, 6, 15)), Some(102.0));

        // After step-down
        assert_eq!(schedule.call_price_on(date(2025, 6, 15)), Some(101.0));

        // First call date should be protection end date
        assert_eq!(schedule.first_call_date(), Some(date(2024, 1, 15)));
    }

    #[test]
    fn test_make_whole_call() {
        let schedule = CallSchedule::make_whole(25.0)
            .with_protection(date(2024, 1, 15))
            .with_entry(CallEntry::new(date(2024, 1, 15), 100.0));

        assert_eq!(schedule.call_type, CallType::MakeWhole);
        assert_eq!(schedule.make_whole_spread, Some(25.0));
    }

    #[test]
    fn test_put_schedule() {
        let schedule = PutSchedule::new(PutType::European)
            .with_entry(PutEntry::new(date(2025, 1, 15), 100.0))
            .with_entry(PutEntry::new(date(2026, 1, 15), 100.0));

        assert!(schedule.is_puttable_on(date(2025, 1, 15)));
        assert_eq!(schedule.put_price_on(date(2025, 1, 15)), Some(100.0));
        assert_eq!(schedule.first_put_date(), Some(date(2025, 1, 15)));
    }
}
