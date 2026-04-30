//! Callable floating-rate note.
//!
//! Composes a [`FloatingRateNote`] with a [`CallSchedule`] and exposes a
//! workout-bullet pricing surface analogous to the pre-OAS callable fixed
//! bond (M4): for each call date, the bond is priced as if it terminates at
//! that date with redemption equal to the schedule's call price; the
//! "discount margin to worst" is the minimum DM across all workout dates
//! plus DM-to-maturity. Pricing happens via
//! [`crate::analytics_glue`] — actually no, the DM solver lives in
//! `convex-analytics`. The bond itself only holds shape + helper accessors;
//! callers run the analytics calculator.

use convex_core::types::{Currency, Date, Frequency};
use rust_decimal::Decimal;

use crate::error::{BondError, BondResult};
use crate::instruments::FloatingRateNote;
use crate::traits::{Bond, BondCashFlow};
use crate::types::{BondIdentifiers, BondType, CalendarId, CallSchedule, CallType};

/// A floating-rate note with an issuer call schedule.
#[derive(Debug, Clone)]
pub struct CallableFloatingRateNote {
    base: FloatingRateNote,
    call_schedule: CallSchedule,
}

impl CallableFloatingRateNote {
    /// Creates a new callable FRN.
    #[must_use]
    pub fn new(base: FloatingRateNote, call_schedule: CallSchedule) -> Self {
        Self {
            base,
            call_schedule,
        }
    }

    /// Underlying FRN.
    #[must_use]
    pub fn base_frn(&self) -> &FloatingRateNote {
        &self.base
    }

    /// Call schedule reference.
    #[must_use]
    pub fn call_schedule(&self) -> &CallSchedule {
        &self.call_schedule
    }

    /// Returns the call type.
    #[must_use]
    pub fn call_type(&self) -> CallType {
        self.call_schedule.call_type
    }

    /// All workout dates strictly after `settlement`. For Bermudan/European
    /// schedules this is just the entries' start dates; for American/ParCall
    /// the bond's coupon dates within each entry's window are emitted (so
    /// solvers see Bermudan-equivalent call exercise points).
    #[must_use]
    pub fn all_workout_dates(&self, settlement: Date) -> Vec<Date> {
        let Some(maturity) = self.base.maturity() else {
            return Vec::new();
        };
        let mut dates = Vec::new();
        for entry in &self.call_schedule.entries {
            if let Some(protection_end) = self.call_schedule.protection_end {
                if entry.start_date < protection_end {
                    continue;
                }
            }
            let start = entry.start_date.max(settlement);
            let end = entry.end_date.unwrap_or(maturity).min(maturity);
            if start >= end || entry.start_date <= settlement {
                continue;
            }
            match self.call_schedule.call_type {
                CallType::American | CallType::MakeWhole | CallType::ParCall => {
                    if let Some(first) = self.base.next_coupon_date(start) {
                        let mut current = first;
                        while current <= end {
                            if current > settlement {
                                dates.push(current);
                            }
                            match self.base.next_coupon_date(current) {
                                Some(next) if next > current => current = next,
                                _ => break,
                            }
                        }
                    }
                }
                CallType::European | CallType::Bermudan | CallType::Mandatory => {
                    if entry.start_date > settlement {
                        dates.push(entry.start_date);
                    }
                }
            }
        }
        dates.sort();
        dates.dedup();
        dates
    }

    /// Look up the call price scheduled on `date`, if any.
    #[must_use]
    pub fn call_price_on(&self, date: Date) -> Option<f64> {
        self.call_schedule.call_price_on(date)
    }

    /// First workout date after `settlement`, if any.
    #[must_use]
    pub fn first_call_date_after(&self, settlement: Date) -> Option<Date> {
        self.all_workout_dates(settlement).into_iter().next()
    }
}

impl Bond for CallableFloatingRateNote {
    fn identifiers(&self) -> &BondIdentifiers {
        self.base.identifiers()
    }

    fn bond_type(&self) -> BondType {
        // The library doesn't have a dedicated CallableFRN BondType yet; report
        // FloatingRateNote so existing classifiers don't misroute. A future
        // refactor can add the variant.
        self.base.bond_type()
    }

    fn currency(&self) -> Currency {
        self.base.currency()
    }

    fn maturity(&self) -> Option<Date> {
        self.base.maturity()
    }

    fn issue_date(&self) -> Date {
        self.base.issue_date()
    }

    fn first_settlement_date(&self) -> Date {
        self.base.first_settlement_date()
    }

    fn dated_date(&self) -> Date {
        self.base.dated_date()
    }

    fn face_value(&self) -> Decimal {
        self.base.face_value()
    }

    fn frequency(&self) -> Frequency {
        self.base.frequency()
    }

    fn cash_flows(&self, from: Date) -> Vec<BondCashFlow> {
        self.base.cash_flows(from)
    }

    fn next_coupon_date(&self, after: Date) -> Option<Date> {
        self.base.next_coupon_date(after)
    }

    fn previous_coupon_date(&self, before: Date) -> Option<Date> {
        self.base.previous_coupon_date(before)
    }

    fn accrued_interest(&self, settlement: Date) -> Decimal {
        self.base.accrued_interest(settlement)
    }

    fn day_count_convention(&self) -> &str {
        self.base.day_count_convention()
    }

    fn calendar(&self) -> &CalendarId {
        self.base.calendar()
    }

    fn redemption_value(&self) -> Decimal {
        self.base.redemption_value()
    }
}

/// Builder for [`CallableFloatingRateNote`].
#[derive(Debug, Clone, Default)]
pub struct CallableFloatingRateNoteBuilder {
    base: Option<FloatingRateNote>,
    call_schedule: Option<CallSchedule>,
}

impl CallableFloatingRateNoteBuilder {
    /// New empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the underlying FRN.
    #[must_use]
    pub fn base_frn(mut self, frn: FloatingRateNote) -> Self {
        self.base = Some(frn);
        self
    }

    /// Set the call schedule.
    #[must_use]
    pub fn call_schedule(mut self, schedule: CallSchedule) -> Self {
        self.call_schedule = Some(schedule);
        self
    }

    /// Finalise.
    pub fn build(self) -> BondResult<CallableFloatingRateNote> {
        let base = self
            .base
            .ok_or_else(|| BondError::missing_field("base_frn"))?;
        let schedule = self
            .call_schedule
            .ok_or_else(|| BondError::missing_field("call_schedule"))?;
        Ok(CallableFloatingRateNote::new(base, schedule))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CallEntry, CallSchedule};
    use convex_core::types::Frequency;
    use convex_curves::multicurve::RateIndex;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn build_frn() -> FloatingRateNote {
        FloatingRateNote::builder()
            .index(RateIndex::Sofr)
            .spread_bps(125)
            .maturity(date(2030, 1, 15))
            .issue_date(date(2025, 1, 15))
            .frequency(Frequency::Quarterly)
            .day_count(convex_core::daycounts::DayCountConvention::Act360)
            .build()
            .unwrap()
    }

    #[test]
    fn workout_dates_bermudan_after_settlement() {
        let frn = build_frn();
        let schedule = CallSchedule::new(CallType::Bermudan)
            .with_entry(CallEntry::new(date(2027, 1, 15), 100.0))
            .with_entry(CallEntry::new(date(2028, 1, 15), 100.0));
        let cb = CallableFloatingRateNote::new(frn, schedule);

        // Settlement before all calls — both should appear.
        let dates = cb.all_workout_dates(date(2026, 1, 15));
        assert_eq!(dates, vec![date(2027, 1, 15), date(2028, 1, 15)]);

        // Settlement after first call — only the second.
        let dates = cb.all_workout_dates(date(2027, 7, 15));
        assert_eq!(dates, vec![date(2028, 1, 15)]);
    }

    #[test]
    fn call_price_lookup() {
        let frn = build_frn();
        let schedule = CallSchedule::new(CallType::Bermudan)
            .with_entry(CallEntry::new(date(2027, 1, 15), 101.0))
            .with_entry(CallEntry::new(date(2028, 1, 15), 100.0));
        let cb = CallableFloatingRateNote::new(frn, schedule);
        assert_eq!(cb.call_price_on(date(2027, 1, 15)), Some(101.0));
        assert_eq!(cb.call_price_on(date(2028, 1, 15)), Some(100.0));
        // Between the two entries, the most recent active entry (2027-01-15)
        // is what governs — its call price stays in force until the next
        // entry's start date.
        assert_eq!(cb.call_price_on(date(2027, 6, 15)), Some(101.0));
        // Before the first call date, the bond is not callable.
        assert_eq!(cb.call_price_on(date(2026, 12, 31)), None);
    }
}
