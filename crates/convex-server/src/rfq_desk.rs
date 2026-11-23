//! The RFQ desk: each open request for quote, held in memory, and every transition of
//! every request, recorded.
//!
//! A transition is recorded before it is applied: one audit entry of type `rfq` whose
//! entity is the request's key and whose action is the state it moves to, carrying the
//! numbers it was made from. Nothing changes a request unrecorded, so a transition whose
//! entry cannot be written changes nothing. Entries are written through the audit store's
//! one writer, so their IDs commit in order.
//!
//! At most [`MAX_OPEN_RFQS`] requests are open. One task expires each at its deadline; a
//! transition that finds the deadline passed records the expiry itself, under the same
//! lock, so each request expires once. Before the desk takes any request, the task records
//! every request a previous run left open as expired, reason `restart`; no quote is sent
//! again. Those requests were received within twice the longest deadline before the last
//! `rfq` entry, so the task walks back from the log's end only that far; a log with no
//! `rfq` entry is read once, whole.
//!
//! A request's key is a digest of its source and the source's id for it: each is free
//! text up to 128 bytes, and an entity ID holds 128. A retry is recognised from the key's
//! first entry, so a source's id stays taken after its request leaves memory, until
//! retention removes that entry.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use convex_core::contracts::{ExactDecimal, InstrumentId, SourceId, UtcTimestamp, Version};
use convex_core::types::WidthFactor;
use convex_engine::ports::limits::LimitStatus;
use convex_engine::ports::quote_event::QuoteInput;
use convex_engine::ports::rfq::{
    transition, ClientTier, DeskSide, ExpiryReason, RfqActor, RfqEvent, RfqRequest, RfqState,
    RfqTransitionError, SourceOutcome, MAX_RFQ_DEADLINE_MS,
};
use convex_engine::ports::storage::{AuditEntry, MAX_AUDIT_BATCH, MAX_ENTITY_PAGE};
use convex_engine::ports::TraitError;
use serde::{Deserialize, Serialize};
use tokio::sync::{watch, Mutex, Notify};

use crate::access::AccessError;
use crate::audit_writer::AuditWriter;

/// The audit event type of an RFQ's transition.
pub(crate) const RFQ_EVENT: &str = "rfq";
/// The audit entity type of an RFQ's transition; the entity ID is its key.
pub(crate) const RFQ_ENTITY: &str = "rfq";
/// The most requests open at once; ingest refuses more with `RFQ_CAPACITY`.
pub const MAX_OPEN_RFQS: usize = 256;
/// Requests open.
pub const OPEN_RFQS: &str = "convex_rfqs_open";
/// Time from a request's receipt to its `priced` entry, in seconds.
pub const RFQ_PRICING_SECONDS: &str = "convex_rfq_pricing_seconds";
/// How soon a failed read or append is tried again.
const RETRY: Duration = Duration::from_secs(1);
/// The clock is outside the range a timestamp holds.
const OUT_OF_RANGE: TraitError =
    TraitError::InvalidInput("the clock is outside the supported range");

/// The server's key for a request: a digest of its source and the source's id for it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub(crate) struct RfqKey(String);

impl RfqKey {
    /// The key of `request`.
    pub(crate) fn of(request: &RfqRequest) -> Self {
        let mut bytes = request.source_id.as_str().as_bytes().to_vec();
        bytes.push(0);
        bytes.extend_from_slice(request.rfq_id.as_bytes());
        Self(convex_engine::ports::snapshot::content_digest(
            b"convex.rfq.v1\0",
            &bytes,
        ))
    }

    /// A key as a path names it: 64 lowercase hexadecimal digits.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        (text.len() == 64
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then(|| Self(text.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a request was received as: the request, the bond it resolved to, and when.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Receipt {
    pub(crate) request: RfqRequest,
    /// The bond the request names, resolved at receipt.
    pub(crate) instrument_id: InstrumentId,
    /// Its version at receipt.
    pub(crate) instrument_version: Version,
    pub(crate) received_at: UtcTimestamp,
}

/// What the server priced a request at, and what the book's limits said of it.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct RfqPricing {
    /// The configuration it was priced under, and its client tier's factor there.
    pub(crate) configuration: PricedUnder,
    pub(crate) auto_price: AutoPrice,
    /// The book's limits as if the trade were done, as the pre-trade check answers.
    pub(crate) check: LimitCheck,
    pub(crate) priced_at: UtcTimestamp,
}

/// The configuration a request was priced under.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct PricedUnder {
    pub(crate) configuration_id: SourceId,
    pub(crate) version: Version,
    pub(crate) activation_revision: Version,
    pub(crate) client_tier: ClientTier,
    pub(crate) width_factor: WidthFactor,
}

/// The server's price for the side of the desk's quote the client trades on, or why it
/// has none.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum AutoPrice {
    /// Priced from the bond's Live level.
    Priced(Box<PricedSide>),
    /// Not priced: the trader sets the price.
    Unpriced { reason: String },
}

/// The desk side's price and what it was made from.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct PricedSide {
    pub(crate) desk_side: DeskSide,
    /// The desk side's clean price.
    pub(crate) clean_price: ExactDecimal,
    /// The desk side's level in the rule's unit, for a bond quoted by a level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) level: Option<ExactDecimal>,
    pub(crate) clean_price_mid: ExactDecimal,
    /// Why the price is the mid: the bond has no two-way width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mid_only: Option<&'static str>,
    /// The quote the bond's Live record was valued from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) quote_input: Option<QuoteInput>,
    /// The rule as read: its version and method, the set and each security's Live price,
    /// the benchmark and the two-way.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pricing_rule: Option<serde_json::Value>,
    /// Each curve revision read.
    pub(crate) curves: Vec<CurveRead>,
}

/// One curve revision a price read.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CurveRead {
    pub(crate) curve_id: String,
    pub(crate) version: Version,
}

/// The pre-trade check of a request, or why it did not run.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum LimitCheck {
    /// The check route's answer, and how many limits it put in each status.
    Evaluated {
        status: serde_json::Value,
        statuses: BTreeMap<LimitStatus, usize>,
    },
    Unavailable {
        reason: &'static str,
    },
}

/// The quote a trader sent.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SentQuote {
    pub(crate) clean_price: ExactDecimal,
    pub(crate) quoted_at: UtcTimestamp,
}

/// A request received and priced, to be admitted.
pub(crate) struct Admission {
    pub(crate) rfq_key: RfqKey,
    pub(crate) receipt: Receipt,
    pub(crate) pricing: RfqPricing,
}

/// An open request: what it was admitted with, where it stands, and the quote sent.
#[derive(Clone)]
pub(crate) struct OpenRfq {
    pub(crate) admission: Arc<Admission>,
    pub(crate) state: RfqState,
    pub(crate) quote: Option<SentQuote>,
}

/// A transition as recorded: the event that makes it and what it carries. Each moves a
/// request to the state of its name.
#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(crate) enum Recorded<'a> {
    Received {
        #[serde(flatten)]
        receipt: &'a Receipt,
    },
    Priced {
        #[serde(flatten)]
        pricing: &'a RfqPricing,
    },
    Quoted {
        #[serde(flatten)]
        quote: &'a SentQuote,
    },
    Passed,
    Done,
    TradedAway,
    Covered,
    Expired {
        reason: ExpiryReason,
    },
    Cancelled,
}

impl Recorded<'_> {
    /// The source's report of the client's outcome or withdrawal.
    pub(crate) fn outcome(outcome: SourceOutcome) -> Self {
        match outcome {
            SourceOutcome::Done => Self::Done,
            SourceOutcome::TradedAway => Self::TradedAway,
            SourceOutcome::Covered => Self::Covered,
            SourceOutcome::Cancelled => Self::Cancelled,
        }
    }

    /// The state it moves a request to.
    fn state(&self) -> RfqState {
        match self {
            Self::Received { .. } => RfqState::Received,
            Self::Priced { .. } => RfqState::Priced,
            Self::Quoted { .. } => RfqState::Quoted,
            Self::Passed => RfqState::Passed,
            Self::Done => RfqState::Done,
            Self::TradedAway => RfqState::TradedAway,
            Self::Covered => RfqState::Covered,
            Self::Expired { .. } => RfqState::Expired,
            Self::Cancelled => RfqState::Cancelled,
        }
    }

    /// The event of the state table it is; none for a receipt.
    fn event(&self) -> Option<RfqEvent> {
        Some(match self {
            Self::Received { .. } => return None,
            Self::Priced { .. } => RfqEvent::Priced,
            Self::Quoted { .. } => RfqEvent::Quoted,
            Self::Passed => RfqEvent::Passed,
            Self::Done => RfqEvent::Done,
            Self::TradedAway => RfqEvent::TradedAway,
            Self::Covered => RfqEvent::Covered,
            Self::Expired { reason } => RfqEvent::Expired { reason: *reason },
            Self::Cancelled => RfqEvent::Cancelled,
        })
    }
}

/// One transition's details, as its entry carries them.
#[derive(Serialize)]
struct RecordDetails<'a> {
    actor: RfqActor,
    recorded_at: UtcTimestamp,
    #[serde(flatten)]
    record: &'a Recorded<'a>,
}

/// A request the desk knows: open, or recorded and no longer held.
pub(crate) struct Known {
    pub(crate) receipt: Receipt,
    pub(crate) state: RfqState,
}

/// What admitting a request did.
pub(crate) enum Admitted {
    /// Recorded received and priced, and open.
    Accepted,
    /// Already known, with the same content, in this state.
    Retry(RfqState),
    /// Already known with other content.
    Conflict,
    /// The desk holds [`MAX_OPEN_RFQS`].
    Full,
    /// Its entries could not be written, or its key's read.
    Unrecorded(TraitError),
}

/// Why a transition was refused.
#[derive(Debug)]
pub(crate) enum Refused {
    /// The desk knows no request by the key.
    Unknown,
    /// The table refuses the event in the request's state.
    Transition(RfqTransitionError),
    /// The caller may not move this request.
    Forbidden(AccessError),
    /// The entry could not be written, or the request's entries read; nothing changed.
    Unrecorded(TraitError),
}

/// The open requests, the writer their transitions are recorded through, and the task's
/// wake-up.
pub(crate) struct RfqDesk {
    writer: Arc<AuditWriter>,
    open: Mutex<BTreeMap<RfqKey, OpenRfq>>,
    /// A new request may have the nearest deadline.
    wake: Notify,
    /// Every request a previous run left open has been recorded expired.
    ready: AtomicBool,
}

impl RfqDesk {
    pub(crate) fn new(writer: Arc<AuditWriter>) -> Arc<Self> {
        Arc::new(Self {
            writer,
            open: Mutex::new(BTreeMap::new()),
            wake: Notify::new(),
            ready: AtomicBool::new(false),
        })
    }

    /// Whether the desk takes requests: the restart's expiries are recorded.
    pub(crate) fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    /// How many requests are open.
    pub(crate) async fn open_count(&self) -> usize {
        self.open.lock().await.len()
    }

    /// Every open request, by key.
    pub(crate) async fn open_rfqs(&self) -> Vec<OpenRfq> {
        self.open.lock().await.values().cloned().collect()
    }

    /// Every entry of the request `key`, in order.
    pub(crate) async fn entries(&self, key: &RfqKey) -> Result<Vec<AuditEntry>, TraitError> {
        let mut entries: Vec<AuditEntry> = Vec::new();
        loop {
            let after = entries.last().map_or(0, |entry| entry.id);
            let page = self
                .writer
                .store()
                .entity_page(RFQ_ENTITY, key.as_str(), after, MAX_ENTITY_PAGE)
                .await?;
            entries.extend(page.entries);
            if !page.more {
                return Ok(entries);
            }
        }
    }

    /// The request `key` names, open or recorded.
    pub(crate) async fn known(&self, key: &RfqKey) -> Result<Option<Known>, TraitError> {
        if let Some(open) = self.open.lock().await.get(key) {
            return Ok(Some(Known {
                receipt: open.admission.receipt.clone(),
                state: open.state,
            }));
        }
        self.recorded(key).await
    }

    /// The request `key` names as its entries record it: its receipt and its last entry's
    /// state. A request whose receipt retention removed is unknown.
    async fn recorded(&self, key: &RfqKey) -> Result<Option<Known>, TraitError> {
        let entries = self.entries(key).await?;
        let (Some(first), Some(last)) = (entries.first(), entries.last()) else {
            return Ok(None);
        };
        let Some(receipt) = receipt_of(first)? else {
            return Ok(None);
        };
        let state = state_of(&last.action)
            .ok_or_else(|| TraitError::SerializationError("an RFQ entry names no state".into()))?;
        Ok(Some(Known { receipt, state }))
    }

    /// Record each admission received and priced, and open it, unless the desk already
    /// knows its key or is full. Each is recorded whole or not at all; the answers are in
    /// order.
    pub(crate) async fn admit(&self, admissions: Vec<Admission>) -> Vec<Admitted> {
        let mut open = self.open.lock().await;
        let mut answers = Vec::with_capacity(admissions.len());
        let mut taking = Vec::new();
        for admission in admissions {
            let known = match open.get(&admission.rfq_key) {
                Some(rfq) => Ok(Some((rfq.admission.receipt.request.clone(), rfq.state))),
                None => self
                    .recorded(&admission.rfq_key)
                    .await
                    .map(|known| known.map(|known| (known.receipt.request, known.state))),
            };
            let answer = match known {
                Ok(Some((request, state))) if request == admission.receipt.request => {
                    Admitted::Retry(state)
                }
                Ok(Some(_)) => Admitted::Conflict,
                Ok(None) if open.len() + taking.len() >= MAX_OPEN_RFQS => Admitted::Full,
                Ok(None) => {
                    taking.push((answers.len(), admission));
                    Admitted::Accepted
                }
                Err(error) => Admitted::Unrecorded(error),
            };
            answers.push(answer);
        }
        // Two entries each, so a batch takes half the store's batch. A batch refused for
        // capacity is tried one by one, so what fits is recorded.
        let mut taking = taking.into_iter().peekable();
        while taking.peek().is_some() {
            let chunk: Vec<_> = taking.by_ref().take(MAX_AUDIT_BATCH / 2).collect();
            let admissions: Vec<&Admission> =
                chunk.iter().map(|(_, admission)| admission).collect();
            let recorded = match self.record_admissions(&admissions).await {
                Err(TraitError::CapacityExceeded) if chunk.len() > 1 => {
                    let mut each = Vec::with_capacity(chunk.len());
                    for admission in &admissions {
                        each.push(self.record_admissions(&[admission]).await);
                    }
                    each
                }
                recorded => vec![recorded; chunk.len()],
            };
            for ((at, admission), recorded) in chunk.into_iter().zip(recorded) {
                match recorded {
                    Ok(()) => {
                        let rfq = OpenRfq {
                            admission: Arc::new(admission),
                            state: RfqState::Priced,
                            quote: None,
                        };
                        open.insert(rfq.admission.rfq_key.clone(), rfq);
                    }
                    Err(error) => {
                        if let Some(answer) = answers.get_mut(at) {
                            *answer = Admitted::Unrecorded(error);
                        }
                    }
                }
            }
        }
        metrics::gauge!(OPEN_RFQS).set(open.len() as f64);
        drop(open);
        self.wake.notify_one();
        answers
    }

    async fn record_admissions(&self, admissions: &[&Admission]) -> Result<(), TraitError> {
        let mut entries = Vec::with_capacity(admissions.len() * 2);
        for admission in admissions {
            let received = Recorded::Received {
                receipt: &admission.receipt,
            };
            let priced = Recorded::Priced {
                pricing: &admission.pricing,
            };
            entries.push(entry(
                &admission.rfq_key,
                &received,
                None,
                admission.receipt.received_at,
            )?);
            entries.push(entry(
                &admission.rfq_key,
                &priced,
                None,
                admission.pricing.priced_at,
            )?);
        }
        self.append(entries).await
    }

    /// Move the request `key` as `record` says, sent by `user`, once `allowed` admits the
    /// caller to its receipt, open or not; its new state. A request past its deadline is
    /// recorded expired first, and the move refused.
    pub(crate) async fn transition(
        &self,
        key: &RfqKey,
        record: Recorded<'_>,
        user: String,
        allowed: impl FnOnce(&Receipt) -> Result<(), AccessError>,
    ) -> Result<RfqState, Refused> {
        let event = record
            .event()
            .ok_or(Refused::Unrecorded(TraitError::InvalidInput(
                "a request is received only at ingest",
            )))?;
        let mut open = self.open.lock().await;
        let Some(rfq) = open.get_mut(key) else {
            drop(open);
            let known = match self.recorded(key).await {
                Ok(Some(known)) => known,
                Ok(None) => return Err(Refused::Unknown),
                Err(error) => return Err(Refused::Unrecorded(error)),
            };
            allowed(&known.receipt).map_err(Refused::Forbidden)?;
            return Err(Refused::Transition(
                transition(known.state, event)
                    .err()
                    .unwrap_or(RfqTransitionError::NotAllowed(known.state)),
            ));
        };
        allowed(&rfq.admission.receipt).map_err(Refused::Forbidden)?;
        let now = crate::reference::now().map_err(|_| Refused::Unrecorded(OUT_OF_RANGE))?;
        if rfq.admission.receipt.request.deadline <= now {
            let expiry = Recorded::Expired {
                reason: ExpiryReason::Deadline,
            };
            let expired = entry(key, &expiry, None, now).map_err(Refused::Unrecorded)?;
            self.append(vec![expired])
                .await
                .map_err(Refused::Unrecorded)?;
            open.remove(key);
            metrics::gauge!(OPEN_RFQS).set(open.len() as f64);
            return Err(Refused::Transition(RfqTransitionError::Final(
                RfqState::Expired,
            )));
        }
        let state = transition(rfq.state, event).map_err(Refused::Transition)?;
        let moved = entry(key, &record, Some(user), now).map_err(Refused::Unrecorded)?;
        self.append(vec![moved])
            .await
            .map_err(Refused::Unrecorded)?;
        rfq.state = state;
        if let Recorded::Quoted { quote } = record {
            rfq.quote = Some(quote.clone());
        }
        if state.is_final() {
            open.remove(key);
            metrics::gauge!(OPEN_RFQS).set(open.len() as f64);
        }
        Ok(state)
    }

    /// Append `entries`, built without IDs, taking their IDs under the writer.
    async fn append(&self, mut entries: Vec<AuditEntry>) -> Result<(), TraitError> {
        let mut writer = self.writer.hold().await?;
        for entry in &mut entries {
            entry.id = writer.take_id().ok_or(TraitError::CapacityExceeded)?;
        }
        writer.append_all(&entries).await
    }

    /// Record `keys` expired for `reason` at `at`, in batches.
    async fn expire(
        &self,
        keys: &[RfqKey],
        reason: ExpiryReason,
        at: UtcTimestamp,
    ) -> Result<(), TraitError> {
        let expiry = Recorded::Expired { reason };
        for chunk in keys.chunks(MAX_AUDIT_BATCH) {
            let entries = chunk
                .iter()
                .map(|key| entry(key, &expiry, None, at))
                .collect::<Result<Vec<_>, _>>()?;
            self.append(entries).await?;
        }
        Ok(())
    }

    /// Record every open request whose deadline has passed expired, and close it; the time
    /// until the nearest deadline left, if any, or until a retry after a failed record.
    async fn expire_due(&self) -> Option<Duration> {
        let Ok(now) = crate::reference::now() else {
            return Some(RETRY);
        };
        let mut open = self.open.lock().await;
        let due: Vec<RfqKey> = open
            .iter()
            .filter(|(_, rfq)| rfq.admission.receipt.request.deadline <= now)
            .map(|(key, _)| key.clone())
            .collect();
        if let Err(error) = self.expire(&due, ExpiryReason::Deadline, now).await {
            tracing::warn!(%error, "RFQ desk: expiries could not be recorded; they are tried again within a second");
            return Some(RETRY);
        }
        for key in &due {
            open.remove(key);
        }
        metrics::gauge!(OPEN_RFQS).set(open.len() as f64);
        open.values()
            .map(|rfq| rfq.admission.receipt.request.deadline)
            .min()
            .map(|deadline| until(now, deadline))
    }

    /// Record every request a previous run left open expired, reason `restart`. They were
    /// received within twice the longest deadline before the last `rfq` entry; walking
    /// back from the log's end, the first entry of each request is its latest.
    async fn expire_left_open(&self) -> Result<usize, TraitError> {
        let store = self.writer.store();
        let mut latest: BTreeMap<String, RfqState> = BTreeMap::new();
        let mut cutoff: Option<i64> = None;
        let mut before = u64::MAX;
        'walk: loop {
            let page = store.entries_before(before, MAX_ENTITY_PAGE).await?;
            for entry in &page {
                before = entry.id;
                if cutoff.is_some_and(|cutoff| entry.timestamp < cutoff) {
                    break 'walk;
                }
                if entry.event_type != RFQ_EVENT {
                    continue;
                }
                cutoff.get_or_insert(entry.timestamp.saturating_sub(2 * MAX_RFQ_DEADLINE_MS));
                if let Some(state) = state_of(&entry.action) {
                    latest.entry(entry.entity_id.clone()).or_insert(state);
                }
            }
            if page.len() < MAX_ENTITY_PAGE {
                break;
            }
        }
        let left: Vec<RfqKey> = latest
            .into_iter()
            .filter(|(_, state)| !state.is_final())
            .filter_map(|(key, _)| RfqKey::parse(&key))
            .collect();
        let now = crate::reference::now().map_err(|_| OUT_OF_RANGE)?;
        self.expire(&left, ExpiryReason::Restart, now).await?;
        Ok(left.len())
    }

    /// Expire what a previous run left open, then take requests and expire each at its
    /// deadline, until `stop` turns true.
    pub(crate) async fn run(self: Arc<Self>, mut stop: watch::Receiver<bool>) {
        loop {
            match self.expire_left_open().await {
                Ok(expired) => {
                    if expired > 0 {
                        tracing::info!(
                            expired,
                            "RFQ desk: requests left open by the last run are expired"
                        );
                    }
                    break;
                }
                Err(error) => {
                    tracing::warn!(%error, "RFQ desk: requests left open by the last run could not be expired; the desk takes none until they are");
                    tokio::select! {
                        changed = stop.changed() => if changed.is_err() || *stop.borrow() { return },
                        _ = tokio::time::sleep(RETRY) => {}
                    }
                }
            }
        }
        self.ready.store(true, Ordering::Release);
        loop {
            let wait = self.expire_due().await;
            tokio::select! {
                biased;
                changed = stop.changed() => if changed.is_err() || *stop.borrow() { break },
                () = self.wake.notified() => {}
                () = tokio::time::sleep(wait.unwrap_or_default()), if wait.is_some() => {}
            }
        }
    }
}

/// The receipt a request's first entry records, or none if the entry is not its receipt,
/// as when retention removed it.
pub(crate) fn receipt_of(entry: &AuditEntry) -> Result<Option<Receipt>, TraitError> {
    if entry.action != RfqState::Received.as_str() {
        return Ok(None);
    }
    serde_json::from_str(entry.details.as_deref().unwrap_or_default())
        .map(Some)
        .map_err(|_| TraitError::SerializationError("an RFQ entry is unreadable".into()))
}

/// The state an entry's action names.
fn state_of(action: &str) -> Option<RfqState> {
    RfqState::ALL
        .into_iter()
        .find(|state| state.as_str() == action)
}

/// How long from `now` until `deadline`, none if it has passed.
fn until(now: UtcTimestamp, deadline: UtcTimestamp) -> Duration {
    (deadline.datetime() - now.datetime())
        .to_std()
        .unwrap_or_default()
}

/// The entry of `record` on the request `key` at `at`, sent by `user` when one is known,
/// its ID still to be taken.
fn entry(
    key: &RfqKey,
    record: &Recorded<'_>,
    user: Option<String>,
    at: UtcTimestamp,
) -> Result<AuditEntry, TraitError> {
    let details = RecordDetails {
        actor: record.event().map_or(RfqActor::Server, RfqEvent::actor),
        recorded_at: at,
        record,
    };
    let details = serde_json::to_string(&details)
        .map_err(|error| TraitError::SerializationError(error.to_string()))?;
    Ok(AuditEntry {
        id: 0,
        timestamp: at.datetime().timestamp_millis(),
        event_type: RFQ_EVENT.into(),
        entity_type: RFQ_ENTITY.into(),
        entity_id: key.as_str().to_owned(),
        user,
        action: record.state().as_str().into(),
        details: Some(details),
    })
}
