//! Bounded memoization of complete snapshot/context results. Reads never refresh lineage.
#![forbid(unsafe_code)]

use crate::{context::ValuationContext, snapshot::SnapshotValuationResult};
use convex_ports::TraitError;
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};

/// Application result cache. Every entry retains the complete context and captured generation.
pub struct ValuationCache {
    capacity: usize,
    results: Mutex<HashMap<String, Arc<SnapshotValuationResult>>>,
}
impl ValuationCache {
    /// Explicit 1..=100,000 entry bound. Capacity refusal is visible; there is no arbitrary eviction.
    pub fn new(capacity: usize) -> Result<Self, TraitError> {
        if !(1..=100_000).contains(&capacity) {
            return Err(TraitError::InvalidInput("invalid result cache capacity"));
        }
        Ok(Self {
            capacity,
            results: Mutex::new(HashMap::new()),
        })
    }
    /// Only a complete exact context addresses the cache. Bind it to a snapshot before access.
    pub fn get(
        &self,
        context: &ValuationContext,
    ) -> Result<Option<Arc<SnapshotValuationResult>>, TraitError> {
        let key = context.cache_key()?;
        Ok(self
            .results
            .lock()
            .get(&key)
            .filter(|entry| entry.context() == context)
            .cloned())
    }
    /// Retain the first result/timestamp for an identical context. Fail conflicting retries.
    pub fn insert(
        &self,
        result: SnapshotValuationResult,
    ) -> Result<Arc<SnapshotValuationResult>, TraitError> {
        result.validate()?;
        let key = result.context().cache_key()?;
        let mut results = self.results.lock();
        if let Some(prior) = results.get(&key) {
            if prior.context() != result.context()
                || prior.envelope().results() != result.envelope().results()
                || prior.envelope().status() != result.envelope().status()
                || prior.envelope().errors() != result.envelope().errors()
                || prior.envelope().quality() != result.envelope().quality()
                || prior.envelope().provenance().input_versions
                    != result.envelope().provenance().input_versions
            {
                return Err(TraitError::Conflict(
                    "different valuation for identical context",
                ));
            }
            return Ok(prior.clone());
        }
        if results.len() == self.capacity {
            return Err(TraitError::CapacityExceeded);
        }
        let result = Arc::new(result);
        results.insert(key, result.clone());
        Ok(result)
    }
    /// Explicit administrative release; handed-out snapshots/results stay immutable.
    pub fn clear(&self) {
        self.results.lock().clear();
    }
    /// Number of retained contexts.
    pub fn len(&self) -> usize {
        self.results.lock().len()
    }
    /// Whether any contexts are retained.
    pub fn is_empty(&self) -> bool {
        self.results.lock().is_empty()
    }
}
