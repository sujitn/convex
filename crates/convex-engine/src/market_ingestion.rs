//! Bounded, deterministic quote image/delta assembly. No I/O or clock reads.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap};

use convex_ports::market_data::{
    FieldUpdate, QuoteEvent, QuoteEventKind, QuoteField, QuoteKey, QuoteQuality,
};
use convex_ports::TraitError;
use serde::{Deserialize, Serialize};

pub use convex_ports::quote_event::{DataState, FieldObservation, QuoteSnapshot};

/// Deterministic per-event outcome, suitable for a bounded external trace sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestionOutcome {
    /// Data was accepted.
    Applied,
    /// Exact latest data event retry; no side effects.
    Duplicate,
    /// Older sequence ignored without rolling state backwards.
    OutOfOrder,
    /// Missing or discontinuous session/sequence; image required.
    RecoveryRequired,
    /// Structural, temporal, unit, conflicting duplicate or crossed-price error.
    Invalid,
    /// Explicit status/liveness message; values unchanged.
    Control,
    /// New key refused; all existing keys blocked until refreshed.
    CapacityExceeded,
}

/// Per-event trace; callers choose retention. The assembler does not accumulate traces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestionResult {
    /// Ordering key.
    pub key: QuoteKey,
    /// Supplied session.
    pub session: String,
    /// Supplied sequence.
    pub sequence: u64,
    /// Admission decision.
    pub outcome: IngestionOutcome,
    /// Resulting current state for the key.
    pub state: DataState,
}

/// Explicit bounds and freshness policy. These are engineering defaults, not approved trading SLOs.
#[derive(Debug, Clone, Copy)]
pub struct IngestionPolicy {
    /// Maximum retained source/instrument keys, 1..=100,000.
    pub max_keys: usize,
    /// Maximum age of every retained field, 1..=86,400,000 milliseconds.
    pub stale_after_ms: i64,
    /// Allowed source time ahead of receipt, 0..=60,000 milliseconds.
    pub future_tolerance_ms: i64,
}

impl Default for IngestionPolicy {
    fn default() -> Self {
        Self {
            max_keys: 10_000,
            stale_after_ms: 5_000,
            future_tolerance_ms: 100,
        }
    }
}

struct QuoteState {
    session: String,
    sequence: u64,
    recovery_floor: u64,
    state: DataState,
    fields: BTreeMap<QuoteField, FieldObservation>,
    last_data: Option<QuoteEvent>,
}

impl QuoteState {
    fn new(session: String) -> Self {
        Self {
            session,
            sequence: 0,
            recovery_floor: 0,
            state: DataState::AwaitingImage,
            fields: BTreeMap::new(),
            last_data: None,
        }
    }

    fn block(&mut self, state: DataState, sequence: u64) {
        self.state = state;
        self.recovery_floor = self.recovery_floor.max(sequence).max(self.sequence);
    }

    fn needs_image(&self) -> bool {
        matches!(
            self.state,
            DataState::AwaitingImage
                | DataState::Gap
                | DataState::Invalid
                | DataState::Closed
                | DataState::Unauthorized
        )
    }
}

/// One writer owns this bounded state. There is no queue, catch-up buffer or unbounded quarantine.
#[derive(Default)]
pub struct MarketDataAssembler {
    policy: IngestionPolicy,
    quotes: HashMap<QuoteKey, QuoteState>,
    source_counts: HashMap<convex_core::ids::InstrumentId, usize>,
}

impl MarketDataAssembler {
    /// Validate policy and create empty state.
    pub fn new(policy: IngestionPolicy) -> Result<Self, TraitError> {
        if policy.max_keys == 0
            || policy.max_keys > 100_000
            || !(1..=86_400_000).contains(&policy.stale_after_ms)
            || !(0..=60_000).contains(&policy.future_tolerance_ms)
        {
            return Err(TraitError::InvalidInput(
                "invalid ingestion bounds/time policy",
            ));
        }
        Ok(Self {
            policy,
            quotes: HashMap::new(),
            source_counts: HashMap::new(),
        })
    }

    /// Number of retained keys. Each key holds at most 12 fields and one accepted data envelope.
    pub fn len(&self) -> usize {
        self.quotes.len()
    }

    /// Whether any quote keys are retained.
    pub fn is_empty(&self) -> bool {
        self.quotes.is_empty()
    }

    /// Iterate retained keys without allocating another collection.
    pub fn keys(&self) -> impl Iterator<Item = &QuoteKey> {
        self.quotes.keys()
    }

    /// Whether a different source supplies the same instrument. No composite policy is assumed.
    pub fn has_other_source(&self, key: &QuoteKey) -> bool {
        self.source_counts
            .get(&key.instrument_id)
            .copied()
            .unwrap_or(0)
            > 1
    }

    /// Inspect quality without cloning retained fields or envelopes.
    pub fn state(&self, key: &QuoteKey, now_ms: i64) -> Option<DataState> {
        self.quotes
            .get(key)
            .map(|state| effective_state(state, now_ms, self.policy))
    }

    /// Remaining freshness budget for an admitted Live quote, calculated from every original field time.
    pub fn fresh_for_ms(&self, key: &QuoteKey, now_ms: i64) -> Option<i64> {
        let state = self.quotes.get(key)?;
        if effective_state(state, now_ms, self.policy) != DataState::Live {
            return None;
        }
        let age = state
            .fields
            .values()
            .map(|field| {
                now_ms
                    .saturating_sub(field.source_time_ms)
                    .max(now_ms.saturating_sub(field.receive_time_ms))
                    .max(0)
            })
            .max()?;
        Some(self.policy.stale_after_ms - age)
    }

    /// Forget an explicitly unsubscribed key. Reintroduction requires an image.
    pub fn remove(&mut self, key: &QuoteKey) {
        if self.quotes.remove(key).is_some() {
            if let Some(count) = self.source_counts.get_mut(&key.instrument_id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    self.source_counts.remove(&key.instrument_id);
                }
            }
        }
    }

    /// Transport loss affects every key because the discarded messages' keys are unknown.
    pub fn transport_gap(&mut self) {
        for state in self.quotes.values_mut() {
            state.block(DataState::Gap, state.sequence);
        }
    }

    /// Mark all retained data closed without deleting display evidence.
    pub fn close(&mut self) {
        for state in self.quotes.values_mut() {
            state.block(DataState::Closed, state.sequence);
        }
    }

    /// Inspect retained values at supplied time. Heartbeats cannot refresh age.
    pub fn snapshot(&self, key: &QuoteKey, now_ms: i64) -> Option<QuoteSnapshot> {
        let state = self.quotes.get(key)?;
        Some(QuoteSnapshot {
            watermark: state.last_data.clone()?,
            fields: state.fields.clone(),
            state: effective_state(state, now_ms, self.policy),
        })
    }

    /// Admit one bounded event, returning its exact disposition. Invalid content is never applied.
    pub fn ingest(&mut self, event: QuoteEvent, now_ms: i64) -> IngestionResult {
        // Validate before retaining identifiers, vectors or payloads.
        let valid = event.validate().is_ok()
            && event.receive_time_ms <= now_ms
            && event.source_time_ms
                <= event
                    .receive_time_ms
                    .saturating_add(self.policy.future_tolerance_ms);
        if !valid {
            if let Some(state) = self.quotes.get_mut(&event.key) {
                state.block(DataState::Invalid, event.sequence);
            }
            return result(&event, IngestionOutcome::Invalid, DataState::Invalid);
        }
        if !self.quotes.contains_key(&event.key) && self.quotes.len() >= self.policy.max_keys {
            self.transport_gap();
            return result(&event, IngestionOutcome::CapacityExceeded, DataState::Gap);
        }
        if !self.quotes.contains_key(&event.key) {
            *self
                .source_counts
                .entry(event.key.instrument_id.clone())
                .or_default() += 1;
        }
        let state = self
            .quotes
            .entry(event.key.clone())
            .or_insert_with(|| QuoteState::new(event.session.clone()));
        let outcome = apply(state, &event);
        result(&event, outcome, effective_state(state, now_ms, self.policy))
    }
}

fn result(event: &QuoteEvent, outcome: IngestionOutcome, state: DataState) -> IngestionResult {
    IngestionResult {
        key: event.key.clone(),
        session: event.session.clone(),
        sequence: event.sequence,
        outcome,
        state,
    }
}

fn effective_state(state: &QuoteState, now_ms: i64, policy: IngestionPolicy) -> DataState {
    if state.needs_image() {
        return state.state;
    }
    if state.fields.values().any(|field| {
        now_ms < field.receive_time_ms
            || now_ms.saturating_sub(field.source_time_ms) > policy.stale_after_ms
            || now_ms.saturating_sub(field.receive_time_ms) > policy.stale_after_ms
    }) {
        return DataState::Stale;
    }
    state.state
}

fn apply(state: &mut QuoteState, event: &QuoteEvent) -> IngestionOutcome {
    if matches!(event.event, QuoteEventKind::Reset) {
        state.session = event.session.clone();
        state.sequence = 0;
        state.recovery_floor = 0;
        state.state = if event.entitled {
            DataState::AwaitingImage
        } else {
            DataState::Unauthorized
        };
        // Retain the last good display value; never combine its fields with the new session.
        return IngestionOutcome::Control;
    }
    if state.session != event.session {
        state.block(DataState::Gap, state.sequence);
        return IngestionOutcome::RecoveryRequired;
    }
    if !event.entitled {
        state.block(DataState::Unauthorized, event.sequence);
        return IngestionOutcome::Control;
    }
    match &event.event {
        QuoteEventKind::Image { fields } => apply_data(state, event, fields, true),
        QuoteEventKind::Delta { fields } => apply_data(state, event, fields, false),
        QuoteEventKind::Heartbeat => {
            if event.sequence != state.sequence {
                state.block(DataState::Gap, event.sequence);
                IngestionOutcome::RecoveryRequired
            } else {
                IngestionOutcome::Control
            }
        }
        QuoteEventKind::Closed => {
            state.block(DataState::Closed, event.sequence);
            IngestionOutcome::Control
        }
        QuoteEventKind::Error { .. } => {
            state.block(DataState::Invalid, event.sequence);
            IngestionOutcome::Control
        }
        QuoteEventKind::Gap => {
            state.block(DataState::Gap, event.sequence);
            IngestionOutcome::Control
        }
        QuoteEventKind::Reset => IngestionOutcome::Control,
    }
}

fn apply_data(
    state: &mut QuoteState,
    event: &QuoteEvent,
    changes: &[convex_ports::quote_event::QuoteChange],
    image: bool,
) -> IngestionOutcome {
    if event.sequence < state.sequence {
        return IngestionOutcome::OutOfOrder;
    }
    if event.sequence == state.sequence && state.sequence != 0 {
        if state
            .last_data
            .as_ref()
            .is_some_and(|last| same_data(last, event))
            && !state.needs_image()
        {
            return IngestionOutcome::Duplicate;
        }
        // A newly requested image at the same watermark may repair a transport loss.
        if !image || !state.needs_image() {
            state.block(DataState::Invalid, event.sequence);
            return IngestionOutcome::Invalid;
        }
    }
    if (!image && (state.needs_image() || state.sequence.checked_add(1) != Some(event.sequence)))
        || (image && event.sequence < state.recovery_floor)
    {
        state.block(DataState::Gap, event.sequence);
        return IngestionOutcome::RecoveryRequired;
    }
    if let Some(last) = &state.last_data {
        if last.session == event.session
            && (event.receive_time_ms < last.receive_time_ms
                || event.source_time_ms < last.source_time_ms)
        {
            state.block(DataState::Invalid, event.sequence);
            return IngestionOutcome::Invalid;
        }
    }
    let mut fields = if image {
        BTreeMap::new()
    } else {
        state.fields.clone()
    };
    for change in changes {
        match &change.update {
            FieldUpdate::Unchanged => {}
            FieldUpdate::Clear => {
                fields.remove(&change.field);
            }
            FieldUpdate::Set(value) => {
                fields.insert(
                    change.field,
                    FieldObservation {
                        value: value.clone(),
                        source_time_ms: event.source_time_ms,
                        receive_time_ms: event.receive_time_ms,
                        sequence: event.sequence,
                    },
                );
            }
        }
    }
    if let (Some(bid), Some(ask)) = (
        fields.get(&QuoteField::BidPrice),
        fields.get(&QuoteField::AskPrice),
    ) {
        if bid.value.value > ask.value.value {
            state.block(DataState::Invalid, event.sequence);
            return IngestionOutcome::Invalid;
        }
    }
    state.state = if fields
        .keys()
        .all(|field| matches!(field, QuoteField::BidSize | QuoteField::AskSize))
    {
        DataState::Incomplete
    } else {
        match event.quality {
            QuoteQuality::Live => DataState::Live,
            QuoteQuality::Delayed => DataState::Delayed,
            QuoteQuality::Indicative => DataState::Indicative,
            QuoteQuality::Manual => DataState::Manual,
        }
    };
    state.fields = fields;
    state.sequence = event.sequence;
    state.recovery_floor = event.sequence;
    state.last_data = Some(event.clone());
    IngestionOutcome::Applied
}

fn same_data(last: &QuoteEvent, event: &QuoteEvent) -> bool {
    // A network retry has a new receipt time. It cannot refresh the retained observation.
    let mut previous = last.clone();
    previous.receive_time_ms = event.receive_time_ms;
    previous == *event
}
