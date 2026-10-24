//! Private atomic node transitions, error propagation and bounded ready admission.

use super::{
    dag::Dag, frequency_ready, next, CachedValue, CalculationFailure, GraphError, NodeRole,
    SnapshotValuationResult,
};
use convex_core::contracts::{DataQuality, ErrorCode, ResultStatus};
use convex_ports::snapshot::MarketSnapshot;
use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

#[derive(Default)]
pub(super) struct NodeState {
    pub(super) generation: u64,
    pub(super) revision: u64,
    pub(super) dirty: bool,
    pub(super) forced: bool,
    pub(super) cached: Option<Arc<CachedValue>>,
    pub(super) last_good: Option<Arc<CachedValue>>,
    pub(super) failure: Option<CalculationFailure>,
    pub(super) blocked: bool,
    pub(super) failed_valuation: Option<Arc<SnapshotValuationResult>>,
    pub(super) attempt: Option<(u64, Instant)>,
    pub(super) last_calc: Option<Instant>,
}
impl NodeState {
    pub(super) fn invalidate(&mut self, generation: u64, forced: bool) {
        self.generation = generation;
        self.dirty = true;
        self.forced = forced;
        self.failure = None;
        self.blocked = false;
        self.failed_valuation = None;
        self.attempt = None;
    }
    pub(super) fn current(&self, now: Instant) -> Option<&Arc<CachedValue>> {
        self.cached.as_ref().filter(|value| {
            !self.dirty
                && self.failure.is_none()
                && value.generation == self.generation
                && value.valid_until.is_none_or(|deadline| now <= deadline)
        })
    }
}

pub(super) struct GraphState {
    pub(super) dag: Dag,
    pub(super) nodes: Vec<NodeState>,
    pub(super) generation: u64,
    pub(super) epoch: u64,
    pub(super) result_revision: u64,
    pub(super) next_attempt: u64,
    pub(super) snapshot: Option<Arc<MarketSnapshot>>,
    pub(super) snapshot_pending: bool,
    pub(super) suspended: bool,
}
impl GraphState {
    pub(super) fn empty() -> Self {
        Self {
            dag: Dag::empty(),
            nodes: vec![],
            generation: 0,
            epoch: 0,
            result_revision: 0,
            next_attempt: 0,
            snapshot: None,
            snapshot_pending: false,
            suspended: false,
        }
    }
    pub(super) fn advance_generation(&mut self) -> Result<u64, GraphError> {
        self.generation = next(self.generation)?;
        Ok(self.generation)
    }
    pub(super) fn invalidate(&mut self, root: usize, forced: bool) -> Result<u64, GraphError> {
        let generation = self.advance_generation()?;
        let affected = if self.snapshot.is_some() || self.snapshot_pending {
            self.snapshot = None;
            self.snapshot_pending = true;
            (0..self.nodes.len()).collect()
        } else {
            self.dag.affected(&[root])
        };
        for i in affected {
            self.nodes[i].invalidate(generation, forced && i == root);
        }
        Ok(generation)
    }
    pub(super) fn fail(
        &mut self,
        root: usize,
        failure: CalculationFailure,
    ) -> Result<u64, GraphError> {
        self.fail_many(vec![(root, failure)])?;
        Ok(self.nodes[root].revision)
    }
    pub(super) fn fail_many(
        &mut self,
        roots: Vec<(usize, CalculationFailure)>,
    ) -> Result<(), GraphError> {
        let mut seen = vec![false; self.nodes.len()];
        let mut work = Vec::with_capacity(self.nodes.len());
        for (i, failure) in roots {
            if !seen[i] {
                seen[i] = true;
                work.push((i, i, failure));
            }
        }
        let mut cursor = 0;
        while cursor < work.len() {
            let (i, origin, failure) = work[cursor].clone();
            for &d in &self.dag.dependents[i] {
                if !seen[d] {
                    seen[d] = true;
                    work.push((d, origin, failure.clone()));
                }
            }
            cursor += 1;
        }
        // Reserve the entire unique closure before mutating any status.
        self.result_revision
            .checked_add(work.len() as u64)
            .ok_or(GraphError::CounterExhausted)?;
        for (i, origin, mut failure) in work {
            self.result_revision += 1;
            if i != origin {
                failure.dependency = Some(self.dag.nodes[origin].id.clone());
            }
            let state = &mut self.nodes[i];
            state.revision = self.result_revision;
            state.failure = Some(failure);
            state.dirty = false;
            state.forced = false;
            state.attempt = None;
            state.blocked = i != origin;
            state.failed_valuation = None;
        }
        Ok(())
    }
    pub(super) fn maintain(&mut self, now: Instant) -> Result<(), GraphError> {
        let mut expired = Vec::new();
        for i in 0..self.nodes.len() {
            if self.nodes[i]
                .attempt
                .is_some_and(|(_, deadline)| now > deadline)
            {
                expired.push((
                    i,
                    CalculationFailure::fixed(
                        ErrorCode::DeadlineExceeded,
                        "calculation attempt expired",
                    ),
                ));
            } else if self.nodes[i].failure.is_none()
                && self.nodes[i].cached.as_ref().is_some_and(|v| {
                    !self.nodes[i].dirty
                        && v.generation == self.nodes[i].generation
                        && v.valid_until.is_some_and(|deadline| now > deadline)
                })
            {
                expired.push((
                    i,
                    CalculationFailure::fixed(
                        ErrorCode::StaleInput,
                        "consumed input freshness deadline expired",
                    ),
                ));
            }
        }
        if !expired.is_empty() {
            self.fail_many(expired)?;
        }
        Ok(())
    }
    pub(super) fn ready(&self, now: Instant, limit: usize) -> Vec<usize> {
        if self.suspended || self.snapshot_pending {
            return vec![];
        }
        let mut ready = BTreeSet::new();
        for (i, node) in self.dag.nodes.iter().enumerate() {
            let state = &self.nodes[i];
            if node.role == NodeRole::Input
                || !state.dirty
                || state.attempt.is_some()
                || !frequency_ready(&node.config.frequency, state, now)
                || self.dag.dependencies[i].iter().any(|&d| {
                    self.nodes[d].current(now).is_none_or(|v| {
                        v.status != ResultStatus::Complete || v.quality != DataQuality::Valid
                    })
                })
            {
                continue;
            }
            let entry = (Reverse(node.config.priority), i);
            if ready.len() < limit {
                ready.insert(entry);
            } else if ready.last().is_some_and(|last| entry < *last) {
                ready.pop_last();
                ready.insert(entry);
            }
        }
        ready.into_iter().map(|(_, i)| i).collect()
    }
}
