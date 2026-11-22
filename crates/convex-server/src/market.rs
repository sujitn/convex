//! Live quote ingress: `POST /api/v1/market/quotes` feeds the Live runtime's subscription.
//!
//! The Live runtime stops on any event it cannot accept, so this ingress is the only
//! gate. It admits a batch only when every event would be accepted, stamps receipt
//! time and sends under one lock so delivery order matches receipt order, and refuses
//! a batch rather than let the runtime's queue overflow. Receipt times never step back:
//! while the wall clock is behind the last stamp by at most [`MAX_CLOCK_STEP_MS`], a
//! batch is stamped with the last stamp.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use convex_core::contracts::UtcTimestamp;
use convex_core::ids::InstrumentId;
use convex_engine::ports::market_data::{QuoteSource, RawQuote, SourceType, StreamReceiver};
use convex_engine::ports::quote_event::{QuoteEvent, QuoteKey};
use convex_engine::ports::TraitError;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::warn;

use crate::desk_levels::{DeskLevel, DeskLevels, DeskMark};
use crate::error::ApiError;

/// Events per request.
pub const MAX_BATCH: usize = 256;
/// Events queued for the runtime; admission past this answers 429.
const CAPACITY: usize = 1024;
/// Rejected rows reported per response.
const MAX_ROW_ERRORS: usize = 64;
/// A backward wall-clock step the ingress absorbs by repeating its last receipt time.
/// While the clock is further behind, every batch is refused with 503.
pub const MAX_CLOCK_STEP_MS: i64 = 5_000;

/// The Live runtime's only quote source, fed by [`QuoteIngress::submit`] and, for the
/// desk's levels, by the desk level route.
pub struct QuoteIngress {
    keys: HashSet<QuoteKey>,
    /// The desk's level source, which only the desk level route writes.
    desk: Option<DeskLevels>,
    queue: Mutex<Queue>,
}

struct Queue {
    sender: broadcast::Sender<QuoteEvent>,
    last_receive_ms: i64,
    /// The clock is behind the last stamp; the step has been logged.
    stepped: bool,
    /// The last sequence of a desk mark. An image is taken at any higher sequence, so one
    /// counter numbers every desk bond's quotes.
    desk_sequence: u64,
    /// Each desk bond's last mark queued by this process.
    desk_marks: HashMap<InstrumentId, DeskMark>,
}

impl Queue {
    /// Receipt time for a batch read at `clock_ms`: the clock, or the last stamp while
    /// the clock is behind it by at most [`MAX_CLOCK_STEP_MS`].
    fn stamp(&mut self, clock_ms: i64) -> Result<i64, IngressError> {
        let behind = self.last_receive_ms.saturating_sub(clock_ms);
        if behind <= 0 {
            self.stepped = false;
            return Ok(clock_ms);
        }
        if !self.stepped {
            self.stepped = true;
            warn!(
                behind_ms = behind,
                bound_ms = MAX_CLOCK_STEP_MS,
                "server clock moved backwards behind the last quote receipt time"
            );
        }
        if behind > MAX_CLOCK_STEP_MS {
            return Err(IngressError::Unavailable(
                "server clock moved backwards by more than 5 s",
            ));
        }
        Ok(self.last_receive_ms)
    }

    /// Queue `events`, stamped `now`, as one batch, or none of them when the runtime's
    /// queue cannot take them all.
    fn send(&mut self, events: Vec<QuoteEvent>, now: i64) -> Result<Receipt, IngressError> {
        if self.sender.len() + events.len() > CAPACITY {
            return Err(IngressError::Busy);
        }
        let accepted = events.len();
        for event in events {
            self.sender
                .send(event)
                .map_err(|_| IngressError::Unavailable("live runtime stopped"))?;
        }
        self.last_receive_ms = now;
        Ok(Receipt {
            accepted,
            receive_time_ms: now,
        })
    }
}

fn wall_clock_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Queued events and the server receipt time stamped on all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Receipt {
    /// Events queued for the runtime, which prices them after this returns.
    pub accepted: usize,
    /// Receipt time that replaced each event's supplied `receive_time_ms`.
    pub receive_time_ms: i64,
}

/// One refused event in a batch that queued nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowError {
    /// Position in the submitted batch.
    pub index: usize,
    /// Stable rejection code.
    pub code: &'static str,
    /// Human-readable reason, without event content.
    pub reason: &'static str,
}

/// Why a batch queued nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngressError {
    /// Empty, or more than [`MAX_BATCH`] events.
    Batch,
    /// Events the running plan cannot accept.
    Rejected {
        /// The first 64 refused rows.
        rows: Vec<RowError>,
        /// More rows were refused than reported.
        truncated: bool,
    },
    /// The runtime's queue cannot take the whole batch now.
    Busy,
    /// No runtime is subscribed, or receipt time cannot be ordered.
    Unavailable(&'static str),
}

impl QuoteIngress {
    /// Accept events only for the plan's selected instrument and source pairs.
    pub fn new(keys: impl IntoIterator<Item = QuoteKey>) -> Self {
        let (sender, _) = broadcast::channel(CAPACITY);
        Self {
            keys: keys.into_iter().collect(),
            desk: None,
            queue: Mutex::new(Queue {
                sender,
                last_receive_ms: 0,
                stepped: false,
                desk_sequence: 0,
                desk_marks: HashMap::new(),
            }),
        }
    }

    /// Reserve the desk's level source for the desk level route: a feed's event on it is
    /// refused, so the ingress alone numbers that source's quotes.
    pub fn with_desk_levels(mut self, desk: DeskLevels) -> Self {
        self.desk = Some(desk);
        self
    }

    /// The desk's level source and its bonds, when this server takes desk levels.
    pub(crate) fn desk_levels(&self) -> Option<&DeskLevels> {
        self.desk.as_ref()
    }

    /// The queue, once a runtime is subscribed to it.
    fn admit(&self) -> Result<MutexGuard<'_, Queue>, IngressError> {
        let queue = self
            .queue
            .lock()
            .map_err(|_| IngressError::Unavailable("quote ingress is unavailable"))?;
        if queue.sender.receiver_count() == 0 {
            return Err(IngressError::Unavailable("no live runtime is subscribed"));
        }
        Ok(queue)
    }

    /// Stamp server receipt time on every event and queue the whole batch, or nothing.
    pub fn submit(&self, events: Vec<QuoteEvent>) -> Result<Receipt, IngressError> {
        self.submit_with_clock(events, wall_clock_ms)
    }

    /// [`Self::submit`] with the wall clock read from `clock`, under the queue lock so
    /// clock readings follow queue order.
    fn submit_with_clock(
        &self,
        mut events: Vec<QuoteEvent>,
        clock: impl FnOnce() -> i64,
    ) -> Result<Receipt, IngressError> {
        if events.is_empty() || events.len() > MAX_BATCH {
            return Err(IngressError::Batch);
        }
        let mut queue = self.admit()?;
        // The runtime refuses receipt times out of order; its cutoffs never fall below one.
        let now = queue.stamp(clock())?;
        let desk_source = self.desk.as_ref().map(DeskLevels::source);
        let mut rows = Vec::new();
        for (index, event) in events.iter_mut().enumerate() {
            event.receive_time_ms = now;
            let refusal = if desk_source == Some(event.key.source.as_str()) {
                Some((
                    "DESK_LEVEL_SOURCE",
                    "the desk's levels are marked through /api/v1/market/levels only",
                ))
            } else if event.validate().is_err() {
                Some(("INVALID_EVENT", "event fails the canonical quote contract"))
            } else if !self.keys.contains(&event.key) {
                Some((
                    "UNKNOWN_KEY",
                    "instrument and source are not in the running plan",
                ))
            } else if event.source_time_ms > now {
                Some(("FUTURE_SOURCE_TIME", "source time is after server receipt"))
            } else {
                None
            };
            if let Some((code, reason)) = refusal {
                rows.push(RowError {
                    index,
                    code,
                    reason,
                });
            }
        }
        rejected(rows)?;
        queue.send(events, now)
    }

    /// Queue the desk's marks as one batch, or none of them. Each is a `manual` image of
    /// its bond's level field on the desk's source, numbered and stamped under the queue
    /// lock: a mark is observed when it is received, so its source time is its receipt
    /// time. The caller has checked each mark against the plan and the bond's rule.
    pub(crate) fn mark_desk(&self, levels: &[DeskLevel]) -> Result<Receipt, IngressError> {
        let desk = self.desk.as_ref().ok_or(IngressError::Unavailable(
            "this server takes no desk levels",
        ))?;
        if levels.is_empty() || levels.len() > MAX_BATCH {
            return Err(IngressError::Batch);
        }
        let mut queue = self.admit()?;
        let now = queue.stamp(wall_clock_ms())?;
        let marked_at = chrono::DateTime::from_timestamp_millis(now)
            .and_then(|at| UtcTimestamp::from_datetime(at).ok())
            .ok_or(IngressError::Unavailable("server clock is out of range"))?;
        let first = queue.desk_sequence + 1;
        let quotes = levels
            .iter()
            .zip(first..)
            .map(|(level, sequence)| desk.quote(level, sequence, now))
            .collect();
        let receipt = queue.send(quotes, now)?;
        queue.desk_sequence += levels.len() as u64;
        for level in levels {
            queue.desk_marks.insert(
                level.instrument_id.clone(),
                DeskMark::queued(level, marked_at),
            );
        }
        Ok(receipt)
    }

    /// Each desk bond's last mark queued by this process.
    pub(crate) fn desk_marks(&self) -> Result<HashMap<InstrumentId, DeskMark>, IngressError> {
        let queue = self
            .queue
            .lock()
            .map_err(|_| IngressError::Unavailable("quote ingress is unavailable"))?;
        Ok(queue.desk_marks.clone())
    }
}

/// The refusal of a batch with `rows` refused, reporting the first [`MAX_ROW_ERRORS`];
/// none when no row is.
pub(crate) fn rejected(mut rows: Vec<RowError>) -> Result<(), IngressError> {
    if rows.is_empty() {
        return Ok(());
    }
    let truncated = rows.len() > MAX_ROW_ERRORS;
    rows.truncate(MAX_ROW_ERRORS);
    Err(IngressError::Rejected { rows, truncated })
}

#[async_trait]
impl QuoteSource for QuoteIngress {
    fn source_type(&self) -> SourceType {
        SourceType::Streaming
    }

    async fn get_quote(&self, _: &InstrumentId) -> Result<Option<RawQuote>, TraitError> {
        Err(TraitError::Unsupported(
            "live quotes reach only the runtime subscription",
        ))
    }

    async fn get_quotes(&self, _: &[InstrumentId]) -> Result<Vec<RawQuote>, TraitError> {
        Err(TraitError::Unsupported(
            "live quotes reach only the runtime subscription",
        ))
    }

    /// One subscriber, so the queue length is that runtime's backlog.
    async fn subscribe(
        &self,
        _: &[InstrumentId],
    ) -> Result<StreamReceiver<QuoteEvent>, TraitError> {
        let queue = self
            .queue
            .lock()
            .map_err(|_| TraitError::SourceNotAvailable("quote ingress is unavailable".into()))?;
        if queue.sender.receiver_count() != 0 {
            return Err(TraitError::Conflict(
                "live quotes already have a subscriber",
            ));
        }
        Ok(StreamReceiver::new(queue.sender.subscribe()))
    }

    async fn unsubscribe(&self, _: &[InstrumentId]) -> Result<(), TraitError> {
        Ok(())
    }
}

/// `POST /api/v1/market/quotes` body. Each event's `receive_time_ms` is replaced by
/// the server's receipt time; any value, such as 0, may be sent.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteBatch {
    /// Canonical events, 1 through [`MAX_BATCH`].
    pub events: Vec<QuoteEvent>,
}

/// Queue canonical events for the Live runtime. 202 means queued, not yet priced;
/// results arrive on the WebSocket and `/api/v1/runtime/results`. A body that is not
/// a JSON `QuoteBatch` is 400, so 422 always means per-row `QUOTES_REJECTED`.
pub async fn post_quotes(
    ingress: Option<Extension<Arc<QuoteIngress>>>,
    payload: Result<Json<QuoteBatch>, JsonRejection>,
) -> Response {
    let batch = match payload {
        Ok(Json(batch)) => batch,
        Err(rejection) => return ApiError::invalid(rejection.body_text()).into_response(),
    };
    let Some(Extension(ingress)) = ingress else {
        return ApiError::unavailable("this server has no live runtime").into_response();
    };
    let started = std::time::Instant::now();
    let submitted = ingress.submit(batch.events);
    convex_engine::telemetry::observe(convex_engine::telemetry::INGRESS_SECONDS, started);
    match submitted {
        Ok(receipt) => accepted(receipt),
        Err(error) => ingress_error(error),
    }
}

/// The reply to a batch the ingress queued: 202, its count and receipt time.
pub(crate) fn accepted(receipt: Receipt) -> Response {
    (StatusCode::ACCEPTED, Json(receipt)).into_response()
}

/// The reply to a batch the ingress queued nothing of. A refused row is 422
/// `QUOTES_REJECTED`, with the first rows' codes.
pub(crate) fn ingress_error(error: IngressError) -> Response {
    match error {
        IngressError::Batch => {
            ApiError::invalid(format!("a batch holds 1 to {MAX_BATCH} events")).into_response()
        }
        IngressError::Busy => {
            ApiError::overloaded("the live runtime's queue is full").into_response()
        }
        IngressError::Unavailable(reason) => ApiError::unavailable(reason).into_response(),
        IngressError::Rejected { rows, truncated } => {
            let rows: Vec<_> = rows
                .iter()
                .map(|e| serde_json::json!({"index":e.index, "code":e.code, "reason":e.reason}))
                .collect();
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({
                    "error": "batch rejected; nothing was queued",
                    "code": "QUOTES_REJECTED",
                    "retryable": false,
                    "rows": rows,
                    "truncated": truncated,
                })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use convex_engine::ports::quote_event::*;

    fn key(source: &str) -> QuoteKey {
        QuoteKey {
            instrument_id: InstrumentId::new("BOND-001"),
            source: source.into(),
        }
    }

    fn event(sequence: u64, source_time_ms: i64) -> QuoteEvent {
        QuoteEvent {
            schema_version: 1,
            key: key("feed-a"),
            session: "session-1".into(),
            sequence,
            source_time_ms,
            receive_time_ms: 0,
            quality: QuoteQuality::Live,
            entitled: true,
            entitlement_tags: vec![],
            evidence_reference: None,
            event: QuoteEventKind::Heartbeat,
        }
    }

    fn now() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    #[tokio::test]
    async fn no_subscriber_is_unavailable_and_one_runtime_may_subscribe() {
        let ingress = QuoteIngress::new([key("feed-a")]);
        assert_eq!(
            ingress.submit(vec![event(1, now())]),
            Err(IngressError::Unavailable("no live runtime is subscribed"))
        );
        let _runtime = ingress.subscribe(&[]).await.unwrap();
        assert!(matches!(
            ingress.subscribe(&[]).await,
            Err(TraitError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn refused_rows_queue_nothing() {
        let ingress = QuoteIngress::new([key("feed-a")]);
        let mut runtime = ingress.subscribe(&[]).await.unwrap();
        let mut other = event(1, now());
        other.key = key("feed-b");
        let mut invalid = event(1, now());
        invalid.schema_version = 2;
        let future = event(1, now() + 60_000);
        let Err(IngressError::Rejected { rows, truncated }) =
            ingress.submit(vec![event(1, now()), other, invalid, future])
        else {
            panic!("expected rejected rows");
        };
        assert!(!truncated);
        let codes: Vec<_> = rows.iter().map(|r| (r.index, r.code)).collect();
        assert_eq!(
            codes,
            [
                (1, "UNKNOWN_KEY"),
                (2, "INVALID_EVENT"),
                (3, "FUTURE_SOURCE_TIME")
            ]
        );
        ingress.submit(vec![event(2, now())]).unwrap();
        assert_eq!(runtime.recv().await.unwrap().sequence, 2);
        assert_eq!(ingress.submit(vec![]), Err(IngressError::Batch));
        assert_eq!(
            ingress.submit(vec![event(1, now()); MAX_BATCH + 1]),
            Err(IngressError::Batch)
        );
    }

    #[tokio::test]
    async fn a_full_queue_refuses_before_the_runtime_lags_and_stamps_stay_ordered() {
        let ingress = QuoteIngress::new([key("feed-a")]);
        let mut runtime = ingress.subscribe(&[]).await.unwrap();
        let mut stamps = Vec::new();
        for _ in 0..CAPACITY / MAX_BATCH {
            let receipt = ingress.submit(vec![event(1, now()); MAX_BATCH]).unwrap();
            stamps.push(receipt.receive_time_ms);
        }
        assert_eq!(
            ingress.submit(vec![event(1, now())]),
            Err(IngressError::Busy)
        );
        let mut previous = i64::MIN;
        for _ in 0..CAPACITY {
            let event = runtime.recv().await.unwrap();
            assert!(event.receive_time_ms >= previous);
            assert!(stamps.contains(&event.receive_time_ms));
            previous = event.receive_time_ms;
        }
        ingress.submit(vec![event(1, now())]).unwrap();
        assert!(runtime.recv().await.is_ok());
    }

    #[tokio::test]
    async fn a_clock_behind_by_up_to_the_bound_repeats_the_last_stamp_and_beyond_it_refuses() {
        let ingress = QuoteIngress::new([key("feed-a")]);
        let mut runtime = ingress.subscribe(&[]).await.unwrap();
        let t = 1_700_000_000_000;
        let stamped = |clock: i64, events| {
            ingress
                .submit_with_clock(events, || clock)
                .map(|r| r.receive_time_ms)
        };
        assert_eq!(stamped(t, vec![event(1, t)]), Ok(t));
        // Behind by the bound: the last stamp, and source times are checked against it.
        assert_eq!(
            stamped(t - MAX_CLOCK_STEP_MS, vec![event(2, t - 10), event(3, t)]),
            Ok(t)
        );
        let Err(IngressError::Rejected { rows, .. }) = stamped(t - 1, vec![event(4, t + 1)]) else {
            panic!("a source time after the stamp is in the future");
        };
        assert_eq!(rows[0].code, "FUTURE_SOURCE_TIME");
        let beyond = t - MAX_CLOCK_STEP_MS - 1;
        assert_eq!(
            stamped(beyond, vec![event(4, beyond)]),
            Err(IngressError::Unavailable(
                "server clock moved backwards by more than 5 s"
            ))
        );
        // Back within the bound, the last stamp again; past it, the clock.
        assert_eq!(stamped(t - 1, vec![event(4, t - 1)]), Ok(t));
        assert_eq!(stamped(t + 1, vec![event(5, t + 1)]), Ok(t + 1));
        for expected in [(1, t), (2, t), (3, t), (4, t), (5, t + 1)] {
            let event = runtime.recv().await.unwrap();
            assert_eq!((event.sequence, event.receive_time_ms), expected);
        }
    }

    #[test]
    fn a_clock_step_is_logged_once_until_the_clock_catches_up() {
        let (sender, _) = broadcast::channel(1);
        let mut queue = Queue {
            sender,
            last_receive_ms: 1_000,
            stepped: false,
            desk_sequence: 0,
            desk_marks: HashMap::new(),
        };
        assert_eq!(queue.stamp(900), Ok(1_000));
        assert!(queue.stepped);
        assert_eq!(queue.stamp(950), Ok(1_000));
        assert!(queue.stepped);
        assert_eq!(queue.stamp(1_000), Ok(1_000));
        assert!(!queue.stepped);
    }
}
