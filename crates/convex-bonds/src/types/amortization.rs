//! Amortization schedules for amortizing bonds.
//!
//! Provides structures for representing principal repayment schedules.

use convex_core::Date;
use serde::{Deserialize, Serialize};

/// Type of amortization structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AmortizationType {
    /// Bullet maturity (all principal at maturity)
    Bullet,
    /// Level principal payments
    LevelPrincipal,
    /// Level total payments (mortgage-style)
    LevelPayment,
    /// Custom schedule
    Custom,
    /// Sinking fund (mandatory redemptions)
    SinkingFund,
    /// Pass-through (variable prepayments)
    PassThrough,
}

impl AmortizationType {
    /// Returns true if this type has variable principal payments.
    #[must_use]
    pub fn is_variable(&self) -> bool {
        matches!(self, AmortizationType::PassThrough)
    }

    /// Returns true if this type requires a schedule.
    #[must_use]
    pub fn requires_schedule(&self) -> bool {
        !matches!(self, AmortizationType::Bullet)
    }
}

/// A single entry in an amortization schedule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AmortizationEntry {
    /// Date of principal payment
    pub date: Date,
    /// Principal amount as percentage of original face (e.g., 10.0 = 10%)
    pub principal_pct: f64,
    /// Remaining factor after this payment (optional, computed if not set)
    pub remaining_factor: Option<f64>,
}

impl AmortizationEntry {
    /// Creates a new amortization entry.
    #[must_use]
    pub fn new(date: Date, principal_pct: f64) -> Self {
        Self {
            date,
            principal_pct,
            remaining_factor: None,
        }
    }

    /// Sets the remaining factor after this payment.
    #[must_use]
    pub fn with_remaining_factor(mut self, factor: f64) -> Self {
        self.remaining_factor = Some(factor);
        self
    }

    /// Returns the principal payment as a decimal (e.g., 0.10 for 10%).
    #[must_use]
    pub fn principal_decimal(&self) -> f64 {
        self.principal_pct / 100.0
    }
}

/// Amortization schedule for a bond.
///
/// Defines how principal is repaid over the life of the bond.
///
/// # Example
///
/// ```
/// use convex_bonds::types::{AmortizationSchedule, AmortizationType, AmortizationEntry};
/// use convex_core::Date;
///
/// // Sinking fund bond with 20% annual redemption
/// let schedule = AmortizationSchedule::new(AmortizationType::SinkingFund)
///     .with_entry(AmortizationEntry::new(Date::from_ymd(2025, 6, 15).unwrap(), 20.0))
///     .with_entry(AmortizationEntry::new(Date::from_ymd(2026, 6, 15).unwrap(), 20.0))
///     .with_entry(AmortizationEntry::new(Date::from_ymd(2027, 6, 15).unwrap(), 20.0))
///     .with_entry(AmortizationEntry::new(Date::from_ymd(2028, 6, 15).unwrap(), 20.0))
///     .with_entry(AmortizationEntry::new(Date::from_ymd(2029, 6, 15).unwrap(), 20.0));
///
/// assert_eq!(schedule.total_principal_pct(), 100.0);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AmortizationSchedule {
    /// Type of amortization
    pub amort_type: AmortizationType,
    /// Schedule of principal payments
    pub entries: Vec<AmortizationEntry>,
    /// Original face value (for absolute amount calculations)
    pub original_face: Option<f64>,
}

impl AmortizationSchedule {
    /// Creates a new amortization schedule.
    #[must_use]
    pub fn new(amort_type: AmortizationType) -> Self {
        Self {
            amort_type,
            entries: Vec::new(),
            original_face: None,
        }
    }

    /// Creates a bullet schedule (no amortization).
    #[must_use]
    pub fn bullet() -> Self {
        Self::new(AmortizationType::Bullet)
    }

    /// Creates a level principal schedule with given number of payments.
    #[must_use]
    pub fn level_principal(dates: Vec<Date>) -> Self {
        let n = dates.len() as f64;
        let principal_per_payment = 100.0 / n;

        let mut schedule = Self::new(AmortizationType::LevelPrincipal);
        for date in dates {
            schedule
                .entries
                .push(AmortizationEntry::new(date, principal_per_payment));
        }
        schedule.compute_remaining_factors();
        schedule
    }

    /// Adds an amortization entry.
    #[must_use]
    pub fn with_entry(mut self, entry: AmortizationEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Sets the original face value.
    #[must_use]
    pub fn with_original_face(mut self, face: f64) -> Self {
        self.original_face = Some(face);
        self
    }

    /// Returns the factor (remaining principal / original face) as of a date.
    #[must_use]
    pub fn factor_as_of(&self, date: Date) -> f64 {
        // Sum up all principal payments before the date
        let paid_pct: f64 = self
            .entries
            .iter()
            .filter(|e| e.date <= date)
            .map(|e| e.principal_pct)
            .sum();

        (100.0 - paid_pct).max(0.0) / 100.0
    }

    /// Returns entries that occur on or after the given date.
    #[must_use]
    pub fn entries_from(&self, date: Date) -> Vec<&AmortizationEntry> {
        self.entries.iter().filter(|e| e.date >= date).collect()
    }

    /// Returns the total principal percentage in the schedule.
    #[must_use]
    pub fn total_principal_pct(&self) -> f64 {
        self.entries.iter().map(|e| e.principal_pct).sum()
    }

    /// Validates that total principal equals 100%.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        // Allow for small floating point differences
        (self.total_principal_pct() - 100.0).abs() < 0.001
    }

    /// Computes and sets remaining factors for all entries.
    pub fn compute_remaining_factors(&mut self) {
        // First sort by date
        self.entries.sort_by_key(|e| e.date);

        let mut remaining = 100.0;
        for entry in &mut self.entries {
            remaining -= entry.principal_pct;
            entry.remaining_factor = Some((remaining.max(0.0)) / 100.0);
        }
    }

    /// Sorts entries by date.
    pub fn sort_entries(&mut self) {
        self.entries.sort_by_key(|e| e.date);
    }

    /// Returns the next principal payment date after the given date.
    #[must_use]
    pub fn next_payment_date(&self, after: Date) -> Option<Date> {
        self.entries.iter().find(|e| e.date > after).map(|e| e.date)
    }

    /// Returns the principal payment amount for a specific date.
    #[must_use]
    pub fn principal_on(&self, date: Date) -> Option<f64> {
        self.entries
            .iter()
            .find(|e| e.date == date)
            .map(|e| e.principal_pct)
    }

    /// Scales all principal amounts by a factor (for partial position sizing).
    pub fn scale(&mut self, factor: f64) {
        for entry in &mut self.entries {
            entry.principal_pct *= factor;
        }
    }
}

impl Default for AmortizationSchedule {
    fn default() -> Self {
        Self::bullet()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    #[test]
    fn test_amortization_type() {
        assert!(AmortizationType::PassThrough.is_variable());
        assert!(!AmortizationType::SinkingFund.is_variable());
        assert!(AmortizationType::Custom.requires_schedule());
        assert!(!AmortizationType::Bullet.requires_schedule());
    }

    #[test]
    fn test_amortization_entry() {
        let entry = AmortizationEntry::new(date(2025, 6, 15), 25.0);
        assert!((entry.principal_decimal() - 0.25).abs() < 1e-10);
    }

    #[test]
    fn test_bullet_schedule() {
        let schedule = AmortizationSchedule::bullet();
        assert_eq!(schedule.amort_type, AmortizationType::Bullet);
        assert!(schedule.entries.is_empty());
        assert_eq!(schedule.factor_as_of(date(2025, 1, 1)), 1.0);
    }

    #[test]
    fn test_level_principal() {
        let dates = vec![
            date(2025, 6, 15),
            date(2026, 6, 15),
            date(2027, 6, 15),
            date(2028, 6, 15),
        ];

        let schedule = AmortizationSchedule::level_principal(dates);

        assert_eq!(schedule.entries.len(), 4);
        assert!((schedule.entries[0].principal_pct - 25.0).abs() < 0.001);
        assert!(schedule.is_complete());

        // Check remaining factors
        assert!((schedule.entries[0].remaining_factor.unwrap() - 0.75).abs() < 0.001);
        assert!((schedule.entries[3].remaining_factor.unwrap() - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_factor_as_of() {
        let schedule = AmortizationSchedule::new(AmortizationType::SinkingFund)
            .with_entry(AmortizationEntry::new(date(2025, 6, 15), 25.0))
            .with_entry(AmortizationEntry::new(date(2026, 6, 15), 25.0))
            .with_entry(AmortizationEntry::new(date(2027, 6, 15), 25.0))
            .with_entry(AmortizationEntry::new(date(2028, 6, 15), 25.0));

        // Before any payments
        assert!((schedule.factor_as_of(date(2025, 1, 1)) - 1.0).abs() < 0.001);

        // After first payment
        assert!((schedule.factor_as_of(date(2025, 7, 1)) - 0.75).abs() < 0.001);

        // After two payments
        assert!((schedule.factor_as_of(date(2026, 7, 1)) - 0.50).abs() < 0.001);

        // After all payments
        assert!((schedule.factor_as_of(date(2028, 7, 1)) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_next_payment_date() {
        let schedule = AmortizationSchedule::new(AmortizationType::SinkingFund)
            .with_entry(AmortizationEntry::new(date(2025, 6, 15), 50.0))
            .with_entry(AmortizationEntry::new(date(2026, 6, 15), 50.0));

        assert_eq!(
            schedule.next_payment_date(date(2025, 1, 1)),
            Some(date(2025, 6, 15))
        );
        assert_eq!(
            schedule.next_payment_date(date(2025, 6, 15)),
            Some(date(2026, 6, 15))
        );
        assert_eq!(schedule.next_payment_date(date(2026, 6, 15)), None);
    }

    #[test]
    fn test_scale() {
        let mut schedule = AmortizationSchedule::new(AmortizationType::SinkingFund)
            .with_entry(AmortizationEntry::new(date(2025, 6, 15), 100.0));

        schedule.scale(0.5);
        assert!((schedule.entries[0].principal_pct - 50.0).abs() < 0.001);
    }
}
