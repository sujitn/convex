//! Desk closes of a book, and the P&L since the latest one.
//!
//! A close records each position of a book in force and its value from the current Live
//! results, read as limit status reads them: one `MasterKey::BookClose` record per book in
//! the reference repository, each version one close, at most one a UTC date. The desk takes
//! one with `POST /api/v1/books/{book_id}/close`. With `[books] close_time_utc` set, the
//! server takes one of each watched book at that time, Monday to Friday; a close the server
//! was not running for, or woke more than five minutes late for, is not taken.
//!
//! A first close of a book not yet watched is refused when `watched_books` holds as many
//! as it allows (409 `WATCHED_BOOKS_EXCEEDED`).
//!
//! P&L since the close (`GET /api/v1/books/{book_id}/pnl`) lines the latest close up
//! against the book and the Live results now: `convex_portfolio::pnl_since_close`, with the
//! cash each position held through became entitled to between the close's settlement date
//! and now's.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
// Internal early-return errors are the finished HTTP response itself.
#![allow(clippy::result_large_err)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{DateTime, Datelike, NaiveTime, Utc, Weekday};
use convex_core::contracts::{
    BusinessDate, ExactDecimal, RequestId, SnapshotId, SourceId, UtcTimestamp, Version,
};
use convex_engine::book_close::{
    holdings_since_close, position_value, CurrentPosition, PricedQuote,
};
use convex_engine::ports::book_close::{
    BookCloseDefinition, CloseBasis, CloseTaker, ClosedPosition, PositionValue,
};
use convex_engine::ports::reference_master::*;
use convex_engine::ports::reference_repository::*;
use convex_portfolio::pnl_since_close::{pnl_since_close, HoldingSinceClose, NoPnlReason};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::sync::watch;

use crate::access::{recorded_subject, AccessContext, Action};
use crate::books::{accepted_runtime_results, book_id, book_of, in_force, typed_error};
use crate::handlers::AppState;
use crate::limits::{
    authorize, authorize_book_read, commit, commit_time, conflict, current, different_payload,
    read_master, read_positions, read_priced_terms, retained, PositionReads, PricedTerms,
    STATUS_DATA,
};
use crate::reference::{key, now, reference_error, with_reference_version};
use crate::watched_books::{may_watch, too_many_watched, watched_books};

/// Source recorded on every close.
const BOOK_CLOSE: &str = "book-close";
/// How late after its time a scheduled close is still taken.
const CLOSE_GRACE: chrono::TimeDelta = chrono::TimeDelta::minutes(5);
/// The longest the schedule sleeps between clock reads, so a wall clock that steps is
/// seen within this.
const CLOCK_STEP: Duration = Duration::from_secs(30);

pub(crate) fn close_of(record: &MasterRecord) -> Result<&BookCloseDefinition, ReferenceError> {
    match &record.definition {
        MasterDefinition::BookClose(close) => Ok(close),
        _ => Err(ReferenceError::Unsupported("not a book close")),
    }
}

/// The refusal of a generic reference write, or commit read, that names a close.
pub(crate) fn close_route_required() -> Response {
    typed_error(
        StatusCode::FORBIDDEN,
        "BOOK_CLOSE_ROUTE_REQUIRED",
        "closes are read and written only through /api/v1/books/{book_id}/close",
    )
}

fn unavailable() -> Response {
    reference_error(ReferenceError::Unavailable)
}

// =============================================================================
// VIEWS
// =============================================================================

#[derive(Serialize)]
struct CloseView<'a> {
    book_id: &'a BookId,
    version: Version,
    recorded_at: UtcTimestamp,
    close_date: BusinessDate,
    basis: CloseBasis,
    taken_by: &'a CloseTaker,
    taken_at: UtcTimestamp,
    book_version: Version,
    currency: &'a str,
    settlement_date: BusinessDate,
    snapshot_ids: &'a [SnapshotId],
    evidence_reference: &'a str,
    positions: &'a [ClosedPosition],
}

fn close_response(record: &MasterRecord, status: StatusCode) -> Response {
    let close = match close_of(record) {
        Ok(close) => close,
        Err(error) => return reference_error(error),
    };
    let view = CloseView {
        book_id: &close.book_id,
        version: record.version,
        recorded_at: record.recorded_at,
        close_date: close.close_date,
        basis: close.basis,
        taken_by: &close.taken_by,
        taken_at: close.taken_at(),
        book_version: close.book_version,
        currency: &close.currency,
        settlement_date: close.settlement_date,
        snapshot_ids: &close.snapshot_ids,
        evidence_reference: &close.provenance.evidence_reference,
        positions: &close.positions,
    };
    with_reference_version((status, Json(view)).into_response(), record.version)
}

fn closed_instruments(close: &BookCloseDefinition) -> impl Iterator<Item = String> + '_ {
    close
        .positions
        .iter()
        .map(|position| position.instrument_id.as_str().to_owned())
}

fn held_instruments(book: &BookDefinition) -> impl Iterator<Item = String> + '_ {
    book.positions
        .iter()
        .map(|position| position.instrument_id.as_str().to_owned())
}

// =============================================================================
// TAKING A CLOSE
// =============================================================================

/// `POST /api/v1/books/{book_id}/close`: close the book now.
///
/// `Idempotency-Key` is required, and there is no body. The caller holds `BookClose` on
/// the book and each instrument it holds, with status's data. The book must be in force
/// (404, or 409 `BOOK_RETIRED`) and name its currency (422 `BOOK_CURRENCY_REQUIRED`); its
/// latest close must be for an earlier UTC date (409 `BOOK_ALREADY_CLOSED`); a first close
/// must leave at most 64 books watched (409 `WATCHED_BOOKS_EXCEEDED`); and the close must
/// value at least one position (422 `BOOK_CLOSE_UNVALUED`). Answers `201` with the close.
pub(crate) async fn take_close(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
) -> Response {
    let (id, idempotency_key) = match (book_id(raw), key(&headers)) {
        (Ok(id), Ok(idempotency_key)) => (id, idempotency_key),
        (Err(error), _) | (_, Err(error)) => return reference_error(error),
    };
    let _writes = state.limit_writes.lock().await;
    match close_book(&state, Closer::Desk(&ctx), id, idempotency_key).await {
        Ok(record) => close_response(&record, StatusCode::CREATED),
        Err(refused) => refused.into_response(),
    }
}

/// Who takes a close.
enum Closer<'c> {
    /// The desk, as the caller of the close route.
    Desk(&'c Option<Extension<AccessContext>>),
    /// The schedule, which is not authorized.
    Schedule,
}

impl Closer<'_> {
    fn taker(&self) -> CloseTaker {
        match self {
            Self::Desk(ctx) => CloseTaker::Desk {
                subject: recorded_subject(ctx),
            },
            Self::Schedule => CloseTaker::Schedule {},
        }
    }

    /// `BookClose` on the book and each of `instruments`, for the desk.
    fn authorize(
        &self,
        state: &AppState,
        book: &BookId,
        instruments: impl IntoIterator<Item = String>,
    ) -> Result<(), Response> {
        match self {
            Self::Desk(ctx) => authorize(
                state,
                ctx,
                Action::BookClose,
                STATUS_DATA,
                book,
                instruments,
            ),
            Self::Schedule => Ok(()),
        }
    }
}

/// Why a close was not taken.
enum CloseRefused {
    /// The book is already closed for the date, which the schedule expects after a restart.
    AlreadyClosed,
    /// Any other refusal, as its response.
    Refused(Response),
}

impl From<Response> for CloseRefused {
    fn from(response: Response) -> Self {
        Self::Refused(response)
    }
}

impl IntoResponse for CloseRefused {
    fn into_response(self) -> Response {
        match self {
            Self::AlreadyClosed => conflict(
                "BOOK_ALREADY_CLOSED",
                "a book is closed at most once a UTC date",
            ),
            Self::Refused(response) => response,
        }
    }
}

/// Close `id` now, for `closer`, under the limit write lock.
async fn close_book(
    state: &AppState,
    closer: Closer<'_>,
    id: BookId,
    idempotency_key: RequestId,
) -> Result<MasterRecord, CloseRefused> {
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let record_key = MasterKey::BookClose(id.clone());
    let taken_by = closer.taker();
    // An exact retry returns the close it wrote, whatever the book has since become.
    if let Some(record) = retained(repo, &idempotency_key, &record_key)
        .await
        .map_err(reference_error)?
    {
        let stored = close_of(&record).map_err(reference_error)?;
        closer.authorize(state, &id, closed_instruments(stored))?;
        if stored.taken_by != taken_by {
            return Err(reference_error(different_payload()).into());
        }
        return Ok(record);
    }
    let book_record = in_force(
        current(repo, MasterKey::Book(id.clone()))
            .await
            .map_err(reference_error)?,
    )?;
    let book = book_of(&book_record).map_err(reference_error)?;
    closer.authorize(state, &id, held_instruments(book))?;
    let currency = book_currency(book)?;
    let existing = current(repo, record_key).await.map_err(reference_error)?;
    let at =
        commit_time(existing.iter().map(|record| record.recorded_at)).map_err(reference_error)?;
    let close_date = utc_date(at)?;
    match &existing {
        Some(record) => {
            if close_of(record).map_err(reference_error)?.close_date >= close_date {
                return Err(CloseRefused::AlreadyClosed);
            }
        }
        None => {
            if !may_watch(repo, &id).await.map_err(reference_error)? {
                return Err(too_many_watched().into());
            }
        }
    }
    let settlement = state
        .engine
        .runtime_settlement_date()
        .ok_or_else(unavailable)?;
    let mut terms = PricedTerms::new();
    let reads = read_book(state, book, settlement, &mut terms).await;
    let positions: Vec<ClosedPosition> = book
        .positions
        .iter()
        .zip(position_values(book, &reads, currency))
        .map(|(position, value)| ClosedPosition {
            instrument_id: position.instrument_id.clone(),
            quantity: position.quantity,
            unit: position.unit,
            value,
        })
        .collect();
    if !positions
        .iter()
        .any(|position| matches!(position.value, PositionValue::Valued { .. }))
    {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "code": "BOOK_CLOSE_UNVALUED",
                "error": "a close values at least one position",
                "book_id": id,
                "book_version": book_record.version,
                "positions": positions.len(),
            })),
        )
            .into_response()
            .into());
    }
    let definition = BookCloseDefinition {
        book_id: id,
        close_date,
        basis: CloseBasis::DeskClose,
        taken_by,
        book_version: book_record.version,
        currency: currency.to_string(),
        settlement_date: settlement,
        snapshot_ids: reads.snapshot_ids.into_iter().collect(),
        positions,
        effective: EffectiveInterval {
            from: at,
            until: None,
        },
        provenance: ReferenceProvenance {
            source: SourceId::new(BOOK_CLOSE).map_err(|_| unavailable())?,
            evidence_reference: format!("{BOOK_CLOSE}:{idempotency_key}"),
        },
    };
    let change = MasterChange {
        expected_version: existing.map(|record| record.version),
        definition: MasterDefinition::BookClose(Box::new(definition)),
    };
    Ok(commit(state, idempotency_key, vec![change], at)
        .await
        .map_err(reference_error)?
        .into_iter()
        .next()
        .ok_or_else(unavailable)?)
}

/// Each position of `book` read from the Live results now, at the runtime's `settlement`
/// date, with the priced terms it reads in `terms`.
async fn read_book<'t>(
    state: &AppState,
    book: &BookDefinition,
    settlement: BusinessDate,
    terms: &'t mut PricedTerms,
) -> PositionReads<'t> {
    let accepted = accepted_runtime_results(state, book);
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    read_priced_terms(repo, book, &accepted, terms).await;
    read_positions(book, &accepted, terms, Some(settlement))
}

/// Each position's value from its read, in book order: what a close records and the P&L
/// reads now.
fn position_values(
    book: &BookDefinition,
    reads: &PositionReads<'_>,
    currency: convex_core::Currency,
) -> Vec<PositionValue> {
    book.positions
        .iter()
        .zip(&reads.reads)
        .map(|(position, read)| {
            let priced = read.as_ref().map(|read| PricedQuote {
                quote: &read.quote,
                instrument_version: read.instrument_version,
            });
            position_value(&position.instrument_id, position.unit, priced, currency)
        })
        .collect()
}

fn book_currency(book: &BookDefinition) -> Result<convex_core::Currency, Response> {
    book.reporting_currency().ok_or_else(|| {
        typed_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "BOOK_CURRENCY_REQUIRED",
            "a close and P&L need the book's reporting currency",
        )
    })
}

/// The UTC date of `at`.
fn utc_date(at: UtcTimestamp) -> Result<BusinessDate, Response> {
    BusinessDate::try_from(convex_core::Date::from(at.datetime().date_naive()))
        .map_err(|_| unavailable())
}

// =============================================================================
// READING A CLOSE
// =============================================================================

/// `GET /api/v1/books/{book_id}/close`: the latest close, under status's read rule on the
/// book and each instrument the book and the close hold, with its version in `ETag`.
pub(crate) async fn get_close(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
) -> Response {
    let read = async {
        let id = book_id(raw).map_err(reference_error)?;
        let record = current(
            state.engine.reference_repository().as_ref(),
            MasterKey::BookClose(id.clone()),
        )
        .await
        .map_err(reference_error)?;
        readable(&state, &ctx, &id, record.as_ref()).await?;
        let record = record.ok_or_else(|| reference_error(ReferenceError::NotFound))?;
        Ok::<_, Response>(close_response(&record, StatusCode::OK))
    };
    match read.await {
        Ok(response) | Err(response) => response,
    }
}

/// `GET /api/v1/books/{book_id}/close/versions/{version}`: one close, as written.
pub(crate) async fn get_close_version(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path((raw, version)): Path<(String, u64)>,
) -> Response {
    let read = async {
        let id = book_id(raw).map_err(reference_error)?;
        let version = Version::new(version)
            .map_err(|_| reference_error(ReferenceError::InvalidInput("invalid version")))?;
        let record = read_master(&state, &MasterKey::BookClose(id.clone()), Some(version)).await?;
        readable(&state, &ctx, &id, Some(&record)).await?;
        Ok::<_, Response>(close_response(&record, StatusCode::OK))
    };
    match read.await {
        Ok(response) | Err(response) => response,
    }
}

/// Status's read rule on the book, each instrument it holds now and each `close` holds.
/// Returns the book's current record.
async fn readable(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    id: &BookId,
    close: Option<&MasterRecord>,
) -> Result<Option<MasterRecord>, Response> {
    let closed = match close {
        Some(record) => closed_instruments(close_of(record).map_err(reference_error)?).collect(),
        None => Vec::new(),
    };
    authorize_book_read(state, ctx, STATUS_DATA, id, closed).await
}

// =============================================================================
// P&L SINCE THE CLOSE
// =============================================================================

#[derive(Serialize)]
struct CloseSummary {
    version: Version,
    close_date: BusinessDate,
    taken_at: UtcTimestamp,
    book_version: Version,
    settlement_date: BusinessDate,
}

#[derive(Serialize)]
struct PositionPnlView<'a> {
    instrument_id: &'a str,
    close_face: ExactDecimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    close_dirty_price: Option<ExactDecimal>,
    face: ExactDecimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    dirty_price: Option<ExactDecimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    held_face: Option<ExactDecimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pnl: Option<ExactDecimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entitled_cash: Option<ExactDecimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    no_pnl: Option<NoPnlReason>,
}

#[derive(Serialize)]
struct PnlView<'a> {
    book_id: &'a BookId,
    book_version: Version,
    close: CloseSummary,
    currency: &'a str,
    settlement_date: BusinessDate,
    evaluated_at: UtcTimestamp,
    snapshot_ids: Vec<SnapshotId>,
    pnl: ExactDecimal,
    entitled_cash: ExactDecimal,
    excluded_count: usize,
    positions: Vec<PositionPnlView<'a>>,
}

/// `GET /api/v1/books/{book_id}/pnl`: P&L since the latest close, against the Live
/// results now, under status's read rule on the book and each instrument the book and the
/// close hold.
///
/// Each position held at the close or now is listed: the face and dirty price per 100 at
/// each, and its P&L on the face held through with the entitled cash, or why it has none.
/// The total leaves out each position with none, and counts them. Refused: a book retired
/// (409 `BOOK_RETIRED`) or without a close (404 `BOOK_CLOSE_NOT_FOUND`); a book currency
/// other than the close's (409 `BOOK_CURRENCY_CHANGED`); and a close that settles after
/// the runtime (409 `BOOK_CLOSE_SETTLES_LATER`).
pub(crate) async fn get_pnl(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
) -> Response {
    match pnl(&state, &ctx, raw).await {
        Ok(response) | Err(response) => response,
    }
}

async fn pnl(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    raw: String,
) -> Result<Response, Response> {
    let id = book_id(raw).map_err(reference_error)?;
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let close_record = current(repo, MasterKey::BookClose(id.clone()))
        .await
        .map_err(reference_error)?;
    let book_record = in_force(readable(state, ctx, &id, close_record.as_ref()).await?)?;
    let close_record = close_record.ok_or_else(|| {
        typed_error(
            StatusCode::NOT_FOUND,
            "BOOK_CLOSE_NOT_FOUND",
            "the book has no close",
        )
    })?;
    let close = close_of(&close_record).map_err(reference_error)?;
    let book = book_of(&book_record).map_err(reference_error)?;
    let currency = book_currency(book)?;
    if currency.to_string() != close.currency {
        return Err(conflict(
            "BOOK_CURRENCY_CHANGED",
            "the book's reporting currency is not the close's",
        ));
    }
    let settlement = state
        .engine
        .runtime_settlement_date()
        .ok_or_else(unavailable)?;
    if close.settlement_date > settlement {
        return Err(conflict(
            "BOOK_CLOSE_SETTLES_LATER",
            "the close settles after the runtime's settlement date",
        ));
    }
    let evaluated_at = now().map_err(reference_error)?;
    let mut terms = PricedTerms::new();
    let reads = read_book(state, book, settlement, &mut terms).await;
    let values = position_values(book, &reads, currency);
    let current_positions: Vec<CurrentPosition<'_>> = book
        .positions
        .iter()
        .zip(&values)
        .map(|(position, value)| CurrentPosition {
            instrument_id: &position.instrument_id,
            quantity: position.quantity.decimal(),
            value,
        })
        .collect();
    let refused = |error: &dyn std::fmt::Display| {
        tracing::error!(%error, book = %close.book_id, "book P&L: refused");
        unavailable()
    };
    let mut holdings = holdings_since_close(close, &current_positions).map_err(|e| refused(&e))?;
    let (from, to) = (close.settlement_date.date(), settlement.date());
    fill_entitled_cash(state, book, &reads, &mut holdings, from, to)
        .await
        .map_err(|e| refused(&e))?;
    let pnl = pnl_since_close(&holdings).map_err(|error| {
        typed_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "BOOK_PNL_REFUSED",
            &error.to_string(),
        )
    })?;
    let positions = holdings
        .iter()
        .zip(&pnl.positions)
        .map(|(holding, position)| {
            let held = position.held.ok();
            PositionPnlView {
                instrument_id: holding.instrument_id,
                close_face: ExactDecimal::new(holding.close_face),
                close_dirty_price: holding.close_dirty_price.map(ExactDecimal::new),
                face: ExactDecimal::new(holding.face),
                dirty_price: holding.dirty_price.map(ExactDecimal::new),
                held_face: held.map(|held| ExactDecimal::new(held.held_face)),
                pnl: held.map(|held| ExactDecimal::new(held.pnl)),
                entitled_cash: held.map(|held| ExactDecimal::new(held.entitled_cash)),
                no_pnl: position.held.err(),
            }
        })
        .collect();
    let view = PnlView {
        book_id: &book.book_id,
        book_version: book_record.version,
        close: CloseSummary {
            version: close_record.version,
            close_date: close.close_date,
            taken_at: close.taken_at(),
            book_version: close.book_version,
            settlement_date: close.settlement_date,
        },
        currency: &close.currency,
        settlement_date: settlement,
        evaluated_at,
        snapshot_ids: reads.snapshot_ids.into_iter().collect(),
        pnl: ExactDecimal::new(pnl.pnl),
        entitled_cash: ExactDecimal::new(pnl.entitled_cash),
        excluded_count: pnl.excluded_count(),
        positions,
    };
    Ok(Json(view).into_response())
}

/// Fill in the entitled cash per 100 face between `from` and `to` of each holding that
/// needs it, from the priced version it is valued at now: `None` when it cannot be
/// projected.
async fn fill_entitled_cash(
    state: &AppState,
    book: &BookDefinition,
    reads: &PositionReads<'_>,
    holdings: &mut [HoldingSinceClose<'_>],
    from: convex_core::Date,
    to: convex_core::Date,
) -> Result<(), convex_engine::EngineError> {
    let by_instrument: HashMap<&str, _> = book
        .positions
        .iter()
        .zip(&reads.reads)
        .filter_map(|(position, read)| Some((position.instrument_id.as_str(), read.as_ref()?)))
        .collect();
    for holding in holdings.iter_mut().filter(|h| h.needs_entitled_cash()) {
        holding.entitled_cash = match by_instrument.get(holding.instrument_id) {
            Some(read) => {
                state
                    .engine
                    .entitled_cash(read.instrument_version, read.bond, from, to)
                    .await?
            }
            None => None,
        };
    }
    Ok(())
}

// =============================================================================
// THE SCHEDULE
// =============================================================================

/// The first weekday instant at `time` UTC strictly after `after`.
pub(crate) fn next_due(after: DateTime<Utc>, time: NaiveTime) -> DateTime<Utc> {
    let mut date = after.date_naive();
    loop {
        let due = date.and_time(time).and_utc();
        if due > after && !matches!(due.weekday(), Weekday::Sat | Weekday::Sun) {
            return due;
        }
        date = date.succ_opt().unwrap_or(date);
    }
}

/// Whether a close due at `due` is still taken at `at`: at most [`CLOSE_GRACE`] late.
pub(crate) fn is_on_time(due: DateTime<Utc>, at: DateTime<Utc>) -> bool {
    at >= due && at - due <= CLOSE_GRACE
}

/// Run the weekday schedule at `time` until `stop` or `shutdown`: at each due instant read
/// from `clock`, `take` the closes when on time, and log a missed one otherwise. A start
/// after the day's time waits for the next weekday.
pub(crate) async fn run_schedule<C, T, F>(
    time: NaiveTime,
    clock: C,
    mut stop: watch::Receiver<bool>,
    mut shutdown: watch::Receiver<bool>,
    mut take: T,
) where
    C: Fn() -> DateTime<Utc>,
    T: FnMut(DateTime<Utc>) -> F,
    F: std::future::Future<Output = ()>,
{
    let mut due = next_due(clock(), time);
    loop {
        if *stop.borrow() || *shutdown.borrow() {
            return;
        }
        let at = clock();
        if at >= due {
            if is_on_time(due, at) {
                take(due).await;
            } else {
                tracing::warn!(%due, "book close schedule: woke too late; this close is not taken");
            }
            due = next_due(clock(), time);
            continue;
        }
        let wait = (due - at).to_std().unwrap_or_default().min(CLOCK_STEP);
        tokio::select! {
            biased;
            changed = stop.changed() => if changed.is_err() { return },
            changed = shutdown.changed() => if changed.is_err() { return },
            () = tokio::time::sleep(wait) => {}
        }
    }
}

/// Take a close of each watched book, each under the limit write lock. A book already
/// closed for the date is skipped; any other refusal is logged and the next book taken.
pub(crate) async fn close_watched_books(state: &AppState, due: DateTime<Utc>) {
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let books = match watched_books(repo).await {
        Ok(books) => books,
        Err(error) => {
            tracing::error!(%error, "book close schedule: the watched books are unreadable");
            return;
        }
    };
    let date = due.date_naive();
    let mut taken = 0usize;
    for id in books {
        let digest = Sha256::digest(id.as_str().as_bytes());
        let hex: String = digest.iter().take(16).map(|b| format!("{b:02x}")).collect();
        let Ok(idempotency_key) = RequestId::new(format!("book-close:{date}:{hex}")) else {
            continue;
        };
        let _writes = state.limit_writes.lock().await;
        match close_book(state, Closer::Schedule, id.clone(), idempotency_key).await {
            Ok(_) => taken += 1,
            Err(CloseRefused::AlreadyClosed) => {}
            Err(CloseRefused::Refused(response)) => {
                let status = response.status();
                tracing::warn!(book = %id, %status, "book close schedule: a close was refused");
            }
        }
    }
    tracing::info!(%date, taken, "book close schedule: closes taken");
}

/// The schedule over `state`'s engine, at `time`, until `stop` or the engine shuts down.
pub(crate) async fn run(state: Arc<AppState>, time: NaiveTime, stop: watch::Receiver<bool>) {
    let shutdown = state.engine.subscribe_shutdown();
    run_schedule(time, Utc::now, stop, shutdown, move |due| {
        let state = state.clone();
        async move { close_watched_books(&state, due).await }
    })
    .await;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn time(text: &str) -> NaiveTime {
        NaiveTime::parse_from_str(text, "%H:%M").unwrap()
    }

    #[test]
    fn a_close_is_due_at_its_time_on_the_next_weekday() {
        // 2026-11-27 is a Friday.
        let friday = at("2026-11-27T21:00:00Z");
        assert_eq!(next_due(friday, time("21:30")), at("2026-11-27T21:30:00Z"));
        // At or after the day's time, the next weekday's: Monday after a Friday.
        assert_eq!(
            next_due(at("2026-11-27T21:30:00Z"), time("21:30")),
            at("2026-11-30T21:30:00Z")
        );
        assert_eq!(
            next_due(at("2026-11-28T09:00:00Z"), time("21:30")),
            at("2026-11-30T21:30:00Z")
        );
        assert_eq!(
            next_due(at("2026-11-30T22:00:00Z"), time("21:30")),
            at("2026-12-01T21:30:00Z")
        );
    }

    #[test]
    fn a_close_is_taken_at_most_five_minutes_late() {
        let due = at("2026-11-27T21:30:00Z");
        assert!(is_on_time(due, due));
        assert!(is_on_time(due, at("2026-11-27T21:35:00Z")));
        assert!(!is_on_time(due, at("2026-11-27T21:35:01Z")));
        assert!(!is_on_time(due, at("2026-11-27T21:29:59Z")));
    }

    /// A clock that starts at `start` and moves with tokio's paused time, jumping by
    /// `jump` once tokio's time passes `jump_after`.
    fn clock(
        start: DateTime<Utc>,
        jump_after: Duration,
        jump: chrono::TimeDelta,
    ) -> impl Fn() -> DateTime<Utc> {
        let origin = tokio::time::Instant::now();
        move || {
            let elapsed = origin.elapsed();
            let moved = start + chrono::TimeDelta::from_std(elapsed).unwrap();
            if elapsed > jump_after {
                moved + jump
            } else {
                moved
            }
        }
    }

    async fn taken_until(
        start: DateTime<Utc>,
        jump_after: Duration,
        jump: chrono::TimeDelta,
        run_for: Duration,
    ) -> Vec<DateTime<Utc>> {
        let (stop_tx, stop) = watch::channel(false);
        let (_shutdown_tx, shutdown) = watch::channel(false);
        let taken = Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = taken.clone();
        let schedule = tokio::spawn(run_schedule(
            time("21:30"),
            clock(start, jump_after, jump),
            stop,
            shutdown,
            move |due| {
                record.lock().unwrap().push(due);
                async {}
            },
        ));
        tokio::time::sleep(run_for).await;
        stop_tx.send(true).unwrap();
        schedule.await.unwrap();
        let taken = taken.lock().unwrap().clone();
        taken
    }

    #[tokio::test(start_paused = true)]
    async fn the_schedule_takes_each_weekday_close_on_time() {
        // From Friday 21:00 for four days: Friday's, then Monday's; none at the weekend.
        let taken = taken_until(
            at("2026-11-27T21:00:00Z"),
            Duration::MAX,
            chrono::TimeDelta::zero(),
            Duration::from_secs(4 * 86_400),
        )
        .await;
        assert_eq!(
            taken,
            [at("2026-11-27T21:30:00Z"), at("2026-11-30T21:30:00Z")]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_start_after_the_time_waits_for_the_next_weekday() {
        let taken = taken_until(
            at("2026-11-30T21:31:00Z"),
            Duration::MAX,
            chrono::TimeDelta::zero(),
            Duration::from_secs(86_400),
        )
        .await;
        assert_eq!(taken, [at("2026-12-01T21:30:00Z")]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_clock_that_steps_past_the_grace_misses_that_close() {
        // At 21:20 the wall clock steps forward an hour, as after a suspend: Monday's
        // close is missed, and Tuesday's is taken.
        let taken = taken_until(
            at("2026-11-30T21:00:00Z"),
            Duration::from_secs(20 * 60),
            chrono::TimeDelta::hours(1),
            Duration::from_secs(86_400 + 3_600),
        )
        .await;
        assert_eq!(taken, [at("2026-12-01T21:30:00Z")]);
    }
}
