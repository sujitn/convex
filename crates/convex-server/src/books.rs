//! Durable position books.
//!
//! A book is a `MasterKey::Book` resource in the reference repository, so every write is
//! one atomic, idempotent, compare-and-set commit with immutable history. Nothing here
//! keeps state of its own.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
// Internal early-return errors are the finished HTTP response itself.
#![allow(clippy::result_large_err)]

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use convex_core::contracts::{
    DataQuality, ExactDecimal, InstrumentId, RequestId, ResultStatus, SourceId, UtcTimestamp,
    ValuationSubject, Version,
};
use convex_engine::ports::output::PortfolioAnalyticsOutput;
use convex_engine::ports::reference_master::*;
use convex_engine::ports::reference_repository::*;
use convex_engine::ports::runtime::PublishedValuation;
use serde::{Deserialize, Serialize};

use crate::access::{AccessContext, Action, DataScope, Resource, ResourceKind};
use crate::handlers::AppState;
use crate::limits::{after, commit_time, Accepted};
use crate::reference::{
    expected_version, key, now, reference_error, with_consumed_reference_version,
    with_reference_version,
};

/// Rejected rows reported in one response; the request stops resolving after this many.
pub(crate) const MAX_ROW_ERRORS: usize = 100;
/// Largest page of `GET /api/v1/books` and of book history.
const MAX_BOOK_PAGE: usize = 500;
/// Owning system recorded when a snapshot does not name one.
const UNATTRIBUTED: &str = "unattributed";

// =============================================================================
// ERRORS
// =============================================================================

pub(crate) fn typed_error(status: StatusCode, code: &'static str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({"error":message, "code":code, "retryable":false})),
    )
        .into_response()
}

pub(crate) fn too_large() -> Response {
    typed_error(
        StatusCode::PAYLOAD_TOO_LARGE,
        "BOOK_TOO_LARGE",
        "book exceeds the position bound",
    )
}

pub(crate) struct RowError {
    pub(crate) index: usize,
    pub(crate) code: &'static str,
    pub(crate) reason: &'static str,
}

/// `422 BOOK_ROWS_REJECTED` naming each rejected row; `error` says what was refused.
pub(crate) fn rows_rejected(error: &str, errors: &[RowError], truncated: bool) -> Response {
    let rows: Vec<_> = errors
        .iter()
        .map(|e| serde_json::json!({"index":e.index, "code":e.code, "reason":e.reason}))
        .collect();
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(serde_json::json!({
            "error":error,
            "code":"BOOK_ROWS_REJECTED",
            "retryable":false,
            "rows":rows,
            "truncated":truncated,
        })),
    )
        .into_response()
}

pub(crate) fn book_id(raw: String) -> Result<BookId, ReferenceError> {
    BookId::new(raw).map_err(|_| ReferenceError::InvalidInput("invalid book identity"))
}

pub(crate) fn book_of(record: &MasterRecord) -> Result<&BookDefinition, ReferenceError> {
    match &record.definition {
        MasterDefinition::Book(book) => Ok(book),
        _ => Err(ReferenceError::Unsupported("not a book resource")),
    }
}

const RETIRED: &str = "the book is retired; a new snapshot revives it";

/// `409 BOOK_RETIRED`: a retired book is not valued, checked or given limits.
fn retired() -> Response {
    typed_error(StatusCode::CONFLICT, "BOOK_RETIRED", RETIRED)
}

/// The current version of a book that is not retired: `404` when there is none, and
/// `409 BOOK_RETIRED` when it is retired.
pub(crate) fn in_force(record: Option<MasterRecord>) -> Result<MasterRecord, Response> {
    let record = record.ok_or_else(|| reference_error(ReferenceError::NotFound))?;
    if book_of(&record).map_err(reference_error)?.is_retired() {
        return Err(retired());
    }
    Ok(record)
}

/// Whether a reference transaction writes a book, which only the book routes may: they
/// start each snapshot at its commit time.
pub(crate) fn writes_books(changes: &[MasterChange]) -> bool {
    changes
        .iter()
        .any(|change| matches!(change.definition, MasterDefinition::Book(_)))
}

/// The refusal of a generic reference write that names a book.
pub(crate) fn books_route_required() -> Response {
    typed_error(
        StatusCode::FORBIDDEN,
        "BOOKS_ROUTE_REQUIRED",
        "books are written only through /api/v1/books/{book_id}",
    )
}

/// If-Match is optional on a snapshot: absent means create-only.
pub(crate) fn optional_expected(headers: &HeaderMap) -> Result<Option<Version>, ReferenceError> {
    if headers.get_all("if-match").iter().next().is_none() {
        return Ok(None);
    }
    expected_version(headers).map(Some)
}

// =============================================================================
// WRITE PATH
// =============================================================================

/// The record a write is based on, and the commit time to reuse on an exact retry.
struct Base {
    record: Option<MasterRecord>,
    replay_at: Option<UtcTimestamp>,
}

/// Read the base explicitly. When the idempotency key already committed this book, the
/// base is that commit's pre-image and its recorded time is reused, so a retry rebuilds
/// the identical command and the repository returns the retained receipt (or a payload
/// conflict) instead of applying the edit twice.
async fn base_for(
    repo: &dyn ReferenceRepository,
    idempotency_key: &RequestId,
    id: &BookId,
) -> Result<Base, ReferenceError> {
    let master_key = MasterKey::Book(id.clone());
    if let Some(prior) = repo.get_commit(idempotency_key).await? {
        if let Some(done) = prior.master_records.first() {
            if done.definition.key() == master_key {
                let pre_image = match done.version.get().checked_sub(1).filter(|v| *v > 0) {
                    Some(v) => {
                        let version = Version::new(v).map_err(|_| ReferenceError::Unavailable)?;
                        let found = repo.get_master_version(&master_key, version).await?;
                        Some(found.ok_or(ReferenceError::Unavailable)?)
                    }
                    None => None,
                };
                return Ok(Base {
                    record: pre_image,
                    replay_at: Some(prior.recorded_at),
                });
            }
        }
    }
    Ok(Base {
        record: repo.get_master(&master_key).await?,
        replay_at: None,
    })
}

fn recorded_at(base: &Base) -> Result<UtcTimestamp, ReferenceError> {
    base.replay_at.map_or_else(now, Ok)
}

fn evidence(idempotency_key: &RequestId, owning_system: &SourceId) -> ReferenceProvenance {
    ReferenceProvenance {
        source: owning_system.clone(),
        evidence_reference: format!("books-api:{idempotency_key}"),
    }
}

async fn commit_book(
    state: &AppState,
    idempotency_key: RequestId,
    expected_version: Option<Version>,
    definition: BookDefinition,
    at: UtcTimestamp,
) -> Result<MasterRecord, ReferenceError> {
    let command = ReferenceCommand {
        idempotency_key,
        changes: Vec::new(),
        master_changes: vec![MasterChange {
            expected_version,
            definition: MasterDefinition::Book(Box::new(definition)),
        }],
    };
    let receipt = state.engine.commit_reference(&command, at).await?;
    receipt
        .master_records
        .into_iter()
        .next()
        .ok_or(ReferenceError::Unavailable)
}

// =============================================================================
// VIEWS
// =============================================================================

#[derive(Serialize)]
struct BookView<'a> {
    book_id: &'a BookId,
    version: Version,
    recorded_at: UtcTimestamp,
    effective_from: UtcTimestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    effective_until: Option<UtcTimestamp>,
    owning_system: &'a SourceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    currency: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: &'a Option<String>,
    evidence_reference: &'a str,
    positions: &'a [BookPosition],
}

fn book_view_value(record: &MasterRecord) -> Result<serde_json::Value, ReferenceError> {
    let book = book_of(record)?;
    serde_json::to_value(BookView {
        book_id: &book.book_id,
        version: record.version,
        recorded_at: record.recorded_at,
        effective_from: book.effective.from,
        effective_until: book.effective.until,
        owning_system: &book.owning_system,
        name: &book.name,
        currency: &book.currency,
        description: &book.description,
        evidence_reference: &book.provenance.evidence_reference,
        positions: &book.positions,
    })
    .map_err(|_| ReferenceError::Unavailable)
}

fn book_response(record: &MasterRecord, status: StatusCode) -> Response {
    match book_view_value(record) {
        Ok(view) => with_reference_version((status, Json(view)).into_response(), record.version),
        Err(error) => reference_error(error),
    }
}

fn summary(record: &MasterRecord) -> Result<serde_json::Value, ReferenceError> {
    let book = book_of(record)?;
    Ok(serde_json::json!({
        "book_id":book.book_id,
        "version":record.version,
        "recorded_at":record.recorded_at,
        "effective_from":book.effective.from,
        "effective_until":book.effective.until,
        "owning_system":book.owning_system,
        "name":book.name,
        "position_count":book.positions.len(),
    }))
}

// =============================================================================
// SNAPSHOT INGEST
// =============================================================================

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BookSnapshotRequest {
    owning_system: Option<SourceId>,
    name: Option<String>,
    currency: Option<String>,
    description: Option<String>,
    positions: Vec<RowRequest>,
}

/// A row names its instrument by internal ID or by an external identifier, never both.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RowRequest {
    instrument_id: Option<String>,
    identifier: Option<ExternalIdentifier>,
    quantity: ExactDecimal,
    unit: Option<QuantityUnit>,
    reference_version: Option<Version>,
    sector: Option<String>,
    rating: Option<String>,
}

impl RowRequest {
    /// The unit the row states, if it states one.
    pub(crate) fn unit(&self) -> Option<QuantityUnit> {
        self.unit
    }
}

pub(crate) struct Resolved {
    pub(crate) position: BookPosition,
    instrument_version: Version,
    mapping_version: Option<Version>,
}

type RowResult = Result<Result<Resolved, RowError>, ReferenceError>;

fn reject(index: usize, code: &'static str, reason: &'static str) -> RowResult {
    Ok(Err(RowError {
        index,
        code,
        reason,
    }))
}

/// Row-level repository outcomes become rejected rows; infrastructure failures abort.
fn classify(error: ReferenceError) -> Result<(&'static str, &'static str), ReferenceError> {
    match error {
        ReferenceError::InvalidInput(reason) | ReferenceError::Conflict(reason) => {
            Ok(("INVALID_ROW", reason))
        }
        ReferenceError::Unsupported(reason) => Ok(("UNSUPPORTED_INSTRUMENT", reason)),
        ReferenceError::AmbiguousIdentifier => Ok((
            "AMBIGUOUS_IDENTIFIER",
            "identifier maps to more than one active instrument",
        )),
        ReferenceError::NotFound => Ok(("UNKNOWN_IDENTIFIER", "identifier target is missing")),
        other => Err(other),
    }
}

/// Resolve one external identifier through the mapping history, never guessing.
async fn resolve_identifier(
    repo: &dyn ReferenceRepository,
    identifier: &ExternalIdentifier,
    lookup: ReferenceLookup,
) -> Result<Result<(InstrumentId, Version), (&'static str, &'static str)>, ReferenceError> {
    let resolution = match repo.resolve_at(identifier, lookup).await {
        Ok(Some(resolution)) => resolution,
        Ok(None) => {
            return Ok(Err((
                "UNKNOWN_IDENTIFIER",
                "no active mapping for identifier",
            )))
        }
        Err(error) => return classify(error).map(Err),
    };
    match resolution.target {
        ReferenceTarget::Instrument { instrument_id } => {
            Ok(Ok((instrument_id, resolution.mapping.version)))
        }
        _ => Ok(Err((
            "NOT_AN_INSTRUMENT",
            "identifier does not map to a bond instrument",
        ))),
    }
}

async fn resolve_row(
    repo: &dyn ReferenceRepository,
    lookup: ReferenceLookup,
    index: usize,
    row: RowRequest,
) -> RowResult {
    let (id, mapping_version, source_identifier) = match (row.instrument_id, row.identifier) {
        (Some(raw), None) => match InstrumentId::new(raw) {
            Ok(id) => (id, None, None),
            Err(_) => return reject(index, "INVALID_ROW", "invalid instrument identity"),
        },
        (None, Some(identifier)) => match resolve_identifier(repo, &identifier, lookup).await? {
            Ok((id, mapping)) => (id, Some(mapping), Some(identifier)),
            Err((code, reason)) => return reject(index, code, reason),
        },
        _ => {
            return reject(
                index,
                "INVALID_ROW",
                "exactly one of instrument_id or identifier is required",
            )
        }
    };
    let ids_id: convex_core::ids::InstrumentId = id.clone().into();
    let current = match repo.get_at(&ids_id, lookup).await {
        Ok(Some(record)) if record.bond.is_some() => record,
        Ok(_) => return reject(index, "UNKNOWN_INSTRUMENT", "no instrument at lookup time"),
        Err(error) => {
            let (code, reason) = classify(error)?;
            return reject(index, code, reason);
        }
    };
    if let Some(pin) = row.reference_version {
        let pinned = repo.get_version(&ids_id, pin).await?;
        if !pinned.is_some_and(|r| r.bond.is_some() && r.recorded_at <= lookup.known_at) {
            return reject(index, "UNKNOWN_REFERENCE_VERSION", "pinned version unknown");
        }
    }
    let position = BookPosition {
        instrument_id: id,
        quantity: row.quantity,
        unit: row.unit.unwrap_or(QuantityUnit::Face),
        reference_version: row.reference_version,
        source_identifier,
        sector: row.sector,
        rating: row.rating,
    };
    if let Err(ReferenceError::InvalidInput(reason)) = position.validate() {
        return reject(index, "INVALID_ROW", reason);
    }
    Ok(Ok(Resolved {
        position,
        instrument_version: current.version,
        mapping_version,
    }))
}

/// Resolve every row, collecting per-row errors; stops after `MAX_ROW_ERRORS`.
pub(crate) async fn resolve_rows(
    repo: &dyn ReferenceRepository,
    lookup: ReferenceLookup,
    rows: Vec<RowRequest>,
) -> Result<Result<Vec<Resolved>, (Vec<RowError>, bool)>, ReferenceError> {
    let mut resolved = Vec::with_capacity(rows.len());
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, row) in rows.into_iter().enumerate() {
        if errors.len() >= MAX_ROW_ERRORS {
            return Ok(Err((errors, true)));
        }
        match resolve_row(repo, lookup, index, row).await? {
            Ok(row) if seen.insert(row.position.instrument_id.clone()) => resolved.push(row),
            Ok(_) => errors.push(RowError {
                index,
                code: "DUPLICATE_INSTRUMENT",
                reason: "instrument already appears in this snapshot",
            }),
            Err(error) => errors.push(error),
        }
    }
    if errors.is_empty() {
        Ok(Ok(resolved))
    } else {
        Ok(Err((errors, false)))
    }
}

/// Instruments reached only through an external identifier were not visible to the
/// request-body entitlement scan, so they are checked here, before anything commits.
fn authorize_resolved(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    action: Action,
    rows: &[Resolved],
) -> Result<(), Response> {
    authorize_positions(
        state,
        ctx,
        action,
        rows.iter()
            .filter(|r| r.position.source_identifier.is_some())
            .map(|r| &r.position),
    )
}

/// The caller must hold `action` on each position's instrument.
fn authorize_positions<'a>(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    action: Action,
    positions: impl IntoIterator<Item = &'a BookPosition>,
) -> Result<(), Response> {
    let resources: Vec<Resource> = positions
        .into_iter()
        .map(|p| Resource::new(ResourceKind::Instrument, p.instrument_id.as_str()))
        .collect();
    if resources.is_empty() {
        return Ok(());
    }
    let Some(Extension(ctx)) = ctx else {
        return Err(crate::access::http::response(
            crate::access::AccessError::Forbidden,
        ));
    };
    state
        .access
        .authorize(ctx, action, &[DataScope::Books], &resources)
        .map_err(crate::access::http::response)
}

fn owning_system(requested: Option<SourceId>, base: &Base) -> Result<SourceId, ReferenceError> {
    let previous = match &base.record {
        Some(record) => Some(book_of(record)?.owning_system.clone()),
        None => None,
    };
    match (requested, previous) {
        (Some(new), Some(old)) if new != old => {
            Err(ReferenceError::Conflict("book owning system cannot change"))
        }
        (Some(system), _) | (None, Some(system)) => Ok(system),
        (None, None) => {
            SourceId::new(UNATTRIBUTED).map_err(|_| ReferenceError::InvalidInput("owning system"))
        }
    }
}

/// `PUT /api/v1/books/{book_id}`: replace the full position snapshot atomically.
///
/// Absent `If-Match` creates the book and conflicts if the key was ever used; present
/// `If-Match` replaces exactly that version. `Idempotency-Key` is required.
pub(crate) async fn put_book(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<BookSnapshotRequest>, JsonRejection>,
) -> Response {
    let request = match payload {
        Ok(Json(request)) => request,
        Err(_) => {
            return reference_error(ReferenceError::InvalidInput("invalid book snapshot JSON"))
        }
    };
    if request.positions.len() > MAX_BOOK_POSITIONS {
        return too_large();
    }
    let (id, idempotency_key, expected) = match (book_id(raw), key(&headers)) {
        (Ok(id), Ok(idempotency_key)) => match optional_expected(&headers) {
            Ok(expected) => (id, idempotency_key, expected),
            Err(error) => return reference_error(error),
        },
        (Err(error), _) | (_, Err(error)) => return reference_error(error),
    };
    match ingest(&state, &ctx, id, idempotency_key, expected, request).await {
        Ok((record, resolved)) => ingest_response(&record, &resolved),
        Err(response) => response,
    }
}

async fn ingest(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    id: BookId,
    idempotency_key: RequestId,
    expected: Option<Version>,
    request: BookSnapshotRequest,
) -> Result<(MasterRecord, Vec<Resolved>), Response> {
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let base = base_for(repo, &idempotency_key, &id)
        .await
        .map_err(reference_error)?;
    let at = recorded_at(&base).map_err(reference_error)?;
    let lookup = ReferenceLookup {
        as_of: at,
        known_at: at,
    };
    let resolved = match resolve_rows(repo, lookup, request.positions).await {
        Ok(Ok(resolved)) => resolved,
        Ok(Err((errors, truncated))) => {
            return Err(rows_rejected(
                "snapshot rejected; nothing was committed",
                &errors,
                truncated,
            ))
        }
        Err(error) => return Err(reference_error(error)),
    };
    authorize_resolved(state, ctx, Action::BookWrite, &resolved)?;
    let system = owning_system(request.owning_system, &base).map_err(reference_error)?;
    let definition = BookDefinition {
        book_id: id,
        provenance: evidence(&idempotency_key, &system),
        owning_system: system,
        name: request.name,
        currency: request.currency,
        description: request.description,
        effective: EffectiveInterval {
            from: at,
            until: None,
        },
        positions: resolved.iter().map(|r| r.position.clone()).collect(),
    };
    let record = commit_book(state, idempotency_key, expected, definition, at)
        .await
        .map_err(reference_error)?;
    Ok((record, resolved))
}

fn ingest_response(record: &MasterRecord, resolved: &[Resolved]) -> Response {
    let status = if record.version.get() == 1 {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    // Consumed instrument/mapping versions are evidence of this explicit resolution
    // step. They are returned, not stored: the snapshot keeps only what the caller pinned.
    let consumed: Vec<_> = resolved
        .iter()
        .enumerate()
        .map(|(index, r)| {
            serde_json::json!({
                "index":index,
                "instrument_id":r.position.instrument_id,
                "instrument_version":r.instrument_version,
                "mapping_version":r.mapping_version,
            })
        })
        .collect();
    let mut body = match book_view_value(record) {
        Ok(body) => body,
        Err(error) => return reference_error(error),
    };
    if let Some(object) = body.as_object_mut() {
        object.insert("resolutions".into(), serde_json::Value::Array(consumed));
    }
    with_reference_version((status, Json(body)).into_response(), record.version)
}

/// `POST /api/v1/books/{book_id}/retire`: close the current snapshot's interval at the
/// commit time, keeping its positions. The book stays readable with its history.
/// Analytics, the limit status and check, and limit writes refuse it with
/// `409 BOOK_RETIRED`, the limit monitor stops watching it, and a compile after the
/// retirement does not find it, until a new snapshot revives it. `Idempotency-Key` and
/// `If-Match` are required; the reply is `204` with the new version.
pub(crate) async fn retire_book(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
) -> Response {
    match retire(&state, &ctx, raw, &headers).await {
        Ok(response) | Err(response) => response,
    }
}

async fn retire(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    raw: String,
    headers: &HeaderMap,
) -> Result<Response, Response> {
    let id = book_id(raw).map_err(reference_error)?;
    let idempotency_key = key(headers).map_err(reference_error)?;
    let expected = expected_version(headers).map_err(reference_error)?;
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    // Limit writes refuse a retired book, so a retirement does not land between one's
    // check and its commit.
    let _writes = state.limit_writes.lock().await;
    // On an exact retry the base is the pre-image, so the retry rebuilds the same command
    // and gets its receipt rather than `BOOK_RETIRED`.
    let base = base_for(repo, &idempotency_key, &id)
        .await
        .map_err(reference_error)?;
    let replay_at = base.replay_at;
    let record = in_force(base.record)?;
    let mut definition = book_of(&record).map_err(reference_error)?.clone();
    authorize_positions(state, ctx, Action::BookWrite, &definition.positions)?;
    // The interval's end follows its start, however the clock moved since the snapshot.
    let at = match replay_at {
        Some(at) => at,
        None => after(definition.effective.from)
            .and_then(|start| commit_time([start]))
            .map_err(reference_error)?,
    };
    definition.effective.until = Some(at);
    definition.provenance = evidence(&idempotency_key, &definition.owning_system);
    let committed = commit_book(state, idempotency_key, Some(expected), definition, at)
        .await
        .map_err(reference_error)?;
    Ok(with_reference_version(
        StatusCode::NO_CONTENT.into_response(),
        committed.version,
    ))
}

// =============================================================================
// READS
// =============================================================================

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BookListQuery {
    #[serde(default = "default_page")]
    limit: usize,
    after: Option<String>,
}

fn default_page() -> usize {
    100
}

/// `GET /api/v1/books`: summaries ordered by book ID, resumed with `after`.
pub(crate) async fn list_books(
    State(state): State<Arc<AppState>>,
    query: Result<Query<BookListQuery>, QueryRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) => query,
        Err(_) => return reference_error(ReferenceError::InvalidInput("invalid book query")),
    };
    if query.limit == 0 || query.limit > MAX_BOOK_PAGE {
        return reference_error(ReferenceError::InvalidInput("limit must be 1..=500"));
    }
    let after = match query.after.map(book_id).transpose() {
        Ok(after) => after,
        Err(error) => return reference_error(error),
    };
    let repo = state.engine.reference_repository();
    let mut records = match repo.list_books(after.as_ref(), query.limit + 1).await {
        Ok(records) => records,
        Err(error) => return reference_error(error),
    };
    let more = records.len() > query.limit;
    records.truncate(query.limit);
    let items: Result<Vec<_>, _> = records.iter().map(summary).collect();
    let items = match items {
        Ok(items) => items,
        Err(error) => return reference_error(error),
    };
    let next_after = if more {
        records
            .last()
            .and_then(|r| book_of(r).ok())
            .map(|b| b.book_id.clone())
    } else {
        None
    };
    Json(serde_json::json!({"books":items, "limit":query.limit, "next_after":next_after}))
        .into_response()
}

/// `GET /api/v1/books/{book_id}`: the current version, pinned in `ETag`.
pub(crate) async fn get_book(
    State(state): State<Arc<AppState>>,
    Path(raw): Path<String>,
) -> Response {
    let id = match book_id(raw) {
        Ok(id) => id,
        Err(error) => return reference_error(error),
    };
    match state
        .engine
        .reference_repository()
        .get_master(&MasterKey::Book(id))
        .await
    {
        Ok(Some(record)) => book_response(&record, StatusCode::OK),
        Ok(None) => reference_error(ReferenceError::NotFound),
        Err(error) => reference_error(error),
    }
}

/// `GET /api/v1/books/{book_id}/versions/{version}`: one immutable version.
pub(crate) async fn get_book_version(
    State(state): State<Arc<AppState>>,
    Path((raw, version)): Path<(String, u64)>,
) -> Response {
    let (id, version) = match (book_id(raw), Version::new(version)) {
        (Ok(id), Ok(version)) => (id, version),
        (Err(error), _) => return reference_error(error),
        (_, Err(_)) => return reference_error(ReferenceError::InvalidInput("invalid version")),
    };
    match state
        .engine
        .reference_repository()
        .get_master_version(&MasterKey::Book(id), version)
        .await
    {
        Ok(Some(record)) => book_response(&record, StatusCode::OK),
        Ok(None) => reference_error(ReferenceError::NotFound),
        Err(error) => reference_error(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HistoryQuery {
    #[serde(default = "first_version")]
    from: u64,
    #[serde(default = "default_page")]
    limit: usize,
}

fn first_version() -> u64 {
    1
}

/// `GET /api/v1/books/{book_id}/history`: version summaries from `from`, oldest first.
pub(crate) async fn get_book_history(
    State(state): State<Arc<AppState>>,
    Path(raw): Path<String>,
    query: Result<Query<HistoryQuery>, QueryRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) if query.limit > 0 && query.limit <= MAX_BOOK_PAGE => query,
        _ => return reference_error(ReferenceError::InvalidInput("invalid history query")),
    };
    let (id, from) = match (book_id(raw), Version::new(query.from)) {
        (Ok(id), Ok(from)) => (id, from),
        (Err(error), _) => return reference_error(error),
        (_, Err(_)) => return reference_error(ReferenceError::InvalidInput("invalid version")),
    };
    let master_key = MasterKey::Book(id);
    let repo = state.engine.reference_repository();
    let current = match repo.get_master(&master_key).await {
        Ok(Some(record)) => record.version,
        Ok(None) => return reference_error(ReferenceError::NotFound),
        Err(error) => return reference_error(error),
    };
    let last = from
        .get()
        .saturating_add(query.limit as u64 - 1)
        .min(current.get());
    let mut versions = Vec::new();
    for number in from.get()..=last {
        let version = match Version::new(number) {
            Ok(version) => version,
            Err(_) => return reference_error(ReferenceError::Unavailable),
        };
        let record = match repo.get_master_version(&master_key, version).await {
            Ok(Some(record)) => record,
            Ok(None) => return reference_error(ReferenceError::Unavailable),
            Err(error) => return reference_error(error),
        };
        match summary(&record) {
            Ok(item) => versions.push(item),
            Err(error) => return reference_error(error),
        }
    }
    let next_from = (last < current.get()).then_some(last + 1);
    with_consumed_reference_version(
        Json(serde_json::json!({
            "versions":versions,
            "current_version":current,
            "next_from":next_from,
        }))
        .into_response(),
        current,
    )
}

// =============================================================================
// ANALYTICS BY BOOK
// =============================================================================

/// Why a by-book analytics request was refused as a whole.
pub(crate) type BookRefusal = (StatusCode, Json<serde_json::Value>);

fn refusal(status: StatusCode, code: &'static str, detail: serde_json::Value) -> BookRefusal {
    (
        status,
        Json(serde_json::json!({"code":code, "retryable":false, "detail":detail})),
    )
}

fn refused_reference(error: ReferenceError) -> BookRefusal {
    let (status, code) = match error {
        ReferenceError::InvalidInput(_) => (StatusCode::BAD_REQUEST, "INVALID_INPUT"),
        ReferenceError::NotFound => (StatusCode::NOT_FOUND, "MISSING_REFERENCE"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE"),
    };
    refusal(status, code, serde_json::Value::Null)
}

/// Analytics by book. Pins the book (explicit version, else the current one, which is
/// named in a refusal), then requires a complete runtime result of an accepted quality for
/// every position. All-or-nothing: any gap refuses the whole request and names the
/// instruments that lack a result.
///
/// Results reach the analyzer through the engine's quote adapter, with the runtime plan's
/// settlement date and each priced instrument version's currency. Positions must be face
/// quantities and the book must name its reporting currency: neither is defaulted.
pub(crate) async fn book_analytics(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    book: &str,
    version: Option<u64>,
) -> Result<PortfolioAnalyticsOutput, BookRefusal> {
    let record = pinned_book(state, book, version).await?;
    let definition = book_of(&record).map_err(refused_reference)?;
    let detail = |reason: &str| serde_json::json!({"book_id":book, "book_version":record.version, "reason":reason});
    // A version before the retirement is still valued when asked for by number.
    if definition.is_retired() {
        return Err(refusal(
            StatusCode::CONFLICT,
            "BOOK_RETIRED",
            detail(RETIRED),
        ));
    }
    let published = gate_runtime_results(state, ctx, &record)?;
    let currency = definition
        .currency
        .as_deref()
        .and_then(convex_core::Currency::from_code)
        .ok_or_else(|| {
            refusal(
                StatusCode::UNPROCESSABLE_ENTITY,
                "BOOK_CURRENCY_REQUIRED",
                detail("analytics by book need the book's reporting currency"),
            )
        })?;
    if definition
        .positions
        .iter()
        .any(|p| p.unit != QuantityUnit::Face)
    {
        return Err(refusal(
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNSUPPORTED_QUANTITY_UNIT",
            detail("analytics by book take face quantities; no unit-to-face conversion is defined"),
        ));
    }
    let unavailable = || {
        refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "UNAVAILABLE",
            serde_json::Value::Null,
        )
    };
    let settlement = state
        .engine
        .runtime_settlement_date()
        .ok_or_else(unavailable)?;
    let repo = state.engine.reference_repository();
    let mut quotes = Vec::with_capacity(published.len());
    for valuation in &published {
        let ValuationSubject::InstrumentVersion {
            instrument_id,
            version,
        } = &valuation.result.provenance().subject
        else {
            return Err(unavailable());
        };
        let priced = repo
            .get_version(
                &convex_core::ids::InstrumentId::new(instrument_id.as_str()),
                *version,
            )
            .await
            .map_err(refused_reference)?;
        let bond = priced
            .as_ref()
            .and_then(|r| r.bond.as_ref())
            .ok_or_else(|| refused_reference(ReferenceError::NotFound))?;
        let quote = convex_engine::runtime_bond_quote(valuation, settlement.date(), bond.currency)
            .map_err(|error| {
                refusal(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "BOOK_RESULTS_UNAVAILABLE",
                    detail(&error.to_string()),
                )
            })?;
        quotes.push(quote);
    }
    let portfolio = convex_engine::Portfolio {
        portfolio_id: convex_core::ids::PortfolioId::new(book),
        name: definition.name.clone().unwrap_or_else(|| book.to_owned()),
        currency,
        positions: definition
            .positions
            .iter()
            .map(|p| convex_engine::Position {
                instrument_id: convex_core::ids::InstrumentId::new(p.instrument_id.as_str()),
                notional: p.quantity.decimal(),
                sector: p.sector.clone(),
                rating: p.rating.clone(),
            })
            .collect(),
    };
    state
        .engine
        .portfolio_analyzer()
        .calculate(&portfolio, &quotes)
        .map_err(|error| {
            refusal(
                StatusCode::UNPROCESSABLE_ENTITY,
                "BOOK_ANALYTICS_REFUSED",
                detail(&error.to_string()),
            )
        })
}

async fn pinned_book(
    state: &AppState,
    book: &str,
    version: Option<u64>,
) -> Result<MasterRecord, BookRefusal> {
    let id = book_id(book.to_owned()).map_err(refused_reference)?;
    let master_key = MasterKey::Book(id);
    let repo = state.engine.reference_repository();
    let found = match version {
        Some(v) => {
            let version = Version::new(v).map_err(|_| {
                refused_reference(ReferenceError::InvalidInput("version must be positive"))
            })?;
            repo.get_master_version(&master_key, version).await
        }
        None => repo.get_master(&master_key).await,
    };
    found
        .map_err(refused_reference)?
        .ok_or_else(|| refused_reference(ReferenceError::NotFound))
}

/// The accepted runtime result for each position, in book order.
fn gate_runtime_results(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    record: &MasterRecord,
) -> Result<Vec<Arc<PublishedValuation>>, BookRefusal> {
    let book = book_of(record).map_err(refused_reference)?;
    let resources: Vec<Resource> = book
        .positions
        .iter()
        .map(|p| Resource::new(ResourceKind::Instrument, p.instrument_id.as_str()))
        .collect();
    // The refusal names instruments, so the caller must be entitled to each of them.
    let forbidden = || refusal(StatusCode::FORBIDDEN, "FORBIDDEN", serde_json::Value::Null);
    let Some(Extension(ctx)) = ctx else {
        return Err(forbidden());
    };
    state
        .access
        .authorize(ctx, Action::BookRead, &[DataScope::Books], &resources)
        .map_err(|_| forbidden())?;
    let mut accepted = accepted_runtime_results(state, book);
    let mut found = Vec::with_capacity(book.positions.len());
    let mut missing = Vec::new();
    for p in &book.positions {
        let result = accepted
            .get_mut(p.instrument_id.as_str())
            .and_then(|versions| match p.reference_version {
                Some(pin) => versions.remove(&pin.get()),
                None => versions.pop_last().map(|(_, result)| result),
            });
        match result {
            Some(result) => found.push(result),
            None => missing.push(p.instrument_id.as_str()),
        }
    }
    if missing.is_empty() {
        return Ok(found);
    }
    Err(refusal(
        StatusCode::UNPROCESSABLE_ENTITY,
        "BOOK_RESULTS_UNAVAILABLE",
        serde_json::json!({
            "book_id":book.book_id,
            "book_version":record.version,
            "positions":book.positions.len(),
            "missing_count":missing.len(),
            "missing_instruments":missing.iter().take(MAX_ROW_ERRORS).collect::<Vec<_>>(),
        }),
    ))
}

/// The complete runtime results of a book's instruments, by instrument and priced version.
/// `Valid` is always accepted; `Demo` only in an explicit demonstration composition, the
/// switch that also lets the runtime value and serve Demo inputs.
pub(crate) fn accepted_runtime_results(state: &AppState, book: &BookDefinition) -> Accepted {
    accepted_results(
        &state.engine,
        book.positions.iter().map(|p| p.instrument_id.as_str()),
    )
}

/// [`accepted_runtime_results`] of an engine's listed instruments, for readers outside a
/// request. The results are shared and read from one table, so none is from a later tick
/// than another.
pub(crate) fn accepted_results<'a>(
    engine: &convex_engine::PricingEngine,
    instruments: impl IntoIterator<Item = &'a str>,
) -> Accepted {
    let demo = engine
        .configuration_use_cases()
        .is_some_and(|cases| cases.accepts(DataQuality::Demo));
    let mut accepted = Accepted::new();
    for published in engine.runtime_results_of(instruments) {
        let envelope = &published.result;
        let quality = envelope.quality();
        if envelope.status() != ResultStatus::Complete
            || !(quality == DataQuality::Valid || (demo && quality == DataQuality::Demo))
        {
            continue;
        }
        if let ValuationSubject::InstrumentVersion {
            instrument_id,
            version,
        } = &envelope.provenance().subject
        {
            let (id, version) = (instrument_id.as_str().to_owned(), version.get());
            accepted.entry(id).or_default().insert(version, published);
        }
    }
    accepted
}
