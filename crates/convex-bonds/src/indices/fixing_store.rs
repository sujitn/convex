//! Index fixing store for historical rate lookups.
//!
//! Provides storage and retrieval of historical rate fixings for floating rate
//! instruments. Supports all major indices including SOFR, SONIA, €STR, EURIBOR.

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_core::types::Date;

use crate::types::RateIndex;

/// A single rate fixing observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexFixing {
    /// The fixing date
    pub date: Date,
    /// The rate index
    pub index: RateIndex,
    /// The fixing rate (as decimal, e.g., 0.0530 for 5.30%)
    pub rate: Decimal,
    /// Source of the fixing (optional)
    pub source: Option<String>,
}

impl IndexFixing {
    /// Creates a new index fixing.
    #[must_use]
    pub fn new(date: Date, index: RateIndex, rate: Decimal) -> Self {
        Self {
            date,
            index,
            rate,
            source: None,
        }
    }

    /// Creates a new fixing with a source attribution.
    #[must_use]
    pub fn with_source(date: Date, index: RateIndex, rate: Decimal, source: &str) -> Self {
        Self {
            date,
            index,
            rate,
            source: Some(source.to_string()),
        }
    }

    /// Returns the rate as a percentage (e.g., 5.30 for 5.30%).
    #[must_use]
    pub fn rate_percent(&self) -> Decimal {
        self.rate * Decimal::ONE_HUNDRED
    }
}

/// Storage for historical rate fixings.
///
/// Provides efficient lookup of rate fixings by index and date.
/// Internally uses a BTreeMap for ordered date access, enabling
/// efficient range queries for overnight compounding calculations.
///
/// # Example
///
/// ```rust,ignore
/// use convex_bonds::indices::IndexFixingStore;
/// use convex_bonds::types::RateIndex;
/// use rust_decimal_macros::dec;
///
/// let mut store = IndexFixingStore::new();
///
/// // Add SOFR fixings
/// store.add_fixing(date!(2024-01-02), RateIndex::SOFR, dec!(0.0530));
/// store.add_fixing(date!(2024-01-03), RateIndex::SOFR, dec!(0.0532));
///
/// // Look up a fixing
/// let rate = store.get_fixing(&RateIndex::SOFR, date!(2024-01-02));
/// assert_eq!(rate, Some(dec!(0.0530)));
/// ```
#[derive(Debug, Clone, Default)]
pub struct IndexFixingStore {
    /// Fixings organized by index -> date -> rate
    fixings: BTreeMap<String, BTreeMap<Date, Decimal>>,
}

impl IndexFixingStore {
    /// Creates a new empty fixing store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fixings: BTreeMap::new(),
        }
    }

    /// Returns the index key for storage lookup.
    fn index_key(index: &RateIndex) -> String {
        format!("{:?}", index)
    }

    /// Adds a single fixing to the store.
    pub fn add_fixing(&mut self, date: Date, index: RateIndex, rate: Decimal) {
        let key = Self::index_key(&index);
        self.fixings
            .entry(key)
            .or_default()
            .insert(date, rate);
    }

    /// Adds a fixing using an IndexFixing struct.
    pub fn add(&mut self, fixing: IndexFixing) {
        self.add_fixing(fixing.date, fixing.index, fixing.rate);
    }

    /// Adds multiple fixings at once.
    pub fn add_fixings(&mut self, fixings: Vec<IndexFixing>) {
        for fixing in fixings {
            self.add(fixing);
        }
    }

    /// Retrieves a fixing for a specific index and date.
    #[must_use]
    pub fn get_fixing(&self, index: &RateIndex, date: Date) -> Option<Decimal> {
        let key = Self::index_key(index);
        self.fixings.get(&key).and_then(|dates| dates.get(&date).copied())
    }

    /// Retrieves all fixings for an index between start and end dates (inclusive).
    ///
    /// Returns fixings in chronological order.
    #[must_use]
    pub fn get_range(&self, index: &RateIndex, start: Date, end: Date) -> Vec<(Date, Decimal)> {
        let key = Self::index_key(index);
        self.fixings
            .get(&key)
            .map(|dates| {
                dates
                    .range(start..=end)
                    .map(|(d, r)| (*d, *r))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns all dates with fixings for an index.
    #[must_use]
    pub fn fixing_dates(&self, index: &RateIndex) -> Vec<Date> {
        let key = Self::index_key(index);
        self.fixings
            .get(&key)
            .map(|dates| dates.keys().copied().collect())
            .unwrap_or_default()
    }

    /// Returns the most recent fixing on or before the given date.
    #[must_use]
    pub fn last_fixing_before(&self, index: &RateIndex, date: Date) -> Option<(Date, Decimal)> {
        let key = Self::index_key(index);
        self.fixings.get(&key).and_then(|dates| {
            dates.range(..=date).last().map(|(d, r)| (*d, *r))
        })
    }

    /// Returns the count of fixings for an index.
    #[must_use]
    pub fn count(&self, index: &RateIndex) -> usize {
        let key = Self::index_key(index);
        self.fixings.get(&key).map_or(0, |dates| dates.len())
    }

    /// Returns true if the store has any fixings for the given index.
    #[must_use]
    pub fn has_index(&self, index: &RateIndex) -> bool {
        let key = Self::index_key(index);
        self.fixings.contains_key(&key)
    }

    /// Returns all indices in the store.
    #[must_use]
    pub fn indices(&self) -> Vec<String> {
        self.fixings.keys().cloned().collect()
    }

    /// Clears all fixings for an index.
    pub fn clear_index(&mut self, index: &RateIndex) {
        let key = Self::index_key(index);
        self.fixings.remove(&key);
    }

    /// Clears all fixings from the store.
    pub fn clear(&mut self) {
        self.fixings.clear();
    }

    /// Creates a store from vectors of date-rate pairs for a specific index.
    #[must_use]
    pub fn from_rates(index: RateIndex, rates: Vec<(Date, Decimal)>) -> Self {
        let mut store = Self::new();
        for (date, rate) in rates {
            store.add_fixing(date, index.clone(), rate);
        }
        store
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    #[test]
    fn test_add_and_get_fixing() {
        let mut store = IndexFixingStore::new();
        store.add_fixing(date(2024, 1, 2), RateIndex::SOFR, dec!(0.0530));

        let rate = store.get_fixing(&RateIndex::SOFR, date(2024, 1, 2));
        assert_eq!(rate, Some(dec!(0.0530)));
    }

    #[test]
    fn test_missing_fixing() {
        let store = IndexFixingStore::new();
        let rate = store.get_fixing(&RateIndex::SOFR, date(2024, 1, 2));
        assert_eq!(rate, None);
    }

    #[test]
    fn test_get_range() {
        let mut store = IndexFixingStore::new();
        store.add_fixing(date(2024, 1, 2), RateIndex::SOFR, dec!(0.0530));
        store.add_fixing(date(2024, 1, 3), RateIndex::SOFR, dec!(0.0532));
        store.add_fixing(date(2024, 1, 4), RateIndex::SOFR, dec!(0.0531));
        store.add_fixing(date(2024, 1, 5), RateIndex::SOFR, dec!(0.0533));

        let range = store.get_range(&RateIndex::SOFR, date(2024, 1, 2), date(2024, 1, 4));
        assert_eq!(range.len(), 3);
        assert_eq!(range[0], (date(2024, 1, 2), dec!(0.0530)));
        assert_eq!(range[1], (date(2024, 1, 3), dec!(0.0532)));
        assert_eq!(range[2], (date(2024, 1, 4), dec!(0.0531)));
    }

    #[test]
    fn test_last_fixing_before() {
        let mut store = IndexFixingStore::new();
        store.add_fixing(date(2024, 1, 2), RateIndex::SOFR, dec!(0.0530));
        store.add_fixing(date(2024, 1, 3), RateIndex::SOFR, dec!(0.0532));
        store.add_fixing(date(2024, 1, 5), RateIndex::SOFR, dec!(0.0531));

        // On a fixing date
        let last = store.last_fixing_before(&RateIndex::SOFR, date(2024, 1, 3));
        assert_eq!(last, Some((date(2024, 1, 3), dec!(0.0532))));

        // Between fixings (should get 1/3)
        let last = store.last_fixing_before(&RateIndex::SOFR, date(2024, 1, 4));
        assert_eq!(last, Some((date(2024, 1, 3), dec!(0.0532))));

        // Before any fixings
        let last = store.last_fixing_before(&RateIndex::SOFR, date(2024, 1, 1));
        assert_eq!(last, None);
    }

    #[test]
    fn test_multiple_indices() {
        let mut store = IndexFixingStore::new();
        store.add_fixing(date(2024, 1, 2), RateIndex::SOFR, dec!(0.0530));
        store.add_fixing(date(2024, 1, 2), RateIndex::SONIA, dec!(0.0520));
        store.add_fixing(date(2024, 1, 2), RateIndex::ESTR, dec!(0.0395));

        assert_eq!(store.get_fixing(&RateIndex::SOFR, date(2024, 1, 2)), Some(dec!(0.0530)));
        assert_eq!(store.get_fixing(&RateIndex::SONIA, date(2024, 1, 2)), Some(dec!(0.0520)));
        assert_eq!(store.get_fixing(&RateIndex::ESTR, date(2024, 1, 2)), Some(dec!(0.0395)));
    }

    #[test]
    fn test_count_and_has_index() {
        let mut store = IndexFixingStore::new();
        assert!(!store.has_index(&RateIndex::SOFR));
        assert_eq!(store.count(&RateIndex::SOFR), 0);

        store.add_fixing(date(2024, 1, 2), RateIndex::SOFR, dec!(0.0530));
        store.add_fixing(date(2024, 1, 3), RateIndex::SOFR, dec!(0.0532));

        assert!(store.has_index(&RateIndex::SOFR));
        assert_eq!(store.count(&RateIndex::SOFR), 2);
    }

    #[test]
    fn test_index_fixing_struct() {
        let fixing = IndexFixing::new(date(2024, 1, 2), RateIndex::SOFR, dec!(0.053));
        assert_eq!(fixing.rate_percent(), dec!(5.3));

        let fixing_with_source = IndexFixing::with_source(
            date(2024, 1, 2),
            RateIndex::SOFR,
            dec!(0.053),
            "Federal Reserve",
        );
        assert_eq!(fixing_with_source.source, Some("Federal Reserve".to_string()));
    }

    #[test]
    fn test_from_rates() {
        let rates = vec![
            (date(2024, 1, 2), dec!(0.0530)),
            (date(2024, 1, 3), dec!(0.0532)),
            (date(2024, 1, 4), dec!(0.0531)),
        ];
        let store = IndexFixingStore::from_rates(RateIndex::SOFR, rates);

        assert_eq!(store.count(&RateIndex::SOFR), 3);
        assert_eq!(store.get_fixing(&RateIndex::SOFR, date(2024, 1, 3)), Some(dec!(0.0532)));
    }

    #[test]
    fn test_clear() {
        let mut store = IndexFixingStore::new();
        store.add_fixing(date(2024, 1, 2), RateIndex::SOFR, dec!(0.0530));
        store.add_fixing(date(2024, 1, 2), RateIndex::SONIA, dec!(0.0520));

        store.clear_index(&RateIndex::SOFR);
        assert!(!store.has_index(&RateIndex::SOFR));
        assert!(store.has_index(&RateIndex::SONIA));

        store.clear();
        assert!(!store.has_index(&RateIndex::SONIA));
    }
}
