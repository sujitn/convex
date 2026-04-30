//! Floating-rate note with an issuer call schedule. Composes
//! [`FloatingRateNote`] with a [`CallSchedule`]; analytics live in
//! `convex-analytics::spreads::DiscountMarginCalculator`.

use convex_core::types::Date;

use crate::instruments::FloatingRateNote;
use crate::traits::Bond;
use crate::types::{CallSchedule, CallType};

/// A floating-rate note with an issuer call schedule.
#[derive(Debug, Clone)]
pub struct CallableFloatingRateNote {
    base: FloatingRateNote,
    call_schedule: CallSchedule,
}

impl CallableFloatingRateNote {
    #[must_use]
    pub fn new(base: FloatingRateNote, call_schedule: CallSchedule) -> Self {
        Self {
            base,
            call_schedule,
        }
    }

    #[must_use]
    pub fn base_frn(&self) -> &FloatingRateNote {
        &self.base
    }

    /// Workout dates strictly after `settlement`. Bermudan/European return
    /// each entry's start date; American/ParCall expand to all coupon dates
    /// inside each entry's window. Entries whose start ≥ maturity (after
    /// clamping) are suppressed — DM-to-call-at-maturity ≡ DM-to-maturity.
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

    #[must_use]
    pub fn call_price_on(&self, date: Date) -> Option<f64> {
        self.call_schedule.call_price_on(date)
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

        let dates = cb.all_workout_dates(date(2026, 1, 15));
        assert_eq!(dates, vec![date(2027, 1, 15), date(2028, 1, 15)]);

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
        // Between entries, the most recent active one governs.
        assert_eq!(cb.call_price_on(date(2027, 6, 15)), Some(101.0));
        // Before the first call date, the bond is not callable.
        assert_eq!(cb.call_price_on(date(2026, 12, 31)), None);
    }
}
