//! Bounded memoization of complete snapshot/context results. Reads never refresh lineage.
#![forbid(unsafe_code)]

use crate::{context::ValuationContext, snapshot::SnapshotValuationResult};
use convex_core::contracts::SnapshotId;
use convex_ports::TraitError;
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};

/// Application result cache for one snapshot (D51). Every entry retains the complete context
/// and captured generation. A context names its snapshot, so a result for another snapshot
/// releases every retained entry; callers sharing one cache across snapshots recompute, never fail.
pub struct ValuationCache {
    capacity: usize,
    retained: Mutex<Retained>,
}
/// The contexts of the snapshot most recently inserted.
#[derive(Default)]
struct Retained {
    snapshot_id: Option<SnapshotId>,
    results: HashMap<String, Arc<SnapshotValuationResult>>,
}
impl ValuationCache {
    /// Explicit 1..=100,000 entry bound for one snapshot. Within a snapshot, capacity refusal
    /// is visible; entries are released only when a different snapshot's result arrives.
    pub fn new(capacity: usize) -> Result<Self, TraitError> {
        if !(1..=100_000).contains(&capacity) {
            return Err(TraitError::InvalidInput("invalid result cache capacity"));
        }
        Ok(Self {
            capacity,
            retained: Mutex::new(Retained::default()),
        })
    }
    /// Only a complete exact context addresses the cache. Bind it to a snapshot before access.
    pub fn get(
        &self,
        context: &ValuationContext,
    ) -> Result<Option<Arc<SnapshotValuationResult>>, TraitError> {
        let key = context.cache_key()?;
        Ok(self
            .retained
            .lock()
            .results
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
        let mut retained = self.retained.lock();
        let snapshot_id = &result.context().spec().snapshot_id;
        if retained.snapshot_id.as_ref() != Some(snapshot_id) {
            retained.results.clear();
            retained.snapshot_id = Some(snapshot_id.clone());
        }
        if let Some(prior) = retained.results.get(&key) {
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
        if retained.results.len() == self.capacity {
            return Err(TraitError::CapacityExceeded);
        }
        let result = Arc::new(result);
        retained.results.insert(key, result.clone());
        Ok(result)
    }
    /// Explicit administrative release; handed-out snapshots/results stay immutable.
    pub fn clear(&self) {
        let mut retained = self.retained.lock();
        retained.results.clear();
        retained.snapshot_id = None;
    }
    /// Number of retained contexts.
    pub fn len(&self) -> usize {
        self.retained.lock().results.len()
    }
    /// Whether any contexts are retained.
    pub fn is_empty(&self) -> bool {
        self.retained.lock().results.is_empty()
    }
}
