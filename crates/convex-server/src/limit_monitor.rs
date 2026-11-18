//! The limit monitor.
//!
//! One task, off the tick path, watches every book with a non-empty limit set (at most
//! 64). It listens to the runtime's results stream as a WebSocket
//! session does, and after each published tick evaluates the watched books that hold a
//! revalued instrument. It also evaluates a book when its positions or its limit set
//! commit, which [`PricingEngine::subscribe_reference_commits`] reports.
//!
//! An evaluation reads nothing from the repository: each watched set and book is reread
//! only when it commits, and priced bond terms are kept by instrument version, which never
//! changes. A monitor that falls behind either stream evaluates every watched book once
//! on the latest results; it never queues. The server runs it on a thread of its own, so
//! its evaluations and audit commits never hold a worker serving requests and ticks.
//!
//! A status *change* is recorded before it is published: one `AuditStore` record per
//! limit whose status differs from its last recorded one, then one `limit_status` frame.
//! One evaluation of a book writes all its records in one `append_all`, a single
//! transaction, so a restart's first pass is one commit a book rather than one a
//! limit. A failed batch records none of the book's changes and keeps their last recorded
//! statuses, so a later evaluation records them again; a book with a failed batch is
//! evaluated again within a second, unless the store refused it for capacity, which a
//! second later would not change. A restart begins with nothing recorded, so its first
//! evaluation records every limit's status as a change from `unevaluated`, naming the
//! start. A stop is seen between books, so shutdown waits for one book's commit at most;
//! what was not recorded is recorded at the next start.
//!
//! A `breach` or `unknown` change carries the acknowledgement in force for its limit at
//! the evaluation. The monitor rereads a watched book's acknowledgements when they
//! commit, as it rereads its set; an acknowledgement changes no status, so it records and
//! publishes nothing of its own.
//!
//! A temporary excess changes a status twice: when it is approved, which is a
//! commit, and when it expires, which is nothing the streams report. So the earliest expiry
//! still to come among the watched sets is a deadline beside the retry timer, and the
//! books holding it are evaluated when it passes. A wall clock that steps
//! back would apply an expired excess again for the length of the step, so each book keeps
//! the latest expiry this monitor has passed and reads its excesses no earlier than that.
//!
//! A clear hold damps a value hovering at a bound. A change to a more severe
//! status (`within` < `warning` < `unknown` < `breach`) is recorded at once. A change to a
//! less severe one is recorded once the limit has stayed less severe than its recorded
//! status for the hold: the monitor records the status of its first evaluation at or after
//! the hold's end, and a return to the recorded status, or past it, starts the hold afresh
//! next time. The hold's end is a deadline beside an excess's expiry. Status and the
//! pre-trade check read no record, so they still show the status of the moment. A hold of
//! zero records every change at once.
//!
//! Retention: with `limits.record_retention_days` set, the monitor, the
//! store's only writer, prunes `limit_status` records older than the period. At start,
//! after it reads the sets and acknowledgements and before it records any book, it prunes
//! keeping only the records acknowledgements in force name, so a store at its cap records
//! the restate that follows. Then once an hour it prunes again, also keeping each watched
//! limit's latest record since the start, the evidence for its current status. A limit not
//! recorded since the start (a book refused for capacity, or a set whose book is absent)
//! has no latest record to keep, so its records go by age, as at the start; its next
//! record restates it. The store never removes its last entry, so IDs continue. An hourly
//! pass makes one call, at most 4,096 removals in one transaction, between evaluations, so
//! a pass waits at most one call. A book refused for capacity is evaluated again after a
//! call that removes anything.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use convex_core::contracts::{ExactDecimal, UtcTimestamp, ValuationSubject, Version};
use convex_engine::ports::limits::{
    LimitAcknowledgement, LimitDefinition, LimitEvaluation, LimitId, LimitMeasure, LimitStatus,
};
use convex_engine::ports::reference_master::*;
use convex_engine::ports::reference_repository::*;
use convex_engine::ports::runtime::PublishedValuation;
use convex_engine::ports::storage::{
    AuditEntry, AuditPrune, AuditStore, Pruned, MAX_AUDIT_BATCH, MAX_PRUNE_BATCH, MAX_PRUNE_KEEP,
};
use convex_engine::ports::TraitError;
use convex_engine::PricingEngine;
use serde::Serialize;
use tokio::sync::{broadcast, watch};

use crate::books::{accepted_results, book_of};
use crate::limits::{
    evaluate_book, named_instruments, read_priced_terms, set_of, Accepted, PricedTerms,
    MAX_LIMITED_BOOKS,
};

/// The audit event type of a recorded status change.
pub const LIMIT_STATUS_EVENT: &str = "limit_status";
/// The audit entity type of a recorded status change; the entity ID is the book's.
pub const LIMIT_STATUS_ENTITY: &str = "book";
/// Time per evaluation pass, from its trigger's results to its last frame.
pub const MONITOR_SECONDS: &str = "convex_limit_monitor_seconds";
/// Limits of a watched book whose last recorded status is `breach`.
pub const BREACHES: &str = "convex_limit_breaches";
/// Limits of a watched book whose last recorded status is `unknown`.
pub const UNKNOWNS: &str = "convex_limit_unknown";
/// Status changes recorded.
pub const CHANGES: &str = "convex_limit_status_changes_total";
/// Status changes that could not be recorded, and so were not published.
pub const APPEND_FAILURES: &str = "convex_limit_status_append_failures_total";
/// The entries the audit store holds, read when the monitor starts, after each commit and
/// after each prune call that removes any. The store refuses every append at 1,000,000.
pub const AUDIT_ENTRIES: &str = "convex_audit_entries";
/// Records the monitor's retention has removed.
pub const AUDIT_PRUNED: &str = "convex_audit_pruned_total";

// One book's changes are one batch.
const _: () = assert!(convex_engine::ports::limits::MAX_BOOK_LIMITS <= MAX_AUDIT_BATCH);

/// Frames a slow subscriber may fall behind by: one evaluation of every watched book
/// changes at most 64 × 64 limits.
const FEED_CAPACITY: usize = MAX_LIMITED_BOOKS * convex_engine::ports::limits::MAX_BOOK_LIMITS;
/// How soon a failed read or append is tried again.
const RETRY: Duration = Duration::from_secs(1);
/// The longest wait for an expiry. The wait runs on the monotonic clock and an expiry is on
/// the wall clock, so a wall clock that steps forward is noticed within this.
const MAX_EXPIRY_WAIT: Duration = Duration::from_secs(60);
/// Kept priced terms beyond which those no current result reads are dropped: twice the
/// instruments one runtime values.
const MAX_TERMS: usize = 2 * convex_engine::MAX_RUNTIME_INSTRUMENTS;
/// How often retention prunes after the start.
const PRUNE_INTERVAL: Duration = Duration::from_secs(3600);
/// How soon a pass that failed, or could not read the clock, is tried again.
const PRUNE_RETRY: Duration = Duration::from_secs(60);
/// A day in milliseconds, the unit of a record's timestamp.
const DAY_MS: u64 = 86_400_000;

// A prune keeps each watched limit's latest record and the record its acknowledgement in
// force names, at most one of each a limit.
const _: () = assert!(
    2 * MAX_LIMITED_BOOKS * convex_engine::ports::limits::MAX_BOOK_LIMITS <= MAX_PRUNE_KEEP
);

// =============================================================================
// RECORDS AND FRAMES
// =============================================================================

/// One recorded status change: the audit record's details, and the `limit_status` frame.
#[derive(Debug, Clone, Serialize)]
pub struct LimitStatusChange {
    /// The audit entry that records it.
    pub record_id: u64,
    /// The book.
    pub book_id: BookId,
    /// The book version evaluated.
    pub book_version: Version,
    /// The limit set version evaluated.
    pub limit_set_version: Version,
    /// When it was evaluated.
    pub evaluated_at: UtcTimestamp,
    /// When this monitor started: a change from `unevaluated` restates a status at a start.
    pub monitor_started_at: UtcTimestamp,
    /// The last recorded status, or `unevaluated`.
    pub previous: &'static str,
    /// The limit's measure.
    pub measure: LimitMeasure,
    /// The lower bound it was judged against: an excess's while one applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lower: Option<ExactDecimal>,
    /// The upper bound it was judged against: an excess's while one applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upper: Option<ExactDecimal>,
    /// Its warning fraction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning_fraction: Option<ExactDecimal>,
    /// The evaluation: the limit, its new status, and the numbers behind it.
    #[serde(flatten)]
    pub evaluation: LimitEvaluation,
    /// The distinct snapshots whose results the evaluation read.
    pub snapshot_ids: Arc<[String]>,
    /// The acknowledgement in force for a `breach` or `unknown` limit when evaluated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledgement: Option<AcknowledgementInForce>,
}

/// The acknowledgement in force for a `breach` or `unknown` limit, as
/// status, records and `limit_status` frames carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AcknowledgementInForce {
    /// Who acknowledged it.
    pub acknowledged_by: String,
    /// When.
    pub acknowledged_at: UtcTimestamp,
    /// When it ends, unless withdrawn or ended by a set change first.
    pub expires_at: UtcTimestamp,
    /// The limit set version acknowledged.
    pub limit_set_version: Version,
    /// Why.
    pub note: String,
    /// The recorded change it answers, if it names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<u64>,
}

impl AcknowledgementInForce {
    /// The entry in force at `at` for `evaluation`'s limit under the current `limits`, if
    /// the limit is `breach` or `unknown`.
    pub(crate) fn of(
        entries: &[LimitAcknowledgement],
        evaluation: &LimitEvaluation,
        at: UtcTimestamp,
        limits: &[LimitDefinition],
    ) -> Option<Self> {
        if !matches!(
            evaluation.status,
            LimitStatus::Breach | LimitStatus::Unknown
        ) {
            return None;
        }
        entries
            .iter()
            .find(|entry| entry.limit.limit_id == evaluation.limit_id && entry.in_force(at, limits))
            .map(|entry| Self {
                acknowledged_by: entry.acknowledged_by.clone(),
                acknowledged_at: entry.acknowledged_at,
                expires_at: entry.expires_at,
                limit_set_version: entry.limit_set_version,
                note: entry.note.clone(),
                record_id: entry.record_id,
            })
    }
}

/// A recorded change on its way to subscribers.
pub(crate) struct LimitFrame {
    pub(crate) change: Arc<LimitStatusChange>,
    /// What a subscriber must hold `BookRead` on, as for the status route: each instrument
    /// the book holds or a limit names.
    pub(crate) instruments: Arc<[String]>,
}

/// The stream of recorded changes, handed to WebSocket sessions.
#[derive(Clone)]
pub(crate) struct LimitFeed(broadcast::Sender<Arc<LimitFrame>>);

impl LimitFeed {
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Arc<LimitFrame>> {
        self.0.subscribe()
    }
}

fn status_name(status: Option<LimitStatus>) -> &'static str {
    match status {
        None => "unevaluated",
        Some(LimitStatus::Within) => "within",
        Some(LimitStatus::Warning) => "warning",
        Some(LimitStatus::Unknown) => "unknown",
        Some(LimitStatus::Breach) => "breach",
    }
}

// =============================================================================
// THE MONITOR
// =============================================================================

/// The server's stop and the engine's shutdown, read between units of work.
#[derive(Clone)]
struct Stop {
    stop: watch::Receiver<bool>,
    shutdown: watch::Receiver<bool>,
}

impl Stop {
    fn requested(&self) -> bool {
        *self.stop.borrow() || *self.shutdown.borrow()
    }
}

/// The monitor a server composes over its audit store. [`LimitMonitor::run`] is its task.
pub(crate) struct LimitMonitor {
    audit: Arc<dyn AuditStore>,
    feed: broadcast::Sender<Arc<LimitFrame>>,
    /// The clear hold.
    hold: Duration,
    /// The retention period in milliseconds, if records are pruned.
    retention_ms: Option<i64>,
    /// How often retention prunes after the start: [`PRUNE_INTERVAL`], shorter only in
    /// this module's tests.
    prune_interval: Duration,
}

impl LimitMonitor {
    /// A monitor over `audit` with the clear hold `hold` and, if `retention_days` is set, a
    /// retention of that many days, held to 8 to 3,650 whatever built the configuration.
    pub(crate) fn new(
        audit: Arc<dyn AuditStore>,
        hold: Duration,
        retention_days: Option<u64>,
    ) -> Arc<Self> {
        use crate::config::{MAX_RECORD_RETENTION_DAYS, MIN_RECORD_RETENTION_DAYS};
        let retention_ms = retention_days.and_then(|days| {
            days.clamp(MIN_RECORD_RETENTION_DAYS, MAX_RECORD_RETENTION_DAYS)
                .checked_mul(DAY_MS)
                .and_then(|ms| i64::try_from(ms).ok())
        });
        Arc::new(Self {
            audit,
            feed: broadcast::channel(FEED_CAPACITY).0,
            hold,
            retention_ms,
            prune_interval: PRUNE_INTERVAL,
        })
    }

    pub(crate) fn feed(&self) -> LimitFeed {
        LimitFeed(self.feed.clone())
    }

    /// The store the monitor records in, which the records route reads.
    pub(crate) fn records(&self) -> Arc<dyn AuditStore> {
        self.audit.clone()
    }

    /// Watch and evaluate until `stop` turns true or the engine shuts down.
    pub(crate) async fn run(
        self: Arc<Self>,
        engine: Arc<PricingEngine>,
        mut stop: watch::Receiver<bool>,
    ) {
        // Subscribe before the first read, so nothing committed or published after it is
        // missed.
        let mut results = engine.subscribe_results();
        let mut commits = engine.subscribe_reference_commits();
        let mut shutdown = engine.subscribe_shutdown();
        let Ok(started_at) = crate::reference::now() else {
            tracing::error!("limit monitor: the clock is outside the supported range");
            return;
        };
        let halt = Stop {
            stop: stop.clone(),
            shutdown: shutdown.clone(),
        };
        let mut task = Task {
            engine,
            ledger: Ledger {
                audit: self.audit.clone(),
                feed: self.feed.clone(),
                next_id: None,
                started_at,
                halt: halt.clone(),
                full_reported: false,
                hold: self.hold,
            },
            halt,
            books: BTreeMap::new(),
            holders: HashMap::new(),
            retired: BTreeSet::new(),
            terms: PricedTerms::new(),
            reload: Some(Pending::All),
            dirty: Some(Pending::All),
            retry: BTreeSet::new(),
            full: BTreeSet::new(),
            stalled: false,
            pruning: self.retention_ms.map(|period_ms| Pruning {
                period_ms,
                interval: self.prune_interval,
                started: false,
                pass: None,
                due: Instant::now(),
            }),
        };
        let mut timer = tokio::time::interval(RETRY);
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            // A stop that arrived during the last unit of work, or before the first.
            if task.halt.requested() {
                break;
            }
            task.expire();
            task.release();
            task.schedule();
            task.work().await;
            task.prune().await;
            let expiry = task
                .next_expiry()
                .into_iter()
                .chain(task.next_release())
                .chain(task.next_prune())
                .min();
            tokio::select! {
                biased;
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() { break; }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { break; }
                }
                // Ahead of the streams: under steady ticks one is always ready, and a
                // retry behind them would never run.
                _ = timer.tick(), if task.waiting() => task.again(),
                // The next pass of the loop reads the clocks and finds what has expired and
                // which holds have ended.
                _ = tokio::time::sleep(expiry.unwrap_or_default()), if expiry.is_some() => {}
                notice = commits.recv() => {
                    let mut notice = notice;
                    loop {
                        match notice {
                            Ok(keys) => task.committed(&keys),
                            Err(broadcast::error::RecvError::Lagged(_)) => task.reload = Some(Pending::All),
                            Err(broadcast::error::RecvError::Closed) => return,
                        }
                        notice = match commits.try_recv() {
                            Ok(keys) => Ok(keys),
                            Err(broadcast::error::TryRecvError::Lagged(n)) => Err(broadcast::error::RecvError::Lagged(n)),
                            Err(_) => break,
                        };
                    }
                }
                received = results.recv() => {
                    let mut received = received;
                    loop {
                        match received {
                            Ok(result) => task.revalued(&result),
                            Err(broadcast::error::RecvError::Lagged(_)) => task.dirty = Some(Pending::All),
                            Err(broadcast::error::RecvError::Closed) => return,
                        }
                        received = match results.try_recv() {
                            Ok(result) => Ok(result),
                            Err(broadcast::error::TryRecvError::Lagged(n)) => Err(broadcast::error::RecvError::Lagged(n)),
                            Err(_) => break,
                        };
                    }
                }
                // Last, once the streams are read: a pass in progress makes its next call,
                // and books a call made due are evaluated.
                _ = std::future::ready(()), if task.ready() => {}
            }
        }
    }
}

/// Work outstanding: every book, or some.
enum Pending {
    All,
    Some(BTreeSet<BookId>),
}

impl Pending {
    fn add(slot: &mut Option<Pending>, id: BookId) {
        match slot {
            Some(Pending::All) => {}
            Some(Pending::Some(ids)) => {
                ids.insert(id);
            }
            None => *slot = Some(Pending::Some(BTreeSet::from([id]))),
        }
    }
}

/// What a watched book is evaluated on: its non-empty limit set, the book and its
/// acknowledgements, replaced whole when any of them commits.
struct Subject {
    set_version: Version,
    limits: Vec<LimitDefinition>,
    /// Entries in force or not; each change reads those in force when evaluated.
    acknowledgements: Vec<LimitAcknowledgement>,
    /// Absent while the book itself does not exist; nothing is evaluated then.
    book: Option<(Version, BookDefinition)>,
    instruments: Arc<[String]>,
}

/// A limit's last recorded status, and the record that holds it.
#[derive(Debug, Clone, Copy)]
struct Latest {
    status: LimitStatus,
    record_id: u64,
}

/// A watched book, and each limit's last recorded status since the start.
struct Watched {
    subject: Arc<Subject>,
    recorded: HashMap<LimitId, Latest>,
    /// The latest excess expiry this monitor has passed for the book.
    expired_through: Option<UtcTimestamp>,
    /// Its limits' changes to a less severe status, waiting out the hold.
    held: Holds,
}

/// When a limit's change to a less severe status began to wait out the hold, and whether
/// the monitor has woken for the hold's end.
#[derive(Debug, Clone, Copy)]
struct Hold {
    since: Instant,
    woken: bool,
}

/// One book's holds, by limit.
#[derive(Debug, Default)]
struct Holds(HashMap<LimitId, Hold>);

impl Holds {
    /// Whether to record `status` now, given the limit's last recorded status. A more severe
    /// status, or a first one, is recorded at once. A less severe one is recorded once the
    /// limit has been less severe than `previous` at every evaluation for `hold`; its hold
    /// is kept until the record is written, so a refused write is tried again at once. The
    /// recorded status itself, or one more severe, ends the hold.
    fn records(
        &mut self,
        limit_id: &LimitId,
        previous: Option<LimitStatus>,
        status: LimitStatus,
        now: Instant,
        hold: Duration,
    ) -> bool {
        match previous {
            Some(previous) if status == previous => {
                self.0.remove(limit_id);
                false
            }
            Some(previous) if status < previous && !hold.is_zero() => {
                let since = self
                    .0
                    .entry(limit_id.clone())
                    .or_insert(Hold {
                        since: now,
                        woken: false,
                    })
                    .since;
                now.saturating_duration_since(since) >= hold
            }
            _ => {
                self.0.remove(limit_id);
                true
            }
        }
    }

    /// Drop the holds of limits no longer in the set.
    fn retain(&mut self, limits: &[LimitDefinition]) {
        self.0
            .retain(|id, _| limits.iter().any(|limit| limit.limit_id == *id));
    }

    /// Mark each hold that has ended as woken, and say whether one has.
    fn wake(&mut self, now: Instant, hold: Duration) -> bool {
        let mut woke = false;
        for entry in self.0.values_mut() {
            if !entry.woken && now.saturating_duration_since(entry.since) >= hold {
                entry.woken = true;
                woke = true;
            }
        }
        woke
    }

    /// When the next hold not yet woken for ends.
    fn next_end(&self, hold: Duration) -> Option<Instant> {
        self.0
            .values()
            .filter(|entry| !entry.woken)
            .map(|entry| entry.since + hold)
            .min()
    }
}

/// The time to read `limits`' excesses at, given the wall clock's `at`: no earlier than the
/// latest expiry already passed, so an excess once expired stays expired when the clock
/// steps back. Only an expiry moves `through`, so a clock that jumps ahead and returns
/// pins nothing but the excesses it passed.
fn latch(
    limits: &[LimitDefinition],
    through: &mut Option<UtcTimestamp>,
    at: UtcTimestamp,
) -> UtcTimestamp {
    let at = through.map_or(at, |passed| passed.max(at));
    *through = limits
        .iter()
        .filter_map(|limit| limit.excess.as_ref().map(|excess| excess.expires_at))
        .filter(|expires_at| *expires_at <= at)
        .max()
        .max(*through);
    at
}

/// The earliest expiry `limits` hold that is later than the latest one passed.
fn next_expiry_of(
    limits: &[LimitDefinition],
    through: Option<UtcTimestamp>,
) -> Option<UtcTimestamp> {
    limits
        .iter()
        .filter_map(|limit| limit.excess.as_ref().map(|excess| excess.expires_at))
        .filter(|expires_at| through.is_none_or(|passed| *expires_at > passed))
        .min()
}

/// Retention: the period, the pass in progress, and when the next one starts.
struct Pruning {
    /// A record whose timestamp is earlier than now less this is older than the period.
    period_ms: i64,
    /// How long after a pass the next starts.
    interval: Duration,
    /// The start's pass has run, or failed; until then nothing is recorded.
    started: bool,
    pass: Option<Pass>,
    /// When the next pass starts, on the monotonic clock.
    due: Instant,
}

/// A prune pass: its cutoff, its cursor, and what it has removed.
struct Pass {
    before: i64,
    after: u64,
    removed: u64,
    began: Instant,
}

impl Pass {
    /// A pass over the records older than `period_ms` before the wall clock's now.
    fn begin(period_ms: i64) -> Option<Self> {
        let now = crate::reference::now().ok()?.datetime().timestamp_millis();
        Some(Self {
            before: now.checked_sub(period_ms)?,
            after: 0,
            removed: 0,
            began: Instant::now(),
        })
    }
}

/// What a prune keeps beside the store's last entry: each watched limit's latest record
/// since the start, and each record an acknowledgement in force at `at` names. A
/// limit not recorded since the start has no latest record, so its records go by age.
fn kept(books: &BTreeMap<BookId, Watched>, at: UtcTimestamp) -> BTreeSet<u64> {
    let mut keep = BTreeSet::new();
    for watched in books.values() {
        keep.extend(watched.recorded.values().map(|latest| latest.record_id));
        keep.extend(
            watched
                .subject
                .acknowledgements
                .iter()
                .filter(|entry| entry.in_force(at, &watched.subject.limits))
                .filter_map(|entry| entry.record_id),
        );
    }
    keep
}

struct Task {
    engine: Arc<PricingEngine>,
    ledger: Ledger,
    halt: Stop,
    books: BTreeMap<BookId, Watched>,
    /// Each instrument a watched book holds, and the books holding it.
    holders: HashMap<String, BTreeSet<BookId>>,
    /// Retired books that hold limits: not watched, but reread when a snapshot may
    /// revive one. At most one per limit set.
    retired: BTreeSet<BookId>,
    terms: PricedTerms,
    reload: Option<Pending>,
    dirty: Option<Pending>,
    /// Books with a change that could not be recorded.
    retry: BTreeSet<BookId>,
    /// Books whose changes the store refused for capacity; a prune that removes any
    /// record makes them due again.
    full: BTreeSet<BookId>,
    /// A read failed; it is tried again on the timer.
    stalled: bool,
    /// Retention, if a period is set.
    pruning: Option<Pruning>,
}

impl Task {
    fn waiting(&self) -> bool {
        self.stalled || !self.retry.is_empty()
    }

    /// Timer: try failed reads and failed records again.
    fn again(&mut self) {
        self.stalled = false;
        for id in std::mem::take(&mut self.retry) {
            Pending::add(&mut self.dirty, id);
        }
    }

    /// Evaluate again each book with an excess that has expired since the last look.
    fn expire(&mut self) {
        let Ok(at) = crate::reference::now() else {
            return;
        };
        for (id, watched) in &mut self.books {
            let before = watched.expired_through;
            latch(&watched.subject.limits, &mut watched.expired_through, at);
            if watched.expired_through != before {
                Pending::add(&mut self.dirty, id.clone());
            }
        }
    }

    /// How long until the next excess expires, if one is still to expire.
    fn next_expiry(&self) -> Option<Duration> {
        let next = self
            .books
            .values()
            .filter_map(|watched| next_expiry_of(&watched.subject.limits, watched.expired_through))
            .min()?;
        let at = crate::reference::now().ok()?;
        Some(
            (next.datetime() - at.datetime())
                .to_std()
                .unwrap_or_default()
                .min(MAX_EXPIRY_WAIT),
        )
    }

    /// Evaluate again each book with a hold that has ended since the last look. A hold is
    /// woken for once, so a write that keeps failing is tried on the timer, not in a loop.
    fn release(&mut self) {
        let now = Instant::now();
        for (id, watched) in &mut self.books {
            if watched.held.wake(now, self.ledger.hold) {
                Pending::add(&mut self.dirty, id.clone());
            }
        }
    }

    /// How long until the next hold ends, if one is waiting.
    fn next_release(&self) -> Option<Duration> {
        let next = self
            .books
            .values()
            .filter_map(|watched| watched.held.next_end(self.ledger.hold))
            .min()?;
        Some(next.saturating_duration_since(Instant::now()))
    }

    /// Start a pass when the next is due, once the start's has run.
    fn schedule(&mut self) {
        let Some(pruning) = &mut self.pruning else {
            return;
        };
        if !pruning.started || pruning.pass.is_some() || Instant::now() < pruning.due {
            return;
        }
        pruning.pass = Pass::begin(pruning.period_ms);
        if pruning.pass.is_none() {
            tracing::error!(
                "limit monitor: the clock is outside the supported range; records are not pruned"
            );
            pruning.due = Instant::now() + PRUNE_RETRY;
        }
    }

    /// Whether a pass after the start's is in progress.
    fn pruning(&self) -> bool {
        self.pruning
            .as_ref()
            .is_some_and(|pruning| pruning.started && pruning.pass.is_some())
    }

    /// Whether the loop has work at once: a pass in progress, or books a prune call made
    /// due after this turn's evaluation. A stalled read waits for the timer instead.
    fn ready(&self) -> bool {
        self.pruning() || (self.dirty.is_some() && !self.stalled)
    }

    /// How long until the next pass starts, if none is in progress.
    fn next_prune(&self) -> Option<Duration> {
        let pruning = self.pruning.as_ref()?;
        (pruning.started && pruning.pass.is_none())
            .then(|| pruning.due.saturating_duration_since(Instant::now()))
    }

    /// One call of the pass in progress after the start's.
    async fn prune(&mut self) {
        if self.pruning() && !self.halt.requested() {
            self.prune_call().await;
        }
    }

    /// The start's pass, whole, before any book is recorded: nothing is recorded yet, so
    /// it keeps only what acknowledgements in force name.
    async fn prune_at_start(&mut self) {
        let Some(pruning) = &mut self.pruning else {
            return;
        };
        if pruning.started {
            return;
        }
        if pruning.pass.is_none() {
            pruning.pass = Pass::begin(pruning.period_ms);
            if pruning.pass.is_none() {
                tracing::error!("limit monitor: the clock is outside the supported range; records are not pruned");
                pruning.started = true;
                pruning.due = Instant::now() + PRUNE_RETRY;
                return;
            }
        }
        while self
            .pruning
            .as_ref()
            .is_some_and(|pruning| pruning.pass.is_some())
        {
            if self.halt.requested() {
                return;
            }
            self.prune_call().await;
        }
        if let Some(pruning) = &mut self.pruning {
            pruning.started = true;
        }
    }

    /// One call of the pass in progress: at most 4,096 removals, one transaction. A call
    /// that removes any record makes each book refused for capacity due again. The pass
    /// ends when the walk reaches a newer record, or on a failure, after which the next is
    /// tried within a minute; a store that cannot prune ends retention.
    async fn prune_call(&mut self) {
        let Some(pruning) = &mut self.pruning else {
            return;
        };
        let Some(pass) = &mut pruning.pass else {
            return;
        };
        let Ok(at) = crate::reference::now() else {
            pruning.pass = None;
            pruning.due = Instant::now() + PRUNE_RETRY;
            return;
        };
        let keep = kept(&self.books, at);
        match self.ledger.prune(pass.before, pass.after, &keep).await {
            Ok(pruned) => {
                pass.after = pruned.next_after;
                pass.removed = pass.removed.saturating_add(pruned.removed as u64);
                if pruned.removed > 0 {
                    for id in std::mem::take(&mut self.full) {
                        Pending::add(&mut self.dirty, id);
                    }
                }
                if !pruned.more {
                    if pass.removed > 0 {
                        tracing::info!(
                            removed = pass.removed,
                            seconds = pass.began.elapsed().as_secs_f64(),
                            "limit monitor: pruned records older than the retention period"
                        );
                    }
                    pruning.pass = None;
                    pruning.due = Instant::now() + pruning.interval;
                }
            }
            Err(TraitError::Unsupported(_)) => {
                tracing::error!("limit monitor: the audit store cannot remove records, so limits.record_retention_days has no effect");
                self.pruning = None;
            }
            Err(error) => {
                tracing::warn!(%error, "limit monitor: a prune failed; it is tried again within a minute");
                pruning.pass = None;
                pruning.started = true;
                pruning.due = Instant::now() + PRUNE_RETRY;
            }
        }
    }

    fn committed(&mut self, keys: &[MasterKey]) {
        for key in keys {
            match key {
                MasterKey::LimitSet(id) => Pending::add(&mut self.reload, id.clone()),
                // A snapshot over a retired book that holds limits revives it.
                MasterKey::Book(id) if self.retired.contains(id) => {
                    Pending::add(&mut self.reload, id.clone())
                }
                MasterKey::Book(id) | MasterKey::LimitAcknowledgements(id)
                    if self.books.contains_key(id) =>
                {
                    Pending::add(&mut self.reload, id.clone())
                }
                _ => {}
            }
        }
    }

    fn revalued(&mut self, result: &PublishedValuation) {
        if let ValuationSubject::InstrumentVersion { instrument_id, .. } =
            &result.result.provenance().subject
        {
            if let Some(books) = self.holders.get(instrument_id.as_str()) {
                for id in books {
                    Pending::add(&mut self.dirty, id.clone());
                }
            }
        }
    }

    async fn work(&mut self) {
        if self.ledger.next_id.is_none() {
            match self.ledger.audit.last_id().await {
                Ok(last) => {
                    self.ledger.next_id = Some(last.map_or(1, |id| id.saturating_add(1)));
                    self.ledger.count_entries().await;
                }
                Err(error) => {
                    tracing::warn!(%error, "limit monitor: the audit store's last entry is unreadable");
                    self.stalled = true;
                    return;
                }
            }
        }
        if self.halt.requested() {
            return;
        }
        if let Some(reload) = self.reload.take() {
            if let Err(error) = self.reread(reload).await {
                tracing::warn!(%error, "limit monitor: limit sets or books are unreadable");
                self.reload = Some(Pending::All);
                self.stalled = true;
                return;
            }
        }
        if self.halt.requested() {
            return;
        }
        self.prune_at_start().await;
        if self.halt.requested() {
            return;
        }
        if let Some(dirty) = self.dirty.take() {
            self.evaluate(dirty).await;
        }
    }

    /// Reread the named sets and books, or every one, and evaluate each reread book.
    async fn reread(&mut self, which: Pending) -> Result<(), ReferenceError> {
        let repo = self.engine.reference_repository().clone();
        let ids: Vec<BookId> = match which {
            Pending::All => {
                let mut ids: BTreeSet<BookId> = self.books.keys().cloned().collect();
                let mut after: Option<String> = None;
                loop {
                    let page = repo
                        .list_masters(MasterKind::LimitSet, after.as_deref(), MAX_REFERENCE_PAGE)
                        .await?;
                    for record in &page {
                        ids.insert(set_of(record)?.book_id.clone());
                    }
                    if page.len() < MAX_REFERENCE_PAGE {
                        break;
                    }
                    after = page
                        .last()
                        .map(|r| set_of(r).map(|s| s.book_id.as_str().to_owned()))
                        .transpose()?;
                }
                ids.into_iter().collect()
            }
            Pending::Some(ids) => ids.into_iter().collect(),
        };
        for id in ids {
            if self.halt.requested() {
                return Ok(());
            }
            let set = repo.get_master(&MasterKey::LimitSet(id.clone())).await?;
            let set = match &set {
                Some(record) => Some((record.version, set_of(record)?)),
                None => None,
            };
            self.retired.remove(&id);
            let Some((set_version, set)) = set.filter(|(_, set)| !set.limits.is_empty()) else {
                self.unwatch(&id);
                continue;
            };
            let book = match repo.get_master(&MasterKey::Book(id.clone())).await? {
                Some(record) => {
                    let book = book_of(&record)?;
                    if book.is_retired() {
                        self.unwatch(&id);
                        self.retired.insert(id);
                        continue;
                    }
                    Some((record.version, book.clone()))
                }
                None => None,
            };
            if !self.books.contains_key(&id) && self.books.len() >= MAX_LIMITED_BOOKS {
                tracing::error!(book = %id, "limit monitor: more than {MAX_LIMITED_BOOKS} books hold a limit set; this one is not watched");
                continue;
            }
            let acknowledgements = match repo
                .get_master(&MasterKey::LimitAcknowledgements(id.clone()))
                .await?
            {
                Some(record) => crate::limit_acknowledgements::acknowledgements_of_record(&record)?
                    .acknowledgements
                    .clone(),
                None => Vec::new(),
            };
            let instruments: BTreeSet<String> = book
                .iter()
                .flat_map(|(_, book)| book.positions.iter())
                .map(|p| p.instrument_id.as_str().to_owned())
                .chain(named_instruments(&set.limits).map(|i| i.as_str().to_owned()))
                .collect();
            let (recorded, expired_through, held) = self
                .books
                .remove(&id)
                .map(|watched| (watched.recorded, watched.expired_through, watched.held))
                .unwrap_or_default();
            self.books.insert(
                id.clone(),
                Watched {
                    subject: Arc::new(Subject {
                        set_version,
                        limits: set.limits.clone(),
                        acknowledgements,
                        book,
                        instruments: instruments.into_iter().collect(),
                    }),
                    recorded,
                    expired_through,
                    held,
                },
            );
            Pending::add(&mut self.dirty, id);
        }
        self.index();
        Ok(())
    }

    fn unwatch(&mut self, id: &BookId) {
        if self.books.remove(id).is_some() {
            self.retry.remove(id);
            self.full.remove(id);
            gauges(id, &HashMap::new());
        }
    }

    fn index(&mut self) {
        self.holders.clear();
        for (id, watched) in &self.books {
            if let Some((_, book)) = &watched.subject.book {
                for position in &book.positions {
                    self.holders
                        .entry(position.instrument_id.as_str().to_owned())
                        .or_default()
                        .insert(id.clone());
                }
            }
        }
    }

    /// Evaluate the named books, or every one, on the latest results, then record and
    /// publish each change. Priced terms a book needs and the monitor lacks are read
    /// first.
    async fn evaluate(&mut self, dirty: Pending) {
        let subjects: Vec<(BookId, Arc<Subject>)> = match dirty {
            Pending::All => self
                .books
                .iter()
                .map(|(id, watched)| (id.clone(), watched.subject.clone()))
                .collect(),
            Pending::Some(ids) => ids
                .into_iter()
                .filter_map(|id| {
                    let subject = self.books.get(&id)?.subject.clone();
                    Some((id, subject))
                })
                .collect(),
        };
        if subjects.is_empty() {
            return;
        }
        let started = Instant::now();
        // The results of the books evaluated, from one table. Every book's are those of
        // the watched instruments, each listed once.
        let every = subjects.len() == self.books.len();
        let accepted: Accepted = if every {
            accepted_results(&self.engine, self.holders.keys().map(String::as_str))
        } else {
            accepted_results(
                &self.engine,
                subjects
                    .iter()
                    .filter_map(|(_, subject)| subject.book.as_ref())
                    .flat_map(|(_, book)| book.positions.iter().map(|p| p.instrument_id.as_str())),
            )
        };
        let settlement = self.engine.runtime_settlement_date();
        let Ok(evaluated_at) = crate::reference::now() else {
            tracing::error!("limit monitor: the clock is outside the supported range");
            return;
        };
        let repo = self.engine.reference_repository().clone();
        for (id, subject) in &subjects {
            if self.halt.requested() {
                return;
            }
            let Some((book_version, book)) = &subject.book else {
                continue;
            };
            read_priced_terms(repo.as_ref(), book, &accepted, &mut self.terms).await;
            let Some(watched) = self.books.get_mut(id) else {
                continue;
            };
            let excess_at = latch(&subject.limits, &mut watched.expired_through, evaluated_at);
            let evaluation = match evaluate_book(
                book,
                &subject.limits,
                &accepted,
                &self.terms,
                settlement,
                excess_at,
            ) {
                Ok(evaluation) => evaluation,
                Err(error) => {
                    tracing::error!(book = %id, %error, "limit monitor: evaluation refused a stored book and limit set");
                    continue;
                }
            };
            let context = Context {
                book_id: id,
                book_version: *book_version,
                set_version: subject.set_version,
                evaluated_at,
                instruments: &subject.instruments,
                now: started,
            };
            match self
                .ledger
                .record(
                    context,
                    &subject.limits,
                    &subject.acknowledgements,
                    evaluation,
                    &mut watched.recorded,
                    &mut watched.held,
                )
                .await
            {
                Recorded::All => {
                    self.retry.remove(id);
                    self.full.remove(id);
                }
                // A full store stays full until a prune removes a record, or the book's
                // next evaluation finds room: not a timer.
                Recorded::Full => {
                    self.retry.remove(id);
                    self.full.insert(id.clone());
                }
                Recorded::Stopped => {
                    self.retry.remove(id);
                }
                Recorded::Retry => {
                    self.retry.insert(id.clone());
                }
            }
            gauges(id, &watched.recorded);
        }
        if self.terms.len() > MAX_TERMS {
            // Kept for every watched instrument's priced version, not only this pass's
            // books', so no other book reads its terms again.
            let watched = if every {
                accepted
            } else {
                accepted_results(&self.engine, self.holders.keys().map(String::as_str))
            };
            self.terms.retain(|(instrument, version), _| {
                watched
                    .get(instrument.as_str())
                    .is_some_and(|versions| versions.contains_key(&version.get()))
            });
        }
        metrics::histogram!(MONITOR_SECONDS).record(started.elapsed().as_secs_f64());
    }
}

/// Publish a watched book's counts of limits in breach and unknown, as last recorded.
fn gauges(id: &BookId, recorded: &HashMap<LimitId, Latest>) {
    let count = |status| {
        recorded
            .values()
            .filter(|latest| latest.status == status)
            .count() as f64
    };
    metrics::gauge!(BREACHES, "book_id" => id.as_str().to_owned()).set(count(LimitStatus::Breach));
    metrics::gauge!(UNKNOWNS, "book_id" => id.as_str().to_owned()).set(count(LimitStatus::Unknown));
}

/// What one evaluation of a book records with each change.
struct Context<'a> {
    book_id: &'a BookId,
    book_version: Version,
    set_version: Version,
    evaluated_at: UtcTimestamp,
    instruments: &'a Arc<[String]>,
    /// The monotonic time of the evaluation, which holds are measured on.
    now: Instant,
}

/// How one book's changes were recorded.
enum Recorded {
    /// Every change.
    All,
    /// Not every change; a second later may succeed.
    Retry,
    /// The store refused the book's changes for capacity; none was recorded.
    Full,
    /// A stop arrived; the rest were not tried.
    Stopped,
}

/// The audit store, the feed, and the next entry ID.
struct Ledger {
    audit: Arc<dyn AuditStore>,
    feed: broadcast::Sender<Arc<LimitFrame>>,
    /// Read from the store's last entry at start; every attempt takes a new one, so a
    /// write that failed after it committed never makes a retry conflict.
    next_id: Option<u64>,
    started_at: UtcTimestamp,
    halt: Stop,
    /// A capacity refusal has been logged since the last append that succeeded.
    full_reported: bool,
    /// The clear hold.
    hold: Duration,
}

impl Ledger {
    /// One prune call: `limit_status` records older than `before` after `after`, except
    /// `keep`, then the counter and the entries gauge.
    async fn prune(
        &self,
        before: i64,
        after: u64,
        keep: &BTreeSet<u64>,
    ) -> Result<Pruned, TraitError> {
        let pruned = self
            .audit
            .prune(&AuditPrune {
                event_type: LIMIT_STATUS_EVENT,
                before,
                after,
                keep,
                max: MAX_PRUNE_BATCH,
            })
            .await?;
        if pruned.removed > 0 {
            metrics::counter!(AUDIT_PRUNED).increment(pruned.removed as u64);
            self.count_entries().await;
        }
        Ok(pruned)
    }

    /// Publish how many entries the store holds; an unreadable count leaves the gauge.
    async fn count_entries(&self) {
        match self.audit.entry_count().await {
            Ok(count) => metrics::gauge!(AUDIT_ENTRIES).set(count as f64),
            Err(error) => {
                tracing::debug!(%error, "limit monitor: the audit store's entry count is unreadable")
            }
        }
    }

    /// Record, then publish, each limit whose status differs from its last recorded one,
    /// under the clear hold: the book's records in one batch, then their frames in limit
    /// order.
    async fn record(
        &mut self,
        context: Context<'_>,
        limits: &[LimitDefinition],
        acknowledgements: &[LimitAcknowledgement],
        evaluation: crate::limits::BookEvaluation,
        recorded: &mut HashMap<LimitId, Latest>,
        held: &mut Holds,
    ) -> Recorded {
        recorded.retain(|id, _| limits.iter().any(|limit| limit.limit_id == *id));
        held.retain(limits);
        let snapshot_ids: Arc<[String]> = evaluation.snapshot_ids.into();
        let mut complete = true;
        let mut entries = Vec::new();
        let mut changes = Vec::new();
        for (limit, evaluation) in limits.iter().zip(evaluation.evaluations) {
            let previous = recorded.get(&limit.limit_id).map(|latest| latest.status);
            if !held.records(
                &limit.limit_id,
                previous,
                evaluation.status,
                context.now,
                self.hold,
            ) {
                continue;
            }
            let Some(record_id) = self.next_id else {
                complete = false;
                break;
            };
            self.next_id = record_id.checked_add(1);
            let status = evaluation.status;
            let (lower, upper) = limit.judged_bounds(&evaluation);
            let acknowledgement = AcknowledgementInForce::of(
                acknowledgements,
                &evaluation,
                context.evaluated_at,
                limits,
            );
            let change = LimitStatusChange {
                record_id,
                book_id: context.book_id.clone(),
                book_version: context.book_version,
                limit_set_version: context.set_version,
                evaluated_at: context.evaluated_at,
                monitor_started_at: self.started_at,
                previous: status_name(previous),
                measure: limit.measure.clone(),
                lower,
                upper,
                warning_fraction: limit.warning_fraction,
                evaluation,
                snapshot_ids: snapshot_ids.clone(),
                acknowledgement,
            };
            let details = match serde_json::to_string(&change) {
                Ok(details) => details,
                Err(error) => {
                    tracing::error!(%error, "limit monitor: a status change cannot be serialized");
                    complete = false;
                    continue;
                }
            };
            entries.push(AuditEntry {
                id: record_id,
                timestamp: context.evaluated_at.datetime().timestamp_millis(),
                event_type: LIMIT_STATUS_EVENT.into(),
                entity_type: LIMIT_STATUS_ENTITY.into(),
                entity_id: context.book_id.as_str().to_owned(),
                user: None,
                action: status_name(Some(status)).into(),
                details: Some(details),
            });
            changes.push((limit.limit_id.clone(), status, change));
        }
        if entries.is_empty() {
            return if complete {
                Recorded::All
            } else {
                Recorded::Retry
            };
        }
        if self.halt.requested() {
            return Recorded::Stopped;
        }
        // Each change counts once, recorded or not.
        let count = entries.len() as u64;
        match self.audit.append_all(&entries).await {
            Ok(()) => {
                self.full_reported = false;
                metrics::counter!(CHANGES).increment(count);
                for (limit_id, status, change) in changes {
                    held.0.remove(&limit_id);
                    recorded.insert(
                        limit_id,
                        Latest {
                            status,
                            record_id: change.record_id,
                        },
                    );
                    // No subscriber is not an error.
                    let _ = self.feed.send(Arc::new(LimitFrame {
                        change: Arc::new(change),
                        instruments: context.instruments.clone(),
                    }));
                }
                self.count_entries().await;
            }
            Err(TraitError::CapacityExceeded) => {
                metrics::counter!(APPEND_FAILURES).increment(count);
                if !self.full_reported {
                    self.full_reported = true;
                    tracing::error!(book = %context.book_id, "limit monitor: the audit store refuses status changes for capacity (it holds at most 1,000,000 entries; with limits.record_retention_days set, records older than the period are pruned hourly); changes are neither recorded nor published until it accepts one");
                }
                return Recorded::Full;
            }
            Err(error) => {
                tracing::warn!(book = %context.book_id, changes = count, %error, "limit monitor: a book's status changes could not be recorded");
                metrics::counter!(APPEND_FAILURES).increment(count);
                complete = false;
            }
        }
        if complete {
            Recorded::All
        } else {
            Recorded::Retry
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn stamp(text: &str) -> UtcTimestamp {
        UtcTimestamp::parse(text).unwrap()
    }

    /// One limit per expiry.
    fn limits(expiries: &[&str]) -> Vec<LimitDefinition> {
        expiries
            .iter()
            .enumerate()
            .map(|(index, expires_at)| {
                serde_json::from_value(serde_json::json!({
                    "limit_id": format!("limit-{index}"),
                    "measure": {"kind": "market_value"},
                    "upper": "100",
                    "excess": {"upper": "200", "expires_at": expires_at, "reason": "mandate"},
                }))
                .unwrap()
            })
            .collect()
    }

    /// At, just before and just after an expiry, and after a clock that steps back
    /// (WSL2's does, by over 2 s), which must not apply the excess again.
    #[test]
    fn an_excess_once_expired_stays_expired_when_the_clock_steps_back() {
        const EXPIRY: &str = "2026-11-09T12:00:00Z";
        let limits = limits(&[EXPIRY]);
        let mut through = None;

        let before = stamp("2026-11-09T11:59:59.999999Z");
        assert_eq!(latch(&limits, &mut through, before), before);
        assert!(through.is_none());
        assert!(limits[0].excess_at(before).is_some());
        assert_eq!(next_expiry_of(&limits, through), Some(stamp(EXPIRY)));

        let at = latch(&limits, &mut through, stamp(EXPIRY));
        assert_eq!(through, Some(stamp(EXPIRY)));
        assert!(limits[0].excess_at(at).is_none());
        assert_eq!(next_expiry_of(&limits, through), None);

        let after = stamp("2026-11-09T12:00:00.000001Z");
        assert_eq!(latch(&limits, &mut through, after), after);
        assert_eq!(through, Some(stamp(EXPIRY)));

        let stepped = latch(&limits, &mut through, stamp("2026-11-09T11:59:57.8Z"));
        assert_eq!(stepped, stamp(EXPIRY));
        assert!(limits[0].excess_at(stepped).is_none());
        assert_eq!(next_expiry_of(&limits, through), None);
    }

    // =========================================================================
    // RETENTION
    // =========================================================================

    use convex_engine::ports::storage::{AuditFilter, Page, Pagination, StorageAdapter};

    const DAYS_8: i64 = 8 * 86_400_000;

    fn market_value(id: &str) -> LimitDefinition {
        serde_json::from_value(serde_json::json!({
            "limit_id": id,
            "measure": {"kind": "market_value"},
            "upper": "100",
        }))
        .unwrap()
    }

    fn record(id: u64, timestamp: i64, book: &str) -> AuditEntry {
        AuditEntry {
            id,
            timestamp,
            event_type: LIMIT_STATUS_EVENT.into(),
            entity_type: LIMIT_STATUS_ENTITY.into(),
            entity_id: book.into(),
            user: None,
            action: "breach".into(),
            details: None,
        }
    }

    fn acknowledgement(
        limit: &LimitDefinition,
        expires_at: &str,
        record_id: u64,
    ) -> LimitAcknowledgement {
        LimitAcknowledgement {
            limit: limit.clone(),
            limit_set_version: Version::new(1).unwrap(),
            acknowledged_by: "risk".into(),
            acknowledged_at: stamp("2026-01-01T00:00:00Z"),
            expires_at: stamp(expires_at),
            note: "seen".into(),
            record_id: Some(record_id),
        }
    }

    /// A task over `audit` with nothing watched, whose retention has the 8-day period;
    /// `started` says whether the start's pass has run.
    fn pruning_task(storage: StorageAdapter, audit: Arc<dyn AuditStore>, started: bool) -> Task {
        let engine = Arc::new(
            convex_engine::PricingEngineBuilder::new()
                .with_storage(Arc::new(storage))
                .build()
                .unwrap(),
        );
        let halt = Stop {
            stop: watch::channel(false).1,
            shutdown: watch::channel(false).1,
        };
        Task {
            engine,
            ledger: Ledger {
                audit,
                feed: broadcast::channel(16).0,
                next_id: None,
                started_at: stamp("2026-01-01T00:00:00Z"),
                halt: halt.clone(),
                full_reported: false,
                hold: Duration::ZERO,
            },
            halt,
            books: BTreeMap::new(),
            holders: HashMap::new(),
            retired: BTreeSet::new(),
            terms: PricedTerms::new(),
            reload: None,
            dirty: None,
            retry: BTreeSet::new(),
            full: BTreeSet::new(),
            stalled: false,
            pruning: Some(Pruning {
                period_ms: DAYS_8,
                interval: PRUNE_INTERVAL,
                started,
                pass: None,
                due: Instant::now(),
            }),
        }
    }

    /// `BOOK-A` watched with `limit-0` and `limit-1`, `recorded` their latest records, and
    /// two acknowledgements: `limit-1`'s in force naming 2, `limit-0`'s expired naming 7.
    fn watch_book_a(task: &mut Task, recorded: &[(&str, u64)]) {
        let limits = vec![market_value("limit-0"), market_value("limit-1")];
        let acknowledgements = vec![
            acknowledgement(&limits[1], "2999-01-01T00:00:00Z", 2),
            acknowledgement(&limits[0], "2026-01-02T00:00:00Z", 7),
        ];
        task.books.insert(
            BookId::new("BOOK-A").unwrap(),
            Watched {
                subject: Arc::new(Subject {
                    set_version: Version::new(1).unwrap(),
                    limits,
                    acknowledgements,
                    book: None,
                    instruments: Arc::from(Vec::<String>::new()),
                }),
                recorded: recorded
                    .iter()
                    .map(|(limit, record_id)| {
                        (
                            LimitId::new(*limit).unwrap(),
                            Latest {
                                status: LimitStatus::Breach,
                                record_id: *record_id,
                            },
                        )
                    })
                    .collect(),
                expired_through: None,
                held: Holds::default(),
            },
        );
    }

    /// Records 1 to 7 a year old (`BOOK-B`'s 4 is of a book not watched, 6 of a limit no
    /// longer in `BOOK-A`'s set), 4,200 more of `BOOK-B` a year old, so a pass takes two
    /// calls, and one of now, the last.
    async fn fixture() -> (tempfile::TempDir, StorageAdapter, u64) {
        let directory = tempfile::tempdir().unwrap();
        let storage =
            convex_ext_redb::create_redb_storage(directory.path().join("audit.redb")).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let old = now - 365 * 86_400_000;
        let books = [
            "BOOK-A", "BOOK-A", "BOOK-A", "BOOK-B", "BOOK-A", "BOOK-A", "BOOK-A",
        ];
        let mut entries: Vec<AuditEntry> = books
            .iter()
            .enumerate()
            .map(|(index, book)| record(index as u64 + 1, old, book))
            .collect();
        entries.extend((8..8 + 4200).map(|id| record(id, old, "BOOK-B")));
        let last = 8 + 4200;
        entries.push(record(last, now, "BOOK-A"));
        for batch in entries.chunks(MAX_AUDIT_BATCH) {
            storage.audit.append_all(batch).await.unwrap();
        }
        (directory, storage, last)
    }

    async fn ids(audit: &Arc<dyn AuditStore>) -> Vec<u64> {
        let mut ids = Vec::new();
        loop {
            let page = audit
                .query(&AuditFilter::default(), &Pagination::new(ids.len(), 1000))
                .await
                .unwrap();
            let done = page.items.len() < 1000;
            ids.extend(page.items.iter().map(|entry| entry.id));
            if done {
                return ids;
            }
        }
    }

    /// An hourly pass keeps each watched limit's latest record, each record an
    /// acknowledgement in force names, and the last; the rest older than the period go,
    /// in calls of at most 4,096, between which the monitor evaluates. A book refused for
    /// capacity is due again, and the next pass is an hour away.
    #[tokio::test]
    async fn an_hourly_pass_keeps_each_latest_record_and_each_acknowledged_one() {
        let (_directory, storage, last) = fixture().await;
        let audit = storage.audit.clone();
        let mut task = pruning_task(storage, audit.clone(), true);
        watch_book_a(&mut task, &[("limit-0", 3), ("limit-1", 5)]);
        let book_a = BookId::new("BOOK-A").unwrap();
        task.full.insert(book_a.clone());
        assert_eq!(task.next_prune(), Some(Duration::ZERO));
        task.schedule();
        let mut calls = 0;
        while task.pruning() {
            task.prune().await;
            calls += 1;
        }
        assert_eq!(calls, 2, "4,206 records to remove, 4,096 a call");
        assert_eq!(ids(&audit).await, [2, 3, 5, last]);
        assert!(matches!(&task.dirty, Some(Pending::Some(ids)) if ids.contains(&book_a)));
        assert!(task.full.is_empty());
        assert!(task.next_prune().unwrap() > Duration::from_secs(3590));
    }

    /// The start's pass runs whole before anything is recorded, so nothing is recorded
    /// since the start: it keeps only what acknowledgements in force name, and the last.
    #[tokio::test]
    async fn the_start_pass_keeps_only_acknowledged_records() {
        let (_directory, storage, last) = fixture().await;
        let audit = storage.audit.clone();
        let mut task = pruning_task(storage, audit.clone(), false);
        watch_book_a(&mut task, &[]);
        assert_eq!(task.next_prune(), None, "no hourly pass before the start's");
        task.prune_at_start().await;
        assert_eq!(ids(&audit).await, [2, last]);
        assert!(task.pruning.as_ref().unwrap().started);
        assert!(task.next_prune().unwrap() > Duration::from_secs(3590));
        // It runs once.
        task.prune_at_start().await;
        assert!(task.pruning.as_ref().unwrap().pass.is_none());
    }

    /// The monitor's own loop: the start's pass, then an hourly pass that starts on its
    /// deadline with no tick or commit to wake it, and runs call after call until done.
    #[tokio::test]
    async fn the_monitor_starts_each_pass_when_due_and_runs_it_to_the_end_while_idle() {
        let directory = tempfile::tempdir().unwrap();
        let storage =
            convex_ext_redb::create_redb_storage(directory.path().join("audit.redb")).unwrap();
        let audit = storage.audit.clone();
        let old = chrono::Utc::now().timestamp_millis() - 365 * 86_400_000;
        let append = |ids: std::ops::RangeInclusive<u64>| {
            let audit = audit.clone();
            async move {
                let entries: Vec<AuditEntry> = ids.map(|id| record(id, old, "BOOK-B")).collect();
                for batch in entries.chunks(MAX_AUDIT_BATCH) {
                    audit.append_all(batch).await.unwrap();
                }
            }
        };
        let until_count = |want: u64| {
            let audit = audit.clone();
            async move {
                let waited = tokio::time::timeout(Duration::from_secs(10), async {
                    while audit.entry_count().await.unwrap() != want {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await;
                assert!(
                    waited.is_ok(),
                    "never {want} entries: {}",
                    audit.entry_count().await.unwrap()
                );
            }
        };
        append(1..=10).await;
        let engine = Arc::new(
            convex_engine::PricingEngineBuilder::new()
                .with_storage(Arc::new(storage))
                .build()
                .unwrap(),
        );
        let monitor = Arc::new(LimitMonitor {
            audit: audit.clone(),
            feed: broadcast::channel(16).0,
            hold: Duration::ZERO,
            retention_ms: Some(DAYS_8),
            prune_interval: Duration::from_secs(2),
        });
        let (stop, stopped) = watch::channel(false);
        let running = tokio::spawn(monitor.run(engine, stopped));
        // The start's pass keeps only the last.
        until_count(1).await;
        // Two calls' worth, older than the period, for the next pass: nothing else wakes
        // the monitor.
        append(11..=5010).await;
        assert_eq!(audit.entry_count().await.unwrap(), 5001);
        until_count(1).await;
        assert_eq!(audit.last_id().await.unwrap(), Some(5010));
        stop.send(true).unwrap();
        running.await.unwrap();
    }

    /// An audit store over redb that refuses a write past `cap` entries for capacity, as
    /// redb does at 1,000,000.
    struct Capped {
        inner: Arc<dyn AuditStore>,
        cap: u64,
    }

    #[async_trait::async_trait]
    impl AuditStore for Capped {
        async fn append(&self, entry: &AuditEntry) -> Result<(), TraitError> {
            self.append_all(std::slice::from_ref(entry)).await
        }
        async fn append_all(&self, entries: &[AuditEntry]) -> Result<(), TraitError> {
            if self.inner.entry_count().await? + entries.len() as u64 > self.cap {
                return Err(TraitError::CapacityExceeded);
            }
            self.inner.append_all(entries).await
        }
        async fn entry_count(&self) -> Result<u64, TraitError> {
            self.inner.entry_count().await
        }
        async fn query(
            &self,
            filter: &AuditFilter,
            pagination: &Pagination,
        ) -> Result<Page<AuditEntry>, TraitError> {
            self.inner.query(filter, pagination).await
        }
        async fn last_id(&self) -> Result<Option<u64>, TraitError> {
            self.inner.last_id().await
        }
        async fn prune(&self, prune: &AuditPrune<'_>) -> Result<Pruned, TraitError> {
            self.inner.prune(prune).await
        }
    }

    /// A book the store refused for capacity is recorded as soon as a pass's last call
    /// frees room, with no tick or commit to wake the monitor, not at the next pass.
    #[tokio::test]
    async fn a_book_refused_for_capacity_is_recorded_once_the_last_call_frees_room() {
        const INTERVAL: Duration = Duration::from_secs(3);
        let directory = tempfile::tempdir().unwrap();
        let storage =
            convex_ext_redb::create_redb_storage(directory.path().join("audit.redb")).unwrap();
        let inner = storage.audit.clone();
        // Full, and none older than the 1 s period at the start's pass.
        let now = chrono::Utc::now().timestamp_millis();
        let entries: Vec<AuditEntry> = (1..=10).map(|id| record(id, now, "BOOK-B")).collect();
        inner.append_all(&entries).await.unwrap();
        let engine = Arc::new(
            convex_engine::PricingEngineBuilder::new()
                .with_storage(Arc::new(storage))
                .build()
                .unwrap(),
        );
        let config = crate::ServerConfig {
            profile: crate::ServerProfile::Demo,
            host: "127.0.0.1".into(),
            access: crate::access::identity::AccessSettings {
                mode: crate::access::identity::AccessMode::LocalDevelopment,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut server = crate::Server::with_access_control(
            config,
            engine,
            crate::access::AccessControl::local_development(),
        );
        server.limits = Some(Arc::new(LimitMonitor {
            audit: Arc::new(Capped {
                inner: inner.clone(),
                cap: 10,
            }),
            feed: broadcast::channel(16).0,
            hold: Duration::ZERO,
            retention_ms: Some(1000),
            prune_interval: INTERVAL,
        }));
        let started = Instant::now();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let serving = tokio::spawn(async move {
            server
                .serve_until(listener, async {
                    let _ = stopped.await;
                })
                .await
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for (path, key, body) in [
            (
                "/api/v1/books/BOOK-A",
                "book-a",
                serde_json::json!({"currency": "USD", "positions": []}),
            ),
            (
                "/api/v1/books/BOOK-A/limits",
                "limits-a",
                serde_json::json!({"limits": [{"limit_id": "mv", "measure": {"kind": "market_value"}, "upper": "100"}]}),
            ),
        ] {
            let response = client
                .put(format!("{base}{path}"))
                .header("idempotency-key", key)
                .json(&body)
                .send()
                .await
                .unwrap();
            assert!(
                response.status().is_success(),
                "{path}: {}",
                response.status()
            );
        }
        // The book's first record is refused: the store is full.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(ids(&inner).await, (1..=10).collect::<Vec<_>>());
        // The pass due after the interval frees room in one call, its last: 1 to 9 go.
        let waited = tokio::time::timeout(Duration::from_secs(10), async {
            while ids(&inner).await.first() != Some(&10) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        assert!(waited.is_ok(), "never pruned: {:?}", ids(&inner).await);
        let pruned = started.elapsed();
        // The book's record follows; the refused attempt used up 11. A later pass may
        // remove 10, so wait for 12 alone.
        let waited = tokio::time::timeout(Duration::from_secs(10), async {
            while inner.last_id().await.unwrap() != Some(12) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        assert!(waited.is_ok(), "never recorded: {:?}", ids(&inner).await);
        let recorded = started.elapsed();
        assert!(
            recorded - pruned < INTERVAL / 3,
            "recorded {recorded:?}, {pruned:?} after the start: it waited for the next pass"
        );
        stop.send(()).unwrap();
        serving.await.unwrap().unwrap();
    }

    /// A store that refuses a prune: one that cannot prune ends retention, and a failure
    /// lets the start go on and tries again within a minute.
    #[tokio::test]
    async fn a_store_that_cannot_prune_ends_retention_and_a_failure_is_tried_again() {
        struct Refusing(Arc<dyn AuditStore>, TraitError);
        #[async_trait::async_trait]
        impl AuditStore for Refusing {
            async fn append(&self, entry: &AuditEntry) -> Result<(), TraitError> {
                self.0.append(entry).await
            }
            async fn query(
                &self,
                filter: &AuditFilter,
                pagination: &Pagination,
            ) -> Result<Page<AuditEntry>, TraitError> {
                self.0.query(filter, pagination).await
            }
            async fn prune(&self, _: &AuditPrune<'_>) -> Result<Pruned, TraitError> {
                Err(match &self.1 {
                    TraitError::Unsupported(text) => TraitError::Unsupported(text),
                    _ => TraitError::DatabaseError("injected".into()),
                })
            }
        }
        let (_directory, storage, _) = fixture().await;
        let audit = storage.audit.clone();
        let cannot: Arc<dyn AuditStore> =
            Arc::new(Refusing(audit.clone(), TraitError::Unsupported("no")));
        let mut task = pruning_task(storage, cannot, false);
        task.prune_at_start().await;
        assert!(task.pruning.is_none());
        assert_eq!(task.next_prune(), None);

        let (_directory, storage, last) = fixture().await;
        let audit = storage.audit.clone();
        let failing: Arc<dyn AuditStore> = Arc::new(Refusing(
            audit.clone(),
            TraitError::DatabaseError(String::new()),
        ));
        let mut task = pruning_task(storage, failing, false);
        task.prune_at_start().await;
        let pruning = task.pruning.as_ref().unwrap();
        assert!(pruning.started && pruning.pass.is_none());
        assert!(task.next_prune().unwrap() <= PRUNE_RETRY);
        assert_eq!(ids(&audit).await.len() as u64, last);
    }

    const HOLD: Duration = Duration::from_secs(5);

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    /// A first status, and a more severe one, are recorded at once; a less
    /// severe one only after the hold, and at once with no hold.
    #[test]
    fn a_clearing_waits_out_the_hold_and_a_worsening_does_not() {
        use LimitStatus::*;
        let id = LimitId::new("net").unwrap();
        let t0 = Instant::now();
        let mut held = Holds::default();

        assert!(held.records(&id, None, Within, t0, HOLD), "a first status");
        assert!(held.records(&id, Some(Within), Breach, t0, HOLD));
        assert!(held.records(&id, Some(Warning), Unknown, t0, HOLD));
        assert!(
            !held.records(&id, Some(Breach), Breach, t0, HOLD),
            "no change"
        );
        assert!(held.0.is_empty());

        assert!(
            held.records(&id, Some(Breach), Within, t0, Duration::ZERO),
            "no hold"
        );
        assert!(held.0.is_empty());

        assert!(!held.records(&id, Some(Breach), Within, t0, HOLD));
        assert!(!held.records(&id, Some(Breach), Within, t0 + HOLD - ms(1), HOLD));
        // Less severe throughout, though not the same status: the hold runs on.
        assert!(!held.records(&id, Some(Breach), Warning, t0 + ms(2_000), HOLD));
        assert!(held.records(&id, Some(Breach), Warning, t0 + HOLD, HOLD));
        // Not yet written (a refused write): still due at the next evaluation.
        assert!(held.records(&id, Some(Breach), Within, t0 + HOLD + ms(1), HOLD));
    }

    /// A return to the recorded status, or past it, ends the hold; the next clearing waits
    /// a whole hold again.
    #[test]
    fn a_flap_within_the_hold_starts_it_afresh() {
        use LimitStatus::*;
        let id = LimitId::new("net").unwrap();
        let t0 = Instant::now();
        let mut held = Holds::default();
        assert!(!held.records(&id, Some(Breach), Within, t0, HOLD));
        assert!(!held.records(&id, Some(Breach), Breach, t0 + ms(1_000), HOLD));
        assert!(held.0.is_empty());
        assert!(!held.records(&id, Some(Breach), Within, t0 + ms(2_000), HOLD));
        assert!(
            !held.records(&id, Some(Breach), Within, t0 + HOLD, HOLD),
            "3 s, not 5"
        );
        assert!(held.records(&id, Some(Breach), Within, t0 + ms(7_000), HOLD));

        // Past it: a warning recorded, then a clearing held, then a breach recorded at once.
        assert!(!held.records(&id, Some(Warning), Within, t0, HOLD));
        assert!(held.records(&id, Some(Warning), Breach, t0 + ms(1), HOLD));
        assert!(held.0.is_empty());
    }

    /// The monitor wakes once for each hold's end, and drops the holds of removed limits.
    #[test]
    fn the_monitor_wakes_once_for_a_holds_end() {
        use LimitStatus::*;
        let (net, face) = (LimitId::new("net").unwrap(), LimitId::new("face").unwrap());
        let t0 = Instant::now();
        let mut held = Holds::default();
        assert!(!held.records(&net, Some(Breach), Within, t0, HOLD));
        assert!(!held.records(&face, Some(Breach), Within, t0 + ms(1_000), HOLD));
        assert_eq!(held.next_end(HOLD), Some(t0 + HOLD));

        assert!(!held.wake(t0 + HOLD - ms(1), HOLD));
        assert!(held.wake(t0 + HOLD, HOLD));
        assert!(!held.wake(t0 + HOLD, HOLD), "once");
        assert_eq!(held.next_end(HOLD), Some(t0 + ms(6_000)));

        let face_only: Vec<LimitDefinition> = vec![serde_json::from_value(serde_json::json!({
            "limit_id": "face", "measure": {"kind": "market_value"}, "upper": "100",
        }))
        .unwrap()];
        held.retain(&face_only);
        assert!(!held.0.contains_key(&net));
        assert!(held.wake(t0 + ms(6_000), HOLD));
        assert_eq!(held.next_end(HOLD), None);
    }

    /// Only an expiry moves the latch: one still to come stays pending, and a clock that
    /// jumps ahead and returns holds back only what it passed.
    #[test]
    fn the_latch_holds_only_expiries_passed() {
        const FIRST: &str = "2026-11-09T12:00:00Z";
        const SECOND: &str = "2026-11-09T18:00:00Z";
        let limits = limits(&[SECOND, FIRST]);
        let mut through = None;
        assert_eq!(next_expiry_of(&limits, through), Some(stamp(FIRST)));

        // Ahead to 13:00, past the first expiry only, then back.
        latch(&limits, &mut through, stamp("2026-11-09T13:00:00Z"));
        assert_eq!(through, Some(stamp(FIRST)));
        let back = latch(&limits, &mut through, stamp("2026-11-09T11:00:00Z"));
        assert_eq!(back, stamp(FIRST), "not the 13:00 the clock jumped to");
        assert!(limits[0].excess_at(back).is_some());
        assert!(limits[1].excess_at(back).is_none());
        assert_eq!(next_expiry_of(&limits, through), Some(stamp(SECOND)));

        // A set without an excess moves nothing.
        let mut none = None;
        let plain: Vec<LimitDefinition> = vec![serde_json::from_value(serde_json::json!({
            "limit_id": "plain", "measure": {"kind": "market_value"}, "upper": "100",
        }))
        .unwrap()];
        let at = stamp(FIRST);
        assert_eq!(latch(&plain, &mut none, at), at);
        assert!(none.is_none());
        assert_eq!(next_expiry_of(&plain, none), None);
    }
}
