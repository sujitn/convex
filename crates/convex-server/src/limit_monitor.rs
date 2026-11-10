//! The limit monitor (P14c; D86, D87).
//!
//! One task, off the tick path, watches every book with a non-empty limit set (at most
//! 64, D87). It listens to the runtime's results stream as a WebSocket
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
//! A status *change* is recorded before it is published (D86): one `AuditStore` record per
//! limit whose status differs from its last recorded one, then one `limit_status` frame.
//! One evaluation of a book writes all its records in one `append_all`, a single
//! transaction (D92), so a restart's first pass is one commit a book rather than one a
//! limit. A failed batch records none of the book's changes and keeps their last recorded
//! statuses, so a later evaluation records them again; a book with a failed batch is
//! evaluated again within a second, unless the store refused it for capacity, which a
//! second later would not change. A restart begins with nothing recorded, so its first
//! evaluation records every limit's status as a change from `unevaluated`, naming the
//! start. A stop is seen between books, so shutdown waits for one book's commit at most;
//! what was not recorded is recorded at the next start.
//!
//! A `breach` or `unknown` change carries the acknowledgement in force for its limit at
//! the evaluation (P15c). The monitor rereads a watched book's acknowledgements when they
//! commit, as it rereads its set; an acknowledgement changes no status, so it records and
//! publishes nothing of its own (D97).
//!
//! A temporary excess (P15d, D96) changes a status twice: when it is approved, which is a
//! commit, and when it expires, which is nothing the streams report. So the earliest expiry
//! still to come among the watched sets is a deadline beside the retry timer (amending
//! D87), and the books holding it are evaluated when it passes. A wall clock that steps
//! back would apply an expired excess again for the length of the step, so each book keeps
//! the latest expiry this monitor has passed and reads its excesses no earlier than that.
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
use convex_engine::ports::storage::{AuditEntry, AuditStore, MAX_AUDIT_BATCH};
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
/// The entries the audit store holds, read when the monitor starts and after each commit.
/// The store refuses every append at 1,000,000 (D98).
pub const AUDIT_ENTRIES: &str = "convex_audit_entries";

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
const MAX_TERMS: usize = 2 * convex_engine::ports::snapshot::MAX_SNAPSHOT_INSTRUMENTS;

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

/// The acknowledgement in force for a `breach` or `unknown` limit (P15c; D95, D97), as
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
}

impl LimitMonitor {
    pub(crate) fn new(audit: Arc<dyn AuditStore>) -> Arc<Self> {
        Arc::new(Self {
            audit,
            feed: broadcast::channel(FEED_CAPACITY).0,
        })
    }

    pub(crate) fn feed(&self) -> LimitFeed {
        LimitFeed(self.feed.clone())
    }

    /// The store the monitor records in, which the records route reads (P15b).
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
            },
            halt,
            books: BTreeMap::new(),
            holders: HashMap::new(),
            terms: PricedTerms::new(),
            reload: Some(Pending::All),
            dirty: Some(Pending::All),
            retry: BTreeSet::new(),
            stalled: false,
        };
        let mut timer = tokio::time::interval(RETRY);
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            // A stop that arrived during the last unit of work, or before the first.
            if task.halt.requested() {
                break;
            }
            task.expire();
            task.work().await;
            let expiry = task.next_expiry();
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
                // The next pass of the loop reads the clock and finds what has expired.
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

/// A watched book, and each limit's last recorded status.
struct Watched {
    subject: Arc<Subject>,
    recorded: HashMap<LimitId, LimitStatus>,
    /// The latest excess expiry this monitor has passed for the book.
    expired_through: Option<UtcTimestamp>,
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

struct Task {
    engine: Arc<PricingEngine>,
    ledger: Ledger,
    halt: Stop,
    books: BTreeMap<BookId, Watched>,
    /// Each instrument a watched book holds, and the books holding it.
    holders: HashMap<String, BTreeSet<BookId>>,
    terms: PricedTerms,
    reload: Option<Pending>,
    dirty: Option<Pending>,
    /// Books with a change that could not be recorded.
    retry: BTreeSet<BookId>,
    /// A read failed; it is tried again on the timer.
    stalled: bool,
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

    fn committed(&mut self, keys: &[MasterKey]) {
        for key in keys {
            match key {
                MasterKey::LimitSet(id) => Pending::add(&mut self.reload, id.clone()),
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
            let Some((set_version, set)) = set.filter(|(_, set)| !set.limits.is_empty()) else {
                self.unwatch(&id);
                continue;
            };
            if !self.books.contains_key(&id) && self.books.len() >= MAX_LIMITED_BOOKS {
                tracing::error!(book = %id, "limit monitor: more than {MAX_LIMITED_BOOKS} books hold a limit set; this one is not watched");
                continue;
            }
            let book = match repo.get_master(&MasterKey::Book(id.clone())).await? {
                Some(record) => Some((record.version, book_of(&record)?.clone())),
                None => None,
            };
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
            let (recorded, expired_through) = self
                .books
                .remove(&id)
                .map(|watched| (watched.recorded, watched.expired_through))
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
        let accepted: Accepted = accepted_results(&self.engine);
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
            };
            match self
                .ledger
                .record(
                    context,
                    &subject.limits,
                    &subject.acknowledgements,
                    evaluation,
                    &mut watched.recorded,
                )
                .await
            {
                // A full store stays full: the book's next evaluation tries again, not a
                // timer.
                Recorded::All | Recorded::Full | Recorded::Stopped => {
                    self.retry.remove(id);
                }
                Recorded::Retry => {
                    self.retry.insert(id.clone());
                }
            }
            gauges(id, &watched.recorded);
        }
        if self.terms.len() > MAX_TERMS {
            self.terms.retain(|(instrument, version), _| {
                accepted
                    .get(instrument.as_str())
                    .is_some_and(|versions| versions.contains_key(&version.get()))
            });
        }
        metrics::histogram!(MONITOR_SECONDS).record(started.elapsed().as_secs_f64());
    }
}

/// Publish a watched book's counts of limits in breach and unknown, as last recorded.
fn gauges(id: &BookId, recorded: &HashMap<LimitId, LimitStatus>) {
    let count = |status| recorded.values().filter(|s| **s == status).count() as f64;
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
}

impl Ledger {
    /// Publish how many entries the store holds; an unreadable count leaves the gauge.
    async fn count_entries(&self) {
        match self.audit.entry_count().await {
            Ok(count) => metrics::gauge!(AUDIT_ENTRIES).set(count as f64),
            Err(error) => {
                tracing::debug!(%error, "limit monitor: the audit store's entry count is unreadable")
            }
        }
    }

    /// Record, then publish, each limit whose status differs from its last recorded one:
    /// the book's records in one batch, then their frames in limit order.
    async fn record(
        &mut self,
        context: Context<'_>,
        limits: &[LimitDefinition],
        acknowledgements: &[LimitAcknowledgement],
        evaluation: crate::limits::BookEvaluation,
        recorded: &mut HashMap<LimitId, LimitStatus>,
    ) -> Recorded {
        recorded.retain(|id, _| limits.iter().any(|limit| limit.limit_id == *id));
        let snapshot_ids: Arc<[String]> = evaluation.snapshot_ids.into();
        let mut complete = true;
        let mut entries = Vec::new();
        let mut changes = Vec::new();
        for (limit, evaluation) in limits.iter().zip(evaluation.evaluations) {
            let previous = recorded.get(&limit.limit_id).copied();
            if previous == Some(evaluation.status) {
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
                    recorded.insert(limit_id, status);
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
                    tracing::error!(book = %context.book_id, "limit monitor: the audit store refuses status changes for capacity (it holds at most 1,000,000 entries and keeps them all); changes are neither recorded nor published until it accepts one");
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

    /// P15d: at, just before and just after an expiry, and after a clock that steps back
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
