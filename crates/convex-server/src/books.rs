//! Durable position books and the portfolio alias over them.
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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use convex_core::contracts::{
    ExactDecimal, InstrumentId, RequestId, SourceId, UtcTimestamp, Version,
};
use convex_engine::ports::reference_master::*;
use convex_engine::ports::reference_repository::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::access::{AccessContext, Action, DataScope, Resource, ResourceKind};
use crate::handlers::AppState;
use crate::reference::{
    expected_version, key, now, reference_error, with_consumed_reference_version,
    with_reference_version,
};

/// Rejected rows reported in one response; the request stops resolving after this many.
const MAX_ROW_ERRORS: usize = 100;
/// Largest page of `GET /api/v1/books` and of book history.
const MAX_BOOK_PAGE: usize = 500;
/// Owning system recorded when a snapshot does not name one.
const UNATTRIBUTED: &str = "unattributed";
/// Owning system recorded for books written through the portfolio alias.
const PORTFOLIO_API: &str = "portfolio-api";
const DEFAULT_CURRENCY: &str = "USD";

// =============================================================================
// ERRORS
// =============================================================================

fn typed_error(status: StatusCode, code: &'static str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({"error":message, "code":code, "retryable":false})),
    )
        .into_response()
}

fn too_large() -> Response {
    typed_error(
        StatusCode::PAYLOAD_TOO_LARGE,
        "BOOK_TOO_LARGE",
        "book exceeds the position bound",
    )
}

struct RowError {
    index: usize,
    code: &'static str,
    reason: &'static str,
}

fn rows_rejected(errors: &[RowError], truncated: bool) -> Response {
    let rows: Vec<_> = errors
        .iter()
        .map(|e| serde_json::json!({"index":e.index, "code":e.code, "reason":e.reason}))
        .collect();
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(serde_json::json!({
            "error":"snapshot rejected; nothing was committed",
            "code":"BOOK_ROWS_REJECTED",
            "retryable":false,
            "rows":rows,
            "truncated":truncated,
        })),
    )
        .into_response()
}

fn book_id(raw: String) -> Result<BookId, ReferenceError> {
    BookId::new(raw).map_err(|_| ReferenceError::InvalidInput("invalid book identity"))
}

fn book_of(record: &MasterRecord) -> Result<&BookDefinition, ReferenceError> {
    match &record.definition {
        MasterDefinition::Book(book) => Ok(book),
        _ => Err(ReferenceError::Unsupported("not a book resource")),
    }
}

/// If-Match is optional on a snapshot: absent means create-only.
fn optional_expected(headers: &HeaderMap) -> Result<Option<Version>, ReferenceError> {
    if headers.get_all("if-match").iter().next().is_none() {
        return Ok(None);
    }
    expected_version(headers).map(Some)
}

fn is_live(book: &BookDefinition, at: UtcTimestamp) -> bool {
    book.effective.until.is_none_or(|end| end > at)
}

// =============================================================================
// SHARED WRITE PATH
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
struct RowRequest {
    instrument_id: Option<String>,
    identifier: Option<ExternalIdentifier>,
    quantity: ExactDecimal,
    unit: Option<QuantityUnit>,
    reference_version: Option<Version>,
    sector: Option<String>,
    rating: Option<String>,
}

struct Resolved {
    position: BookPosition,
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
async fn resolve_rows(
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
    let resources: Vec<Resource> = rows
        .iter()
        .filter(|r| r.position.source_identifier.is_some())
        .map(|r| Resource::new(ResourceKind::Instrument, r.position.instrument_id.as_str()))
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
        Ok(Err((errors, truncated))) => return Err(rows_rejected(&errors, truncated)),
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
// PORTFOLIO ALIAS (one release): same shapes as the former in-memory store
// =============================================================================

/// Legacy position shape. `unit` is additive and only shown for non-face quantities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AliasPosition {
    instrument_id: String,
    notional: Decimal,
    sector: Option<String>,
    rating: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unit: Option<QuantityUnit>,
}

#[derive(Serialize)]
struct PortfolioView {
    portfolio_id: String,
    name: String,
    currency: String,
    description: Option<String>,
    positions: Vec<AliasPosition>,
    created_at: i64,
    updated_at: i64,
}

fn millis(time: UtcTimestamp) -> i64 {
    time.datetime().timestamp_millis()
}

fn alias_position(position: &BookPosition) -> AliasPosition {
    AliasPosition {
        instrument_id: position.instrument_id.as_str().to_owned(),
        notional: position.quantity.decimal(),
        sector: position.sector.clone(),
        rating: position.rating.clone(),
        unit: (position.unit != QuantityUnit::Face).then_some(position.unit),
    }
}

fn to_position(position: AliasPosition) -> Result<BookPosition, ReferenceError> {
    Ok(BookPosition {
        instrument_id: InstrumentId::new(position.instrument_id)
            .map_err(|_| ReferenceError::InvalidInput("invalid instrument identity"))?,
        quantity: ExactDecimal::new(position.notional),
        unit: position.unit.unwrap_or(QuantityUnit::Face),
        reference_version: None,
        source_identifier: None,
        sector: position.sector,
        rating: position.rating,
    })
}

fn to_positions(positions: Vec<AliasPosition>) -> Result<Vec<BookPosition>, ReferenceError> {
    if positions.len() > MAX_BOOK_POSITIONS {
        return Err(ReferenceError::CapacityExceeded);
    }
    positions.into_iter().map(to_position).collect()
}

fn portfolio_view(
    record: &MasterRecord,
    created: UtcTimestamp,
) -> Result<PortfolioView, ReferenceError> {
    let book = book_of(record)?;
    Ok(PortfolioView {
        portfolio_id: book.book_id.as_str().to_owned(),
        name: book
            .name
            .clone()
            .unwrap_or_else(|| book.book_id.as_str().to_owned()),
        currency: book
            .currency
            .clone()
            .unwrap_or_else(|| DEFAULT_CURRENCY.to_owned()),
        description: book.description.clone(),
        positions: book.positions.iter().map(alias_position).collect(),
        created_at: millis(created),
        updated_at: millis(record.recorded_at),
    })
}

async fn created_at(
    repo: &dyn ReferenceRepository,
    record: &MasterRecord,
) -> Result<UtcTimestamp, ReferenceError> {
    if record.version.get() == 1 {
        return Ok(record.recorded_at);
    }
    let first = Version::new(1).map_err(|_| ReferenceError::Unavailable)?;
    let key = record.definition.key();
    let found = repo.get_master_version(&key, first).await?;
    Ok(found.ok_or(ReferenceError::Unavailable)?.recorded_at)
}

async fn portfolio_response(
    state: &AppState,
    record: &MasterRecord,
    status: StatusCode,
) -> Response {
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let created = match created_at(repo, record).await {
        Ok(created) => created,
        Err(error) => return reference_error(error),
    };
    match portfolio_view(record, created) {
        Ok(view) => with_reference_version((status, Json(view)).into_response(), record.version),
        Err(error) => reference_error(error),
    }
}

fn portfolio_not_found(id: &str) -> Response {
    typed_error(
        StatusCode::NOT_FOUND,
        "MISSING_REFERENCE",
        &format!("Portfolio not found: {id}"),
    )
}

/// The live base book of an alias write, or the response to return instead.
async fn live_base(
    state: &AppState,
    id: &BookId,
    idempotency_key: &RequestId,
) -> Result<(Base, MasterRecord), Response> {
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let base = base_for(repo, idempotency_key, id)
        .await
        .map_err(reference_error)?;
    let at = recorded_at(&base).map_err(reference_error)?;
    match &base.record {
        Some(record) if book_of(record).is_ok_and(|b| is_live(b, at)) => {
            let record = record.clone();
            Ok((base, record))
        }
        _ => Err(portfolio_not_found(id.as_str())),
    }
}

/// Read-modify-write over the live base. The expected version is the caller's
/// `If-Match`, else the version just read, so a concurrent writer yields 409.
async fn edit_portfolio<F>(state: &AppState, id: BookId, headers: &HeaderMap, edit: F) -> Response
where
    F: FnOnce(&mut BookDefinition) -> Result<bool, Response>,
{
    let idempotency_key = match key(headers) {
        Ok(k) => k,
        Err(error) => return reference_error(error),
    };
    let if_match = match optional_expected(headers) {
        Ok(v) => v,
        Err(error) => return reference_error(error),
    };
    let (base, record) = match live_base(state, &id, &idempotency_key).await {
        Ok(found) => found,
        Err(response) => return response,
    };
    let at = match recorded_at(&base) {
        Ok(at) => at,
        Err(error) => return reference_error(error),
    };
    let mut definition = match book_of(&record) {
        Ok(book) => book.clone(),
        Err(error) => return reference_error(error),
    };
    match edit(&mut definition) {
        Ok(true) => {}
        Ok(false) => return portfolio_response(state, &record, StatusCode::OK).await,
        Err(response) => return response,
    }
    definition.provenance = evidence(&idempotency_key, &definition.owning_system);
    let expected = if_match.unwrap_or(record.version);
    match commit_book(state, idempotency_key, Some(expected), definition, at).await {
        Ok(next) => portfolio_response(state, &next, StatusCode::OK).await,
        Err(error) => reference_error(error),
    }
}

fn open_interval(at: UtcTimestamp) -> EffectiveInterval {
    EffectiveInterval {
        from: at,
        until: None,
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct PortfolioListQuery {
    #[serde(default = "default_page")]
    limit: usize,
    #[serde(default)]
    offset: usize,
    currency: Option<String>,
    q: Option<String>,
}

fn matches_filter(book: &BookDefinition, query: &PortfolioListQuery) -> bool {
    let currency = book.currency.as_deref().unwrap_or(DEFAULT_CURRENCY);
    if query.currency.as_deref().is_some_and(|c| c != currency) {
        return false;
    }
    let Some(text) = &query.q else {
        return true;
    };
    let needle = text.to_lowercase();
    let name = book.name.as_deref().unwrap_or(book.book_id.as_str());
    name.to_lowercase().contains(&needle)
        || book.book_id.as_str().to_lowercase().contains(&needle)
        || book
            .description
            .as_ref()
            .is_some_and(|d| d.to_lowercase().contains(&needle))
}

/// `GET /api/v1/portfolios`: live books, filtered and paged. The alias reads at most one
/// repository page; a larger estate must use `/api/v1/books`.
pub(crate) async fn list_portfolios(
    State(state): State<Arc<AppState>>,
    query: Result<Query<PortfolioListQuery>, QueryRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) => query,
        Err(_) => return reference_error(ReferenceError::InvalidInput("invalid portfolio query")),
    };
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let at = match now() {
        Ok(at) => at,
        Err(error) => return reference_error(error),
    };
    let records = match repo.list_books(None, MAX_REFERENCE_PAGE).await {
        Ok(records) => records,
        Err(error) => return reference_error(error),
    };
    if records.len() == MAX_REFERENCE_PAGE {
        let last = records
            .last()
            .and_then(|r| book_of(r).ok())
            .map(|b| b.book_id.clone());
        match repo.list_books(last.as_ref(), 1).await {
            Ok(rest) if rest.is_empty() => {}
            Ok(_) => return reference_error(ReferenceError::CapacityExceeded),
            Err(error) => return reference_error(error),
        }
    }
    let limit = query.limit.min(MAX_REFERENCE_PAGE);
    let live: Vec<&MasterRecord> = records
        .iter()
        .filter(|r| book_of(r).is_ok_and(|b| is_live(b, at) && matches_filter(b, &query)))
        .collect();
    let mut portfolios = Vec::new();
    for record in live.iter().skip(query.offset).take(limit) {
        let created = match created_at(repo, record).await {
            Ok(created) => created,
            Err(error) => return reference_error(error),
        };
        match portfolio_view(record, created) {
            Ok(view) => portfolios.push(view),
            Err(error) => return reference_error(error),
        }
    }
    Json(serde_json::json!({
        "portfolios":portfolios,
        "total":live.len(),
        "limit":limit,
        "offset":query.offset,
    }))
    .into_response()
}

/// `GET /api/v1/portfolios/{id}`.
pub(crate) async fn get_portfolio(
    State(state): State<Arc<AppState>>,
    Path(raw): Path<String>,
) -> Response {
    let id = match book_id(raw.clone()) {
        Ok(id) => id,
        Err(_) => return portfolio_not_found(&raw),
    };
    let at = match now() {
        Ok(at) => at,
        Err(error) => return reference_error(error),
    };
    match state
        .engine
        .reference_repository()
        .get_master(&MasterKey::Book(id))
        .await
    {
        Ok(Some(record)) if book_of(&record).is_ok_and(|b| is_live(b, at)) => {
            portfolio_response(&state, &record, StatusCode::OK).await
        }
        Ok(_) => portfolio_not_found(&raw),
        Err(error) => reference_error(error),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CreatePortfolioRequest {
    portfolio_id: String,
    name: String,
    #[serde(default = "default_currency")]
    currency: String,
    description: Option<String>,
    #[serde(default)]
    positions: Vec<AliasPosition>,
}

fn default_currency() -> String {
    DEFAULT_CURRENCY.to_owned()
}

fn new_definition(
    request: CreatePortfolioRequest,
    idempotency_key: &RequestId,
    at: UtcTimestamp,
) -> Result<BookDefinition, ReferenceError> {
    let system =
        SourceId::new(PORTFOLIO_API).map_err(|_| ReferenceError::InvalidInput("owning system"))?;
    Ok(BookDefinition {
        book_id: book_id(request.portfolio_id)?,
        provenance: evidence(idempotency_key, &system),
        owning_system: system,
        name: Some(request.name),
        currency: Some(request.currency),
        description: request.description,
        effective: open_interval(at),
        positions: to_positions(request.positions)?,
    })
}

/// `POST /api/v1/portfolios`: create; an existing live portfolio is a 409.
pub(crate) async fn create_portfolio(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    payload: Result<Json<CreatePortfolioRequest>, JsonRejection>,
) -> Response {
    let request = match payload {
        Ok(Json(request)) => request,
        Err(_) => return reference_error(ReferenceError::InvalidInput("invalid portfolio JSON")),
    };
    let (id, idempotency_key) = match (book_id(request.portfolio_id.clone()), key(&headers)) {
        (Ok(id), Ok(k)) => (id, k),
        (Err(error), _) | (_, Err(error)) => return reference_error(error),
    };
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let base = match base_for(repo, &idempotency_key, &id).await {
        Ok(base) => base,
        Err(error) => return reference_error(error),
    };
    let at = match recorded_at(&base) {
        Ok(at) => at,
        Err(error) => return reference_error(error),
    };
    // A retired portfolio is revived by the next version; a live one is a conflict.
    let expected = match &base.record {
        Some(record) if book_of(record).is_ok_and(|b| is_live(b, at)) => {
            return typed_error(
                StatusCode::CONFLICT,
                "VERSION_CONFLICT",
                &format!("Portfolio already exists: {}", id.as_str()),
            )
        }
        Some(record) => Some(record.version),
        None => None,
    };
    let definition = match new_definition(request, &idempotency_key, at) {
        Ok(definition) => definition,
        Err(error) => return reference_error(error),
    };
    match commit_book(&state, idempotency_key, expected, definition, at).await {
        Ok(record) => portfolio_response(&state, &record, StatusCode::CREATED).await,
        Err(error) => reference_error(error),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdatePortfolioRequest {
    name: Option<String>,
    currency: Option<String>,
    description: Option<String>,
    positions: Option<Vec<AliasPosition>>,
}

/// `PUT /api/v1/portfolios/{id}`: replace the supplied fields.
pub(crate) async fn update_portfolio(
    State(state): State<Arc<AppState>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<UpdatePortfolioRequest>, JsonRejection>,
) -> Response {
    let request = match payload {
        Ok(Json(request)) => request,
        Err(_) => return reference_error(ReferenceError::InvalidInput("invalid portfolio JSON")),
    };
    let id = match book_id(raw.clone()) {
        Ok(id) => id,
        Err(_) => return portfolio_not_found(&raw),
    };
    let positions = match request.positions.map(to_positions).transpose() {
        Ok(positions) => positions,
        Err(error) => return reference_error(error),
    };
    edit_portfolio(&state, id, &headers, |book| {
        if let Some(name) = request.name {
            book.name = Some(name);
        }
        if let Some(currency) = request.currency {
            book.currency = Some(currency);
        }
        // A null description means "unchanged" (as before books); it cannot clear one.
        if request.description.is_some() {
            book.description = request.description;
        }
        if let Some(positions) = positions {
            book.positions = positions;
        }
        Ok(true)
    })
    .await
}

/// `DELETE /api/v1/portfolios/{id}`: retire the book by closing its effective interval.
pub(crate) async fn delete_portfolio(
    State(state): State<Arc<AppState>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
) -> Response {
    let id = match book_id(raw.clone()) {
        Ok(id) => id,
        Err(_) => return portfolio_not_found(&raw),
    };
    let idempotency_key = match key(&headers) {
        Ok(k) => k,
        Err(error) => return reference_error(error),
    };
    let if_match = match optional_expected(&headers) {
        Ok(v) => v,
        Err(error) => return reference_error(error),
    };
    let (base, record) = match live_base(&state, &id, &idempotency_key).await {
        Ok(found) => found,
        Err(response) => return response,
    };
    let at = match recorded_at(&base) {
        Ok(at) => at,
        Err(error) => return reference_error(error),
    };
    let mut definition = match book_of(&record) {
        Ok(book) => book.clone(),
        Err(error) => return reference_error(error),
    };
    definition.effective.until = Some(at);
    definition.provenance = evidence(&idempotency_key, &definition.owning_system);
    let expected = if_match.unwrap_or(record.version);
    match commit_book(&state, idempotency_key, Some(expected), definition, at).await {
        Ok(next) => with_reference_version(StatusCode::NO_CONTENT.into_response(), next.version),
        Err(error) => reference_error(error),
    }
}

/// `POST /api/v1/portfolios/{id}/positions`.
pub(crate) async fn add_portfolio_position(
    State(state): State<Arc<AppState>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<AliasPosition>, JsonRejection>,
) -> Response {
    let position = match payload
        .map_err(|_| ())
        .and_then(|Json(p)| to_position(p).map_err(|_| ()))
    {
        Ok(position) => position,
        Err(()) => return reference_error(ReferenceError::InvalidInput("invalid position JSON")),
    };
    let id = match book_id(raw.clone()) {
        Ok(id) => id,
        Err(_) => return portfolio_not_found(&raw),
    };
    edit_portfolio(&state, id, &headers, |book| {
        if book.positions.len() >= MAX_BOOK_POSITIONS {
            return Err(too_large());
        }
        book.positions.push(position);
        Ok(true)
    })
    .await
}

/// `DELETE /api/v1/portfolios/{id}/positions/{instrument_id}`.
pub(crate) async fn remove_portfolio_position(
    State(state): State<Arc<AppState>>,
    Path((raw, instrument)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let id = match book_id(raw.clone()) {
        Ok(id) => id,
        Err(_) => return portfolio_not_found(&raw),
    };
    edit_portfolio(&state, id, &headers, |book| {
        let before = book.positions.len();
        book.positions
            .retain(|p| p.instrument_id.as_str() != instrument);
        Ok(book.positions.len() != before)
    })
    .await
}

/// `PUT /api/v1/portfolios/{id}/positions/{instrument_id}`: replace an existing position.
pub(crate) async fn update_portfolio_position(
    State(state): State<Arc<AppState>>,
    Path((raw, instrument)): Path<(String, String)>,
    headers: HeaderMap,
    payload: Result<Json<AliasPosition>, JsonRejection>,
) -> Response {
    let mut position = match payload {
        Ok(Json(position)) => position,
        Err(_) => return reference_error(ReferenceError::InvalidInput("invalid position JSON")),
    };
    position.instrument_id = instrument.clone();
    let position = match to_position(position) {
        Ok(position) => position,
        Err(error) => return reference_error(error),
    };
    let id = match book_id(raw.clone()) {
        Ok(id) => id,
        Err(_) => return portfolio_not_found(&raw),
    };
    edit_portfolio(&state, id, &headers, |book| {
        match book
            .positions
            .iter_mut()
            .find(|p| p.instrument_id.as_str() == instrument)
        {
            Some(existing) => {
                *existing = position;
                Ok(true)
            }
            None => Err(portfolio_not_found(&format!("{raw}/{instrument}"))),
        }
    })
    .await
}

#[derive(Debug, Deserialize)]
pub(crate) struct BatchPortfolioCreateRequest {
    portfolios: Vec<CreatePortfolioRequest>,
}

/// `POST /api/v1/portfolios/batch`: create the missing portfolios in one atomic commit
/// and skip those that already exist. A retry with the same key returns the same counts.
pub(crate) async fn batch_create_portfolios(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    payload: Result<Json<BatchPortfolioCreateRequest>, JsonRejection>,
) -> Response {
    let request = match payload {
        Ok(Json(request)) => request,
        Err(_) => return reference_error(ReferenceError::InvalidInput("invalid portfolio JSON")),
    };
    let idempotency_key = match key(&headers) {
        Ok(k) => k,
        Err(error) => return reference_error(error),
    };
    if request.portfolios.len() > MAX_REFERENCE_CHANGES {
        return typed_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "BATCH_TOO_LARGE",
            "at most 256 portfolios per batch",
        );
    }
    let total = request.portfolios.len();
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let at = match now() {
        Ok(at) => at,
        Err(error) => return reference_error(error),
    };
    match batch_replay(repo, &idempotency_key, &request.portfolios, at).await {
        Ok(Some(created)) => return batch_counts(created, total),
        Ok(None) => {}
        Err(error) => return reference_error(error),
    }
    let changes = match batch_changes(repo, request.portfolios, &idempotency_key, at).await {
        Ok(changes) => changes,
        Err(error) => return reference_error(error),
    };
    let created = changes.len();
    if created == 0 {
        return batch_counts(0, total);
    }
    let command = ReferenceCommand {
        idempotency_key,
        changes: Vec::new(),
        master_changes: changes,
    };
    match state.engine.commit_reference(&command, at).await {
        Ok(_) => batch_counts(created, total),
        Err(error) => reference_error(error),
    }
}

fn batch_counts(created: usize, total: usize) -> Response {
    Json(serde_json::json!({"created":created, "skipped":total.saturating_sub(created)}))
        .into_response()
}

/// A retained receipt for this key must match this request: every retained book is one
/// this request names, created by this operation with the same content, and any other
/// named portfolio already existed.
async fn batch_replay(
    repo: &dyn ReferenceRepository,
    idempotency_key: &RequestId,
    portfolios: &[CreatePortfolioRequest],
    at: UtcTimestamp,
) -> Result<Option<usize>, ReferenceError> {
    let Some(prior) = repo.get_commit(idempotency_key).await? else {
        return Ok(None);
    };
    let different = ReferenceError::Conflict("idempotency key was used with a different payload");
    if !prior.records.is_empty() || prior.master_records.is_empty() {
        return Err(different);
    }
    for record in &prior.master_records {
        let MasterDefinition::Book(book) = &record.definition else {
            return Err(different);
        };
        // `batch_changes` keeps the first occurrence of a repeated id.
        let Some(request) = portfolios
            .iter()
            .find(|p| p.portfolio_id == book.book_id.as_str())
        else {
            return Err(different);
        };
        let expected = new_definition(request.clone(), idempotency_key, at)?;
        let same = book.owning_system == expected.owning_system
            && book.provenance == expected.provenance
            && book.name == expected.name
            && book.currency == expected.currency
            && book.description == expected.description
            && book.positions == expected.positions;
        if !same {
            return Err(different);
        }
    }
    // A named portfolio the receipt did not create must already have been live.
    let created: BTreeSet<&str> = prior
        .master_records
        .iter()
        .filter_map(|r| match &r.definition {
            MasterDefinition::Book(book) => Some(book.book_id.as_str()),
            _ => None,
        })
        .collect();
    for request in portfolios {
        if created.contains(request.portfolio_id.as_str()) {
            continue;
        }
        let id = book_id(request.portfolio_id.clone())?;
        match repo.get_master(&MasterKey::Book(id)).await? {
            Some(record) if book_of(&record).is_ok_and(|b| is_live(b, at)) => {}
            _ => return Err(different),
        }
    }
    Ok(Some(prior.master_records.len()))
}

async fn batch_changes(
    repo: &dyn ReferenceRepository,
    portfolios: Vec<CreatePortfolioRequest>,
    idempotency_key: &RequestId,
    at: UtcTimestamp,
) -> Result<Vec<MasterChange>, ReferenceError> {
    let mut seen = BTreeSet::new();
    let mut changes = Vec::new();
    for request in portfolios {
        let id = book_id(request.portfolio_id.clone())?;
        if !seen.insert(id.clone()) {
            continue;
        }
        let expected = match repo.get_master(&MasterKey::Book(id)).await? {
            Some(record) if book_of(&record).is_ok_and(|b| is_live(b, at)) => continue,
            Some(record) => Some(record.version),
            None => None,
        };
        let definition = new_definition(request, idempotency_key, at)?;
        changes.push(MasterChange {
            expected_version: expected,
            definition: MasterDefinition::Book(Box::new(definition)),
        });
    }
    Ok(changes)
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

/// Refuse analytics by book. Pins the book (explicit version, else the current one, which
/// is named in the refusal), then requires a valid runtime canonical result for every
/// position. All-or-nothing: any gap refuses the whole request and names the instruments
/// that lack a result.
///
/// The runtime universe is a frozen plan today, so on current deployments this normally
/// refuses. When every position has a valid result there is still no defined adapter from
/// canonical valuation results to the portfolio analyzer's inputs; that is refused
/// explicitly (501) rather than approximated.
pub(crate) async fn refuse_book_analytics(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    book: &str,
    version: Option<u64>,
) -> BookRefusal {
    let record = match pinned_book(state, book, version).await {
        Ok(record) => record,
        Err(refused) => return refused,
    };
    match gate_runtime_results(state, ctx, &record) {
        Ok(()) => refusal(
            StatusCode::NOT_IMPLEMENTED,
            "RUNTIME_RESULT_ADAPTER_UNAVAILABLE",
            serde_json::json!({
                "book_id":book,
                "book_version":record.version,
                "reason":"no defined adapter from canonical valuation results to portfolio analytics inputs",
            }),
        ),
        Err(refused) => refused,
    }
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

fn gate_runtime_results(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    record: &MasterRecord,
) -> Result<(), BookRefusal> {
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
    let valid = valid_runtime_instruments(state);
    let missing: Vec<&str> = book
        .positions
        .iter()
        .filter(|p| {
            !valid
                .get(p.instrument_id.as_str())
                .is_some_and(|versions| match p.reference_version {
                    Some(pin) => versions.contains(&pin.get()),
                    None => !versions.is_empty(),
                })
        })
        .map(|p| p.instrument_id.as_str())
        .collect();
    if missing.is_empty() {
        return Ok(());
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

/// Instruments with a complete, valid runtime result, mapped to the versions they priced.
fn valid_runtime_instruments(state: &AppState) -> BTreeMap<String, BTreeSet<u64>> {
    use convex_core::contracts::{DataQuality, ResultStatus, ValuationSubject};
    let mut valid: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
    for published in state.engine.runtime_results() {
        let envelope = &published.result;
        if envelope.status() != ResultStatus::Complete || envelope.quality() != DataQuality::Valid {
            continue;
        }
        if let ValuationSubject::InstrumentVersion {
            instrument_id,
            version,
        } = &envelope.provenance().subject
        {
            valid
                .entry(instrument_id.as_str().to_owned())
                .or_default()
                .insert(version.get());
        }
    }
    valid
}
