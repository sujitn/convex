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
//! `rfq` entry, so the scan reads only that window.
//!
//! A request's key is a digest of its source and the source's id for it: each is free
//! text up to 128 bytes, and an entity ID holds 128. A retry is recognised from the key's
//! first entry, so a source's id stays taken after its request leaves memory.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use convex_core::contracts::{ExactDecimal, InstrumentId, SourceId, UtcTimestamp, Version};
use convex_core::types::WidthFactor;
use convex_engine::ports::quote_event::QuoteInput;
use convex_engine::ports::rfq::{
    transition, ClientTier, DeskSide, ExpiryReason, RfqActor, RfqEvent, RfqRequest, RfqState,
    RfqTransitionError, MAX_RFQ_DEADLINE_MS,
};
use convex_engine::ports::storage::{AuditEntry, AuditFilter, Pagination, MAX_AUDIT_BATCH};
use convex_engine::ports::TraitError;
use serde::Serialize;
use tokio::sync::{watch, Mutex, MutexGuard, Notify};

use crate::audit_writer::AuditWriter;

/// The audit event type of an RFQ's transition.
pub(crate) const RFQ_EVENT: &str = "rfq";
/// The audit entity type of an RFQ's transition; the entity ID is its key.
pub(crate) const RFQ_ENTITY: &str = "rfq";
/// The most requests open at once; ingest refuses more with `RFQ_CAPACITY`.
pub(crate) const MAX_OPEN_RFQS: usize = 256;
/// Requests open.
pub const OPEN_RFQS: &str = "convex_rfqs_open";
/// Time from a request's receipt to its `priced` entry, in seconds.
pub const RFQ_PRICING_SECONDS: &str = "convex_rfq_pricing_seconds";
/// How soon a failed read or append is tried again.
const RETRY: Duration = Duration::from_secs(1);
/// The most entries one request has: received, priced, quoted and a final one.
const MAX_RFQ_ENTRIES: usize = 4;
/// Audit entries one restart page reads.
const RESTART_PAGE: usize = 1000;
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
#[derive(Debug, Clone, Serialize)]
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
    /// The check route's answer.
    Evaluated {
        status: serde_json::Value,
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

/// An open request.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OpenRfq {
    pub(crate) rfq_key: RfqKey,
    pub(crate) state: RfqState,
    #[serde(flatten)]
    pub(crate) receipt: Arc<Receipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pricing: Option<Arc<RfqPricing>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) quote: Option<SentQuote>,
}

/// One transition as recorded: the entry's details.
#[derive(Serialize)]
struct RecordDetails<'a> {
    rfq_key: &'a RfqKey,
    state: RfqState,
    actor: RfqActor,
    recorded_at: UtcTimestamp,
    #[serde(flatten)]
    record: Recorded<'a>,
}

/// What a transition carries.
#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Recorded<'a> {
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

impl<'a> Recorded<'a> {
    /// The record of `event`, with the quote it sends.
    fn of(event: RfqEvent, quote: Option<&'a SentQuote>) -> Option<Self> {
        Some(match event {
            RfqEvent::Quoted => Self::Quoted { quote: quote? },
            RfqEvent::Passed => Self::Passed,
            RfqEvent::Done => Self::Done,
            RfqEvent::TradedAway => Self::TradedAway,
            RfqEvent::Covered => Self::Covered,
            RfqEvent::Expired { reason } => Self::Expired { reason },
            RfqEvent::Cancelled => Self::Cancelled,
            RfqEvent::Priced => return None,
        })
    }
}

/// A request the desk knows: open, or recorded and no longer held.
#[derive(Debug)]
pub(crate) struct Known {
    /// The request as first received.
    pub(crate) request: RfqRequest,
    pub(crate) state: RfqState,
}

/// Why a transition was refused.
#[derive(Debug)]
pub(crate) enum Refused {
    /// The desk knows no request by the key.
    Unknown,
    /// The table refuses the event in the request's state.
    Transition(RfqTransitionError),
    /// The caller may not move this request.
    Forbidden(axum::response::Response),
    /// The entry could not be written; nothing changed.
    Unrecorded(TraitError),
}

/// A request just priced, to be admitted.
pub(crate) struct Admission {
    pub(crate) rfq_key: RfqKey,
    pub(crate) receipt: Receipt,
    pub(crate) pricing: RfqPricing,
}

/// The open requests, the writer their transitions are recorded through, and the task's
/// wake-up.
pub(crate) struct RfqDesk {
    writer: Arc<AuditWriter>,
    open: Mutex<BTreeMap<RfqKey, OpenRfq>>,
    /// Ingests run one at a time, so a key is admitted once.
    ingest: Mutex<()>,
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
            ingest: Mutex::new(()),
            wake: Notify::new(),
            ready: AtomicBool::new(false),
        })
    }

    /// Whether the desk takes requests: the restart's expiries are recorded.
    pub(crate) fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    /// The store entries are read from.
    pub(crate) fn records(&self) -> &Arc<dyn convex_engine::ports::storage::AuditStore> {
        self.writer.store()
    }

    /// Hold ingest: one ingest admits at a time.
    pub(crate) async fn hold_ingest(&self) -> MutexGuard<'_, ()> {
        self.ingest.lock().await
    }

    /// How many requests are open.
    pub(crate) async fn open_count(&self) -> usize {
        self.open.lock().await.len()
    }

    /// Every open request, by key.
    pub(crate) async fn open_rfqs(&self) -> Vec<OpenRfq> {
        self.open.lock().await.values().cloned().collect()
    }

    /// The request `key` names, open or recorded.
    pub(crate) async fn known(&self, key: &RfqKey) -> Result<Option<Known>, TraitError> {
        if let Some(open) = self.open.lock().await.get(key) {
            return Ok(Some(Known {
                request: open.receipt.request.clone(),
                state: open.state,
            }));
        }
        self.recorded(key).await
    }

    /// The request `key` names as its entries record it: its first entry's request and its
    /// last entry's state.
    async fn recorded(&self, key: &RfqKey) -> Result<Option<Known>, TraitError> {
        let page = self
            .writer
            .store()
            .entity_page(RFQ_ENTITY, key.as_str(), 0, MAX_RFQ_ENTRIES)
            .await?;
        let (Some(first), Some(last)) = (page.entries.first(), page.entries.last()) else {
            return Ok(None);
        };
        #[derive(serde::Deserialize)]
        struct Received {
            request: RfqRequest,
        }
        let unreadable = |_| TraitError::SerializationError("an RFQ entry is unreadable".into());
        let received: Received = serde_json::from_str(first.details.as_deref().unwrap_or_default())
            .map_err(unreadable)?;
        let state = state_of(&last.action)
            .ok_or_else(|| TraitError::SerializationError("an RFQ entry names no state".into()))?;
        Ok(Some(Known {
            request: received.request,
            state,
        }))
    }

    /// Record each admission received and priced, and open it. Each is recorded whole or
    /// not at all; the result says which, in order.
    pub(crate) async fn admit(
        &self,
        admissions: Vec<Admission>,
    ) -> Vec<Result<RfqState, TraitError>> {
        let mut open = self.open.lock().await;
        let mut results = Vec::with_capacity(admissions.len());
        // Two entries each, so a batch takes half the store's batch.
        let mut admissions = admissions.into_iter().peekable();
        while admissions.peek().is_some() {
            let chunk: Vec<Admission> = admissions.by_ref().take(MAX_AUDIT_BATCH / 2).collect();
            let recorded = self.record_admissions(&chunk).await;
            for admission in chunk {
                match &recorded {
                    Ok(()) => {
                        let rfq = OpenRfq {
                            rfq_key: admission.rfq_key.clone(),
                            state: RfqState::Priced,
                            receipt: Arc::new(admission.receipt),
                            pricing: Some(Arc::new(admission.pricing)),
                            quote: None,
                        };
                        open.insert(admission.rfq_key, rfq);
                        results.push(Ok(RfqState::Priced));
                    }
                    Err(error) => results.push(Err(error.clone())),
                }
            }
        }
        metrics::gauge!(OPEN_RFQS).set(open.len() as f64);
        drop(open);
        self.wake.notify_one();
        results
    }

    async fn record_admissions(&self, admissions: &[Admission]) -> Result<(), TraitError> {
        let mut writer = self.writer.hold().await?;
        let mut entries = Vec::with_capacity(admissions.len() * 2);
        for admission in admissions {
            let received = Recorded::Received {
                receipt: &admission.receipt,
            };
            let priced = Recorded::Priced {
                pricing: &admission.pricing,
            };
            entries.push(entry(
                writer.take_id(),
                &admission.rfq_key,
                RfqState::Received,
                (RfqActor::Server, None),
                admission.receipt.received_at,
                received,
            )?);
            entries.push(entry(
                writer.take_id(),
                &admission.rfq_key,
                RfqState::Priced,
                (RfqActor::Server, None),
                admission.pricing.priced_at,
                priced,
            )?);
        }
        writer.append_all(&entries).await
    }

    /// Move the open request `key` by `event`, sent by the holder of `fingerprint` (none for
    /// the server), once `allowed` admits the caller. A trader's quote carries `quote`. A
    /// request past its deadline is recorded expired first, and the event refused.
    pub(crate) async fn transition(
        &self,
        key: &RfqKey,
        event: RfqEvent,
        quote: Option<SentQuote>,
        fingerprint: Option<String>,
        allowed: impl FnOnce(&OpenRfq) -> Result<(), axum::response::Response>,
    ) -> Result<OpenRfq, Refused> {
        let mut open = self.open.lock().await;
        let Some(rfq) = open.get(key) else {
            drop(open);
            return match self.recorded(key).await {
                Ok(Some(known)) => Err(Refused::Transition(
                    transition(known.state, event)
                        .err()
                        .unwrap_or(RfqTransitionError::NotAllowed(known.state)),
                )),
                Ok(None) => Err(Refused::Unknown),
                Err(error) => Err(Refused::Unrecorded(error)),
            };
        };
        allowed(rfq).map_err(Refused::Forbidden)?;
        let now = crate::reference::now().map_err(|_| Refused::Unrecorded(OUT_OF_RANGE))?;
        if rfq.receipt.request.deadline <= now {
            self.record_moves(&[Move::expiry(key, ExpiryReason::Deadline)], now, None)
                .await
                .map_err(Refused::Unrecorded)?;
            open.remove(key);
            metrics::gauge!(OPEN_RFQS).set(open.len() as f64);
            return Err(Refused::Transition(RfqTransitionError::Final(
                RfqState::Expired,
            )));
        }
        let state = transition(rfq.state, event).map_err(Refused::Transition)?;
        let moved = Move {
            key,
            event,
            to: state,
            quote: quote.as_ref(),
        };
        self.record_moves(&[moved], now, fingerprint)
            .await
            .map_err(Refused::Unrecorded)?;
        let mut moved = match open.get(key) {
            Some(rfq) => rfq.clone(),
            None => return Err(Refused::Unknown),
        };
        moved.state = state;
        if quote.is_some() {
            moved.quote = quote;
        }
        if state.is_final() {
            open.remove(key);
            metrics::gauge!(OPEN_RFQS).set(open.len() as f64);
        } else {
            open.insert(key.clone(), moved.clone());
        }
        Ok(moved)
    }

    /// Record `moves` at `at`, in one batch, each sent by the holder of `fingerprint`, or
    /// by the server.
    async fn record_moves(
        &self,
        moves: &[Move<'_>],
        at: UtcTimestamp,
        fingerprint: Option<String>,
    ) -> Result<(), TraitError> {
        let mut writer = self.writer.hold().await?;
        let mut entries = Vec::with_capacity(moves.len());
        for moved in moves {
            let record = Recorded::of(moved.event, moved.quote)
                .ok_or(TraitError::InvalidInput("the server prices only at ingest"))?;
            entries.push(entry(
                writer.take_id(),
                moved.key,
                moved.to,
                (moved.event.actor(), fingerprint.clone()),
                at,
                record,
            )?);
        }
        writer.append_all(&entries).await
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
            .filter(|(_, rfq)| rfq.receipt.request.deadline <= now)
            .map(|(key, _)| key.clone())
            .collect();
        for chunk in due.chunks(MAX_AUDIT_BATCH) {
            let moves: Vec<_> = chunk
                .iter()
                .map(|key| Move::expiry(key, ExpiryReason::Deadline))
                .collect();
            if let Err(error) = self.record_moves(&moves, now, None).await {
                tracing::warn!(%error, "RFQ desk: expiries could not be recorded; they are tried again within a second");
                return Some(RETRY);
            }
            for key in chunk {
                open.remove(key);
            }
        }
        metrics::gauge!(OPEN_RFQS).set(open.len() as f64);
        open.values()
            .map(|rfq| rfq.receipt.request.deadline)
            .min()
            .map(|deadline| until(now, deadline))
    }

    /// Record every request a previous run left open expired, reason `restart`. They were
    /// received within twice the longest deadline before the last `rfq` entry.
    async fn expire_left_open(&self) -> Result<usize, TraitError> {
        let store = self.writer.store();
        let mut filter = AuditFilter {
            event_type: Some(RFQ_EVENT.into()),
            ..AuditFilter::default()
        };
        let total = store.query(&filter, &Pagination::new(0, 0)).await?.total;
        let Some(last_offset) = total
            .checked_sub(1)
            .and_then(|last| usize::try_from(last).ok())
        else {
            return Ok(0);
        };
        let last = store
            .query(&filter, &Pagination::new(last_offset, 1))
            .await?
            .items
            .into_iter()
            .next();
        let Some(last) = last else {
            return Ok(0);
        };
        filter.from = Some(last.timestamp.saturating_sub(2 * MAX_RFQ_DEADLINE_MS));
        let mut states: BTreeMap<String, RfqState> = BTreeMap::new();
        let mut offset = 0;
        loop {
            let page = store
                .query(&filter, &Pagination::new(offset, RESTART_PAGE))
                .await?;
            for entry in &page.items {
                if let Some(state) = state_of(&entry.action) {
                    states.insert(entry.entity_id.clone(), state);
                }
            }
            offset = offset.saturating_add(page.items.len());
            if page.items.len() < RESTART_PAGE || offset as u64 >= page.total {
                break;
            }
        }
        let left: Vec<RfqKey> = states
            .into_iter()
            .filter(|(_, state)| !state.is_final())
            .filter_map(|(key, _)| RfqKey::parse(&key))
            .collect();
        let now = crate::reference::now().map_err(|_| OUT_OF_RANGE)?;
        for chunk in left.chunks(MAX_AUDIT_BATCH) {
            let moves: Vec<_> = chunk
                .iter()
                .map(|key| Move::expiry(key, ExpiryReason::Restart))
                .collect();
            self.record_moves(&moves, now, None).await?;
        }
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

/// One request's move by an event, to the state it reaches.
struct Move<'a> {
    key: &'a RfqKey,
    event: RfqEvent,
    to: RfqState,
    /// The quote a trader's `quoted` sends.
    quote: Option<&'a SentQuote>,
}

impl<'a> Move<'a> {
    /// The server's expiry of `key`.
    fn expiry(key: &'a RfqKey, reason: ExpiryReason) -> Self {
        Self {
            key,
            event: RfqEvent::Expired { reason },
            to: RfqState::Expired,
            quote: None,
        }
    }
}

/// One transition's entry, with ID `id`, sent by `actor`, whose holder is `user` when one
/// is known.
fn entry(
    id: Option<u64>,
    key: &RfqKey,
    state: RfqState,
    (actor, user): (RfqActor, Option<String>),
    at: UtcTimestamp,
    record: Recorded<'_>,
) -> Result<AuditEntry, TraitError> {
    let id = id.ok_or(TraitError::CapacityExceeded)?;
    let details = RecordDetails {
        rfq_key: key,
        state,
        actor,
        recorded_at: at,
        record,
    };
    let details = serde_json::to_string(&details)
        .map_err(|error| TraitError::SerializationError(error.to_string()))?;
    Ok(AuditEntry {
        id,
        timestamp: at.datetime().timestamp_millis(),
        event_type: RFQ_EVENT.into(),
        entity_type: RFQ_ENTITY.into(),
        entity_id: key.as_str().to_owned(),
        user,
        action: state.as_str().into(),
        details: Some(details),
    })
}
