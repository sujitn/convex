//! Book limit sets and their status (P14b; D82, D84, D85, D87).
//!
//! A book's limits are one `MasterKey::LimitSet` record per book in the reference
//! repository, as books are: compare-and-set versions, history, idempotency receipts and
//! backup. Under required access a change needs two subjects (D85): one proposes it
//! (`MasterKey::LimitProposal`), and a different one approves it, which commits the set and
//! closes the proposal in one transaction. Under local development requests carry no
//! subject, so a change commits in one step, recorded as `local-development`.
//!
//! Status is evaluated on request against the current Live results by
//! `convex_engine::limits::evaluate_limits`. A position without an accepted result makes
//! the limits that read market data `unknown`; it never refuses the request.
//!
//! A pre-trade check (P14d; D88–D91) evaluates a book with proposed changes applied the
//! same way, and stores nothing.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
// Internal early-return errors are the finished HTTP response itself.
#![allow(clippy::result_large_err)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use convex_core::contracts::{
    ExactDecimal, InstrumentId, RequestId, SourceId, UtcTimestamp, ValuationSubject, Version,
};
use convex_engine::limits::{evaluate_limits, LimitBook, LimitPosition, LimitTerms};
use convex_engine::ports::limits::{
    validate_limits, LimitDefinition, LimitEvaluation, LimitMeasure, LimitMember,
    LimitProposalClosure, LimitProposalDefinition, LimitSetDefinition, UnknownReason,
};
use convex_engine::ports::output::BondQuoteOutput;
use convex_engine::ports::reference_data::BondReferenceData;
use convex_engine::ports::reference_master::*;
use convex_engine::ports::reference_repository::*;
use convex_engine::ports::runtime::PublishedValuation;
use convex_engine::ports::storage::{AuditStore, MAX_ENTITY_PAGE};
use serde::{Deserialize, Serialize};

use crate::access::{recorded_subject, AccessContext, Action, DataScope, Resource, ResourceKind};
use crate::books::{
    accepted_runtime_results, book_id, book_of, optional_expected, resolve_rows, rows_rejected,
    too_large, typed_error, Resolved, RowError, RowRequest, MAX_ROW_ERRORS,
};
use crate::handlers::AppState;
use crate::limit_monitor::{LIMIT_STATUS_ENTITY, LIMIT_STATUS_EVENT};
use crate::reference::{expected_version, key, now, reference_error, with_reference_version};

/// Books with a non-empty limit set: the monitor watches each of them (D87).
pub(crate) const MAX_LIMITED_BOOKS: usize = convex_engine::configuration::MAX_PLAN_BOOKS;
/// Largest page of limit set or proposal history.
const MAX_HISTORY_PAGE: usize = 500;
/// Source recorded on every limit set and proposal.
const LIMITS_API: &str = "limits-api";
/// Rows one pre-trade check takes (D88).
pub(crate) const MAX_CHECK_ROWS: usize = 256;
/// What status and a check read: the book, its results and priced terms, and market values.
const STATUS_DATA: &[DataScope] = &[
    DataScope::Books,
    DataScope::Results,
    DataScope::Reference,
    DataScope::Market,
];

// =============================================================================
// ERRORS AND SHARED CHECKS
// =============================================================================

fn conflict(code: &'static str, message: &str) -> Response {
    typed_error(StatusCode::CONFLICT, code, message)
}

pub(crate) fn set_of(record: &MasterRecord) -> Result<&LimitSetDefinition, ReferenceError> {
    match &record.definition {
        MasterDefinition::LimitSet(set) => Ok(set),
        _ => Err(ReferenceError::Unsupported("not a limit set")),
    }
}

fn proposal_of(record: &MasterRecord) -> Result<&LimitProposalDefinition, ReferenceError> {
    match &record.definition {
        MasterDefinition::LimitProposal(proposal) => Ok(proposal),
        _ => Err(ReferenceError::Unsupported("not a limit proposal")),
    }
}

/// Whether a master change writes a limit set or proposal, which only these routes may.
pub(crate) fn writes_limits(changes: &[MasterChange]) -> bool {
    changes.iter().any(|change| {
        matches!(
            change.definition,
            MasterDefinition::LimitSet(_) | MasterDefinition::LimitProposal(_)
        )
    })
}

/// The refusal of a generic reference write, or commit read, that names a limit set or
/// proposal: D85's two subjects and the limit routes' instrument checks hold only there.
pub(crate) fn limits_route_required() -> Response {
    typed_error(
        StatusCode::FORBIDDEN,
        "LIMITS_ROUTE_REQUIRED",
        "limit sets and proposals are read and written only through /api/v1/books/{book_id}/limits",
    )
}

/// The instruments a limit names, which the response scan does not read.
pub(crate) fn named_instruments(limits: &[LimitDefinition]) -> impl Iterator<Item = &InstrumentId> {
    limits.iter().filter_map(|limit| match &limit.measure {
        LimitMeasure::Face {
            instrument: LimitMember::One(id),
        } => Some(id),
        _ => None,
    })
}

/// A limit set or status names instruments in fields the access layer's response scan
/// does not read, so the caller must hold the action on the book and on each instrument.
fn authorize(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    action: Action,
    data: &[DataScope],
    book: &BookId,
    instruments: impl IntoIterator<Item = String>,
) -> Result<(), Response> {
    let mut seen = BTreeSet::new();
    let mut resources = vec![Resource::new(ResourceKind::Book, book.as_str())];
    for id in instruments {
        if seen.insert(id.clone()) {
            resources.push(Resource::new(ResourceKind::Instrument, id));
        }
    }
    let Some(Extension(ctx)) = ctx else {
        return Err(crate::access::http::response(
            crate::access::AccessError::Forbidden,
        ));
    };
    state
        .access
        .authorize(ctx, action, data, &resources)
        .map_err(crate::access::http::response)
}

/// Reading limits needs `BookRead` on the book, each position's instrument and each
/// instrument a limit names: a status names instruments, and so may a set.
async fn authorize_read(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    data: &[DataScope],
    book: &BookId,
    limits: &[LimitDefinition],
) -> Result<Option<MasterRecord>, Response> {
    let record = state
        .engine
        .reference_repository()
        .get_master(&MasterKey::Book(book.clone()))
        .await
        .map_err(reference_error)?;
    let positions = match &record {
        Some(record) => book_of(record)
            .map_err(reference_error)?
            .positions
            .as_slice(),
        None => &[],
    };
    let instruments = positions
        .iter()
        .map(|p| p.instrument_id.as_str().to_owned())
        .chain(named_instruments(limits).map(|id| id.as_str().to_owned()));
    authorize(state, ctx, Action::BookRead, data, book, instruments)?;
    Ok(record)
}

/// Writing names each instrument the limits name, so the writer must hold each of them.
fn authorize_write(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    book: &BookId,
    limits: &[LimitDefinition],
) -> Result<(), Response> {
    let instruments = named_instruments(limits).map(|id| id.as_str().to_owned());
    authorize(
        state,
        ctx,
        Action::LimitWrite,
        &[DataScope::Books],
        book,
        instruments,
    )
}

/// A change replaces the current set, dropping each limit it leaves out, so the caller
/// must hold each instrument the current set names as well as those the change names.
fn authorize_replaced(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    book: &BookId,
    set: Option<&MasterRecord>,
) -> Result<(), Response> {
    match set {
        Some(record) => authorize_write(
            state,
            ctx,
            book,
            &set_of(record).map_err(reference_error)?.limits,
        ),
        None => Ok(()),
    }
}

/// One microsecond after `time`: a closed proposal's end must follow its start.
fn after(time: UtcTimestamp) -> Result<UtcTimestamp, ReferenceError> {
    UtcTimestamp::from_datetime(time.datetime() + chrono::Duration::microseconds(1))
        .map_err(|_| ReferenceError::Unavailable)
}

/// The commit time: now, but never before a record the commit replaces, so a wall clock
/// that steps back cannot refuse the write.
fn commit_time(
    floor: impl IntoIterator<Item = UtcTimestamp>,
) -> Result<UtcTimestamp, ReferenceError> {
    Ok(floor.into_iter().fold(now()?, UtcTimestamp::max))
}

fn evidence(idempotency_key: &RequestId) -> Result<ReferenceProvenance, ReferenceError> {
    Ok(ReferenceProvenance {
        source: SourceId::new(LIMITS_API)
            .map_err(|_| ReferenceError::InvalidInput("limits source"))?,
        evidence_reference: format!("limits-api:{idempotency_key}"),
    })
}

async fn current(
    repo: &dyn ReferenceRepository,
    key: MasterKey,
) -> Result<Option<MasterRecord>, ReferenceError> {
    repo.get_master(&key).await
}

/// D87: at most `MAX_LIMITED_BOOKS` books hold a non-empty set. Counted under the limit
/// write lock, so two first sets cannot both take the last place.
async fn within_book_bound(
    repo: &dyn ReferenceRepository,
    book: &BookId,
    limits: &[LimitDefinition],
    base: Option<&MasterRecord>,
) -> Result<bool, ReferenceError> {
    let already = match base {
        Some(record) => !set_of(record)?.limits.is_empty(),
        None => false,
    };
    if limits.is_empty() || already {
        return Ok(true);
    }
    let mut others = 0usize;
    let mut after: Option<String> = None;
    loop {
        let page = repo
            .list_masters(MasterKind::LimitSet, after.as_deref(), MAX_REFERENCE_PAGE)
            .await?;
        for record in &page {
            let set = set_of(record)?;
            if set.book_id != *book && !set.limits.is_empty() {
                others += 1;
            }
        }
        if others >= MAX_LIMITED_BOOKS {
            return Ok(false);
        }
        if page.len() < MAX_REFERENCE_PAGE {
            return Ok(true);
        }
        after = page
            .last()
            .map(|r| set_of(r).map(|s| s.book_id.as_str().to_owned()))
            .transpose()?;
    }
}

fn too_many_books() -> Response {
    conflict(
        "LIMITED_BOOKS_EXCEEDED",
        "at most 64 books hold a non-empty limit set",
    )
}

async fn commit(
    state: &AppState,
    idempotency_key: RequestId,
    master_changes: Vec<MasterChange>,
    at: UtcTimestamp,
) -> Result<Vec<MasterRecord>, ReferenceError> {
    let command = ReferenceCommand {
        idempotency_key,
        changes: Vec::new(),
        master_changes,
    };
    Ok(state
        .engine
        .commit_reference(&command, at)
        .await?
        .master_records)
}

/// The record a retained receipt committed under `key`, if this idempotency key already
/// committed one; a receipt for anything else is a payload conflict.
async fn retained(
    repo: &dyn ReferenceRepository,
    idempotency_key: &RequestId,
    key: &MasterKey,
) -> Result<Option<MasterRecord>, ReferenceError> {
    let Some(prior) = repo.get_commit(idempotency_key).await? else {
        return Ok(None);
    };
    prior
        .master_records
        .into_iter()
        .find(|record| record.definition.key() == *key)
        .map(Some)
        .ok_or(ReferenceError::Conflict(
            "idempotency key was used with a different payload",
        ))
}

fn different_payload() -> ReferenceError {
    ReferenceError::Conflict("idempotency key was used with a different payload")
}

// =============================================================================
// VIEWS
// =============================================================================

#[derive(Serialize)]
struct SetView<'a> {
    book_id: &'a BookId,
    version: Version,
    recorded_at: UtcTimestamp,
    proposed_by: &'a str,
    approved_by: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    proposal_version: Option<Version>,
    evidence_reference: &'a str,
    limits: &'a [LimitDefinition],
}

fn set_response(record: &MasterRecord, status: StatusCode) -> Response {
    let set = match set_of(record) {
        Ok(set) => set,
        Err(error) => return reference_error(error),
    };
    let view = SetView {
        book_id: &set.book_id,
        version: record.version,
        recorded_at: record.recorded_at,
        proposed_by: &set.proposed_by,
        approved_by: &set.approved_by,
        proposal_version: set.proposal_version,
        evidence_reference: &set.provenance.evidence_reference,
        limits: &set.limits,
    };
    with_reference_version((status, Json(view)).into_response(), record.version)
}

#[derive(Serialize)]
struct ProposalView<'a> {
    book_id: &'a BookId,
    version: Version,
    recorded_at: UtcTimestamp,
    open: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_version: Option<Version>,
    proposed_by: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    closed: &'a Option<LimitProposalClosure>,
    evidence_reference: &'a str,
    limits: &'a [LimitDefinition],
}

fn proposal_response(record: &MasterRecord, status: StatusCode) -> Response {
    let proposal = match proposal_of(record) {
        Ok(proposal) => proposal,
        Err(error) => return reference_error(error),
    };
    let view = ProposalView {
        book_id: &proposal.book_id,
        version: record.version,
        recorded_at: record.recorded_at,
        open: proposal.is_open(),
        base_version: proposal.base_version,
        proposed_by: &proposal.proposed_by,
        closed: &proposal.closed,
        evidence_reference: &proposal.provenance.evidence_reference,
        limits: &proposal.limits,
    };
    with_reference_version((status, Json(view)).into_response(), record.version)
}

fn written(record: &MasterRecord) -> StatusCode {
    if record.version.get() == 1 {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    }
}

// =============================================================================
// WRITES
// =============================================================================

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LimitSetRequest {
    limits: Vec<LimitDefinition>,
}

/// `PUT /api/v1/books/{book_id}/limits`: propose a complete limit set.
///
/// `If-Match` names the limit set version the change replaces; absent proposes the book's
/// first set. `Idempotency-Key` is required. Under required access the change is a
/// proposal, returned with `202 Accepted`, that a different subject approves. The caller
/// holds each instrument the new limits and the current set name, since the change drops
/// any limit it leaves out. A new proposal replaces an open one, which withdraws it, so
/// the caller must also hold each instrument the open one names. Under local development it commits at once (`201` or
/// `200`), recorded as `local-development`.
pub(crate) async fn put_limits(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<LimitSetRequest>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = payload else {
        return reference_error(ReferenceError::InvalidInput("invalid limit set JSON"));
    };
    let (id, idempotency_key, base) = match (book_id(raw), key(&headers)) {
        (Ok(id), Ok(idempotency_key)) => match optional_expected(&headers) {
            Ok(base) => (id, idempotency_key, base),
            Err(error) => return reference_error(error),
        },
        (Err(error), _) | (_, Err(error)) => return reference_error(error),
    };
    if let Err(error) = validate_limits(&request.limits) {
        return reference_error(error);
    }
    if let Err(response) = authorize_write(&state, &ctx, &id, &request.limits) {
        return response;
    }
    let subject = recorded_subject(&ctx);
    let _writes = state.limit_writes.lock().await;
    match propose(
        &state,
        &ctx,
        id,
        idempotency_key,
        base,
        request.limits,
        subject,
    )
    .await
    {
        Ok(response) => response,
        Err(response) => response,
    }
}

async fn propose(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    id: BookId,
    idempotency_key: RequestId,
    base: Option<Version>,
    limits: Vec<LimitDefinition>,
    subject: String,
) -> Result<Response, Response> {
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let one_step = state.access.is_local_development();
    let written_key = if one_step {
        MasterKey::LimitSet(id.clone())
    } else {
        MasterKey::LimitProposal(id.clone())
    };
    // An exact retry returns the retained record; any other reuse of the key conflicts.
    if let Some(record) = retained(repo, &idempotency_key, &written_key)
        .await
        .map_err(reference_error)?
    {
        let same = match &record.definition {
            MasterDefinition::LimitSet(set) => {
                set.limits == limits
                    && set.proposed_by == subject
                    && set.proposal_version.is_none()
                    && record.version.get() == base.map_or(1, |v| v.get() + 1)
            }
            MasterDefinition::LimitProposal(proposal) => {
                proposal.is_open()
                    && proposal.limits == limits
                    && proposal.base_version == base
                    && proposal.proposed_by == subject
            }
            _ => false,
        };
        if !same {
            return Err(reference_error(different_payload()));
        }
        return Ok(if one_step {
            set_response(&record, written(&record))
        } else {
            proposal_response(&record, StatusCode::ACCEPTED)
        });
    }
    if current(repo, MasterKey::Book(id.clone()))
        .await
        .map_err(reference_error)?
        .is_none()
    {
        return Err(reference_error(ReferenceError::NotFound));
    }
    let set = current(repo, MasterKey::LimitSet(id.clone()))
        .await
        .map_err(reference_error)?;
    authorize_replaced(state, ctx, &id, set.as_ref())?;
    if set.as_ref().map(|r| r.version) != base {
        return Err(conflict(
            "VERSION_CONFLICT",
            "If-Match must name the current limit set version, or be absent when the book has none",
        ));
    }
    let provenance = evidence(&idempotency_key).map_err(reference_error)?;
    if one_step {
        if !within_book_bound(repo, &id, &limits, set.as_ref())
            .await
            .map_err(reference_error)?
        {
            return Err(too_many_books());
        }
        let at = commit_time(set.iter().map(|r| r.recorded_at)).map_err(reference_error)?;
        let definition = LimitSetDefinition {
            book_id: id,
            limits,
            proposed_by: subject.clone(),
            approved_by: subject,
            proposal_version: None,
            effective: EffectiveInterval {
                from: at,
                until: None,
            },
            provenance,
        };
        let change = MasterChange {
            expected_version: base,
            definition: MasterDefinition::LimitSet(Box::new(definition)),
        };
        let records = commit(state, idempotency_key, vec![change], at)
            .await
            .map_err(reference_error)?;
        let record = records
            .first()
            .ok_or_else(|| reference_error(ReferenceError::Unavailable))?;
        return Ok(set_response(record, written(record)));
    }
    let open = current(repo, MasterKey::LimitProposal(id.clone()))
        .await
        .map_err(reference_error)?;
    // Replacing an open proposal withdraws it, so the caller must hold its instruments too.
    if let Some(record) = &open {
        let existing = proposal_of(record).map_err(reference_error)?;
        if existing.is_open() {
            authorize_write(state, ctx, &id, &existing.limits)?;
        }
    }
    let at = commit_time(open.iter().map(|r| r.recorded_at)).map_err(reference_error)?;
    let definition = LimitProposalDefinition {
        book_id: id,
        base_version: base,
        limits,
        proposed_by: subject,
        closed: None,
        effective: EffectiveInterval {
            from: at,
            until: None,
        },
        provenance,
    };
    let change = MasterChange {
        expected_version: open.map(|r| r.version),
        definition: MasterDefinition::LimitProposal(Box::new(definition)),
    };
    let records = commit(state, idempotency_key, vec![change], at)
        .await
        .map_err(reference_error)?;
    let record = records
        .first()
        .ok_or_else(|| reference_error(ReferenceError::Unavailable))?;
    Ok(proposal_response(record, StatusCode::ACCEPTED))
}

/// The open proposal `If-Match` names, or the response that refuses the request. The
/// caller must hold `LimitWrite` on each instrument the proposal names before learning
/// anything more about it.
async fn open_proposal(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    id: &BookId,
    expected: Version,
) -> Result<(MasterRecord, LimitProposalDefinition), Response> {
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let record = current(repo, MasterKey::LimitProposal(id.clone()))
        .await
        .map_err(reference_error)?
        .ok_or_else(|| reference_error(ReferenceError::NotFound))?;
    authorize_write(
        state,
        ctx,
        id,
        &proposal_of(&record).map_err(reference_error)?.limits,
    )?;
    if record.version != expected {
        return Err(conflict(
            "VERSION_CONFLICT",
            "If-Match must name the current proposal version",
        ));
    }
    let proposal = proposal_of(&record).map_err(reference_error)?.clone();
    if !proposal.is_open() {
        return Err(conflict("LIMIT_PROPOSAL_CLOSED", "the proposal is closed"));
    }
    Ok((record, proposal))
}

/// `POST /api/v1/books/{book_id}/limits/proposal/approve`: commit the open proposal.
///
/// `If-Match` names the proposal version approved and `Idempotency-Key` is required. The
/// approver must be a different subject from the proposer and hold each instrument the
/// proposal and the current set name, and the limit set must still be the version the
/// proposal replaces. The set and the closed proposal commit in one
/// transaction.
pub(crate) async fn approve_limits(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
) -> Response {
    let (id, idempotency_key, expected) =
        match (book_id(raw), key(&headers), expected_version(&headers)) {
            (Ok(id), Ok(k), Ok(v)) => (id, k, v),
            (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
                return reference_error(error)
            }
        };
    let subject = recorded_subject(&ctx);
    let _writes = state.limit_writes.lock().await;
    match approve(&state, &ctx, id, idempotency_key, expected, subject).await {
        Ok(response) | Err(response) => response,
    }
}

async fn approve(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    id: BookId,
    idempotency_key: RequestId,
    expected: Version,
    subject: String,
) -> Result<Response, Response> {
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    if let Some(record) = retained(repo, &idempotency_key, &MasterKey::LimitSet(id.clone()))
        .await
        .map_err(reference_error)?
    {
        let set = set_of(&record).map_err(reference_error)?;
        authorize_write(state, ctx, &id, &set.limits)?;
        if set.proposal_version != Some(expected) || set.approved_by != subject {
            return Err(reference_error(different_payload()));
        }
        return Ok(set_response(&record, written(&record)));
    }
    let (record, proposal) = open_proposal(state, ctx, &id, expected).await?;
    if proposal.proposed_by == subject {
        return Err(typed_error(
            StatusCode::FORBIDDEN,
            "LIMIT_SELF_APPROVAL",
            "a limit change is approved by a different subject from its proposer",
        ));
    }
    let set = current(repo, MasterKey::LimitSet(id.clone()))
        .await
        .map_err(reference_error)?;
    authorize_replaced(state, ctx, &id, set.as_ref())?;
    if set.as_ref().map(|r| r.version) != proposal.base_version {
        return Err(conflict(
            "LIMIT_SET_CHANGED",
            "the limit set changed after the proposal; propose again",
        ));
    }
    if !within_book_bound(repo, &id, &proposal.limits, set.as_ref())
        .await
        .map_err(reference_error)?
    {
        return Err(too_many_books());
    }
    let floor = [
        after(proposal.effective.from).map_err(reference_error)?,
        record.recorded_at,
    ];
    let at = commit_time(floor.into_iter().chain(set.iter().map(|r| r.recorded_at)))
        .map_err(reference_error)?;
    let set_version = Version::new(proposal.base_version.map_or(1, |v| v.get() + 1))
        .map_err(|_| reference_error(ReferenceError::Unavailable))?;
    let provenance = evidence(&idempotency_key).map_err(reference_error)?;
    let definition = LimitSetDefinition {
        book_id: id,
        limits: proposal.limits.clone(),
        proposed_by: proposal.proposed_by.clone(),
        approved_by: subject.clone(),
        proposal_version: Some(expected),
        effective: EffectiveInterval {
            from: at,
            until: None,
        },
        provenance: provenance.clone(),
    };
    let mut closed = proposal;
    closed.closed = Some(LimitProposalClosure::Approved {
        by: subject,
        limit_set_version: set_version,
    });
    closed.effective.until = Some(at);
    closed.provenance = provenance;
    let changes = vec![
        MasterChange {
            expected_version: closed.base_version,
            definition: MasterDefinition::LimitSet(Box::new(definition)),
        },
        MasterChange {
            expected_version: Some(expected),
            definition: MasterDefinition::LimitProposal(Box::new(closed)),
        },
    ];
    let records = commit(state, idempotency_key, changes, at)
        .await
        .map_err(reference_error)?;
    let record = records
        .iter()
        .find(|r| matches!(r.definition, MasterDefinition::LimitSet(_)))
        .ok_or_else(|| reference_error(ReferenceError::Unavailable))?;
    Ok(set_response(record, written(record)))
}

/// `POST /api/v1/books/{book_id}/limits/proposal/withdraw`: close the open proposal
/// without committing it, recording who closed it. `If-Match` names the proposal version
/// and `Idempotency-Key` is required.
pub(crate) async fn withdraw_limits(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
) -> Response {
    let (id, idempotency_key, expected) =
        match (book_id(raw), key(&headers), expected_version(&headers)) {
            (Ok(id), Ok(k), Ok(v)) => (id, k, v),
            (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
                return reference_error(error)
            }
        };
    let subject = recorded_subject(&ctx);
    let _writes = state.limit_writes.lock().await;
    match withdraw(&state, &ctx, id, idempotency_key, expected, subject).await {
        Ok(response) | Err(response) => response,
    }
}

async fn withdraw(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    id: BookId,
    idempotency_key: RequestId,
    expected: Version,
    subject: String,
) -> Result<Response, Response> {
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let proposal_key = MasterKey::LimitProposal(id.clone());
    if let Some(record) = retained(repo, &idempotency_key, &proposal_key)
        .await
        .map_err(reference_error)?
    {
        let proposal = proposal_of(&record).map_err(reference_error)?;
        authorize_write(state, ctx, &id, &proposal.limits)?;
        let same = record.version.get() == expected.get() + 1
            && matches!(&proposal.closed, Some(LimitProposalClosure::Withdrawn { by }) if *by == subject);
        if !same {
            return Err(reference_error(different_payload()));
        }
        return Ok(proposal_response(&record, StatusCode::OK));
    }
    let (record, proposal) = open_proposal(state, ctx, &id, expected).await?;
    let floor = [
        after(proposal.effective.from).map_err(reference_error)?,
        record.recorded_at,
    ];
    let at = commit_time(floor).map_err(reference_error)?;
    let mut closed = proposal;
    closed.closed = Some(LimitProposalClosure::Withdrawn { by: subject });
    closed.effective.until = Some(at);
    closed.provenance = evidence(&idempotency_key).map_err(reference_error)?;
    let change = MasterChange {
        expected_version: Some(expected),
        definition: MasterDefinition::LimitProposal(Box::new(closed)),
    };
    let records = commit(state, idempotency_key, vec![change], at)
        .await
        .map_err(reference_error)?;
    let record = records
        .first()
        .ok_or_else(|| reference_error(ReferenceError::Unavailable))?;
    Ok(proposal_response(record, StatusCode::OK))
}

// =============================================================================
// READS
// =============================================================================

async fn read_master(
    state: &AppState,
    key: &MasterKey,
    version: Option<Version>,
) -> Result<MasterRecord, Response> {
    let repo = state.engine.reference_repository();
    let found = match version {
        Some(version) => repo.get_master_version(key, version).await,
        None => repo.get_master(key).await,
    };
    found
        .map_err(reference_error)?
        .ok_or_else(|| reference_error(ReferenceError::NotFound))
}

async fn read_set(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    raw: String,
    version: Option<Version>,
) -> Result<Response, Response> {
    let id = book_id(raw).map_err(reference_error)?;
    let record = read_master(state, &MasterKey::LimitSet(id.clone()), version).await?;
    let set = set_of(&record).map_err(reference_error)?;
    authorize_read(state, ctx, &[DataScope::Books], &id, &set.limits).await?;
    Ok(set_response(&record, StatusCode::OK))
}

/// `GET /api/v1/books/{book_id}/limits`: the current limit set, pinned in `ETag`.
pub(crate) async fn get_limits(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
) -> Response {
    match read_set(&state, &ctx, raw, None).await {
        Ok(response) | Err(response) => response,
    }
}

/// `GET /api/v1/books/{book_id}/limits/versions/{version}`: one immutable version.
pub(crate) async fn get_limits_version(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path((raw, version)): Path<(String, u64)>,
) -> Response {
    let Ok(version) = Version::new(version) else {
        return reference_error(ReferenceError::InvalidInput("invalid version"));
    };
    match read_set(&state, &ctx, raw, Some(version)).await {
        Ok(response) | Err(response) => response,
    }
}

/// `GET /api/v1/books/{book_id}/limits/proposal`: the latest proposal, open or closed.
pub(crate) async fn get_limit_proposal(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
) -> Response {
    let read = async {
        let id = book_id(raw).map_err(reference_error)?;
        let record = read_master(&state, &MasterKey::LimitProposal(id.clone()), None).await?;
        let proposal = proposal_of(&record).map_err(reference_error)?;
        authorize_read(&state, &ctx, &[DataScope::Books], &id, &proposal.limits).await?;
        Ok::<_, Response>(proposal_response(&record, StatusCode::OK))
    };
    match read.await {
        Ok(response) | Err(response) => response,
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

fn default_page() -> usize {
    100
}

/// `GET /api/v1/books/{book_id}/limits/history`: version summaries from `from`, oldest
/// first. A summary names who proposed and approved each version, not its limits.
pub(crate) async fn get_limits_history(
    State(state): State<Arc<AppState>>,
    Path(raw): Path<String>,
    query: Result<Query<HistoryQuery>, QueryRejection>,
) -> Response {
    let query = match query {
        Ok(Query(query)) if query.limit > 0 && query.limit <= MAX_HISTORY_PAGE => query,
        _ => return reference_error(ReferenceError::InvalidInput("invalid history query")),
    };
    let (id, from) = match (book_id(raw), Version::new(query.from)) {
        (Ok(id), Ok(from)) => (id, from),
        (Err(error), _) => return reference_error(error),
        (_, Err(_)) => return reference_error(ReferenceError::InvalidInput("invalid version")),
    };
    let key = MasterKey::LimitSet(id);
    let repo = state.engine.reference_repository();
    let current = match repo.get_master(&key).await {
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
        let record = match Version::new(number) {
            Ok(version) => match repo.get_master_version(&key, version).await {
                Ok(Some(record)) => record,
                Ok(None) => return reference_error(ReferenceError::Unavailable),
                Err(error) => return reference_error(error),
            },
            Err(_) => return reference_error(ReferenceError::Unavailable),
        };
        let set = match set_of(&record) {
            Ok(set) => set,
            Err(error) => return reference_error(error),
        };
        versions.push(serde_json::json!({
            "version":record.version,
            "recorded_at":record.recorded_at,
            "proposed_by":set.proposed_by,
            "approved_by":set.approved_by,
            "proposal_version":set.proposal_version,
            "limit_count":set.limits.len(),
        }));
    }
    let next_from = (last < current.get()).then_some(last + 1);
    Json(serde_json::json!({
        "versions":versions,
        "current_version":current,
        "next_from":next_from,
    }))
    .into_response()
}

// =============================================================================
// STATUS
// =============================================================================

/// One limit and its evaluation: the bounds beside the value they were judged against.
#[derive(Serialize)]
struct StatusItem<'a> {
    measure: &'a LimitMeasure,
    #[serde(skip_serializing_if = "Option::is_none")]
    lower: Option<convex_core::contracts::ExactDecimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    upper: Option<convex_core::contracts::ExactDecimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning_fraction: Option<convex_core::contracts::ExactDecimal>,
    #[serde(flatten)]
    evaluation: &'a LimitEvaluation,
}

#[derive(Serialize)]
struct StatusView<'a> {
    book_id: &'a BookId,
    book_version: Version,
    limit_set_version: Version,
    evaluated_at: UtcTimestamp,
    /// The distinct snapshots whose results the evaluation read.
    snapshot_ids: Vec<String>,
    limits: Vec<StatusItem<'a>>,
}

/// `GET /api/v1/books/{book_id}/limits/status`: each limit of the current set, evaluated
/// now against the current Live results.
///
/// A position without an accepted result (by analytics by book's rule) makes each limit
/// that reads market data `unknown`, and so does one whose result cannot be read as an
/// analyzer quote or whose priced terms cannot be read. The response carries the book and
/// set versions, the time and the snapshots read.
pub(crate) async fn get_limit_status(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
) -> Response {
    match status(&state, &ctx, raw).await {
        Ok(response) | Err(response) => response,
    }
}

async fn status(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    raw: String,
) -> Result<Response, Response> {
    let id = book_id(raw).map_err(reference_error)?;
    let set_record = read_master(state, &MasterKey::LimitSet(id.clone()), None).await?;
    let set = set_of(&set_record).map_err(reference_error)?;
    let book_record = authorize_read(state, ctx, STATUS_DATA, &id, &set.limits)
        .await?
        .ok_or_else(|| reference_error(ReferenceError::NotFound))?;
    let book = book_of(&book_record).map_err(reference_error)?;
    let evaluated_at = now().map_err(reference_error)?;
    let versions = (book_record.version, set_record.version);
    evaluated(state, versions, &set.limits, book, evaluated_at).await
}

/// Evaluate `limits` on `book` against the current Live results, and answer in the status
/// route's shape. `versions` are the book and limit set versions read.
async fn evaluated(
    state: &AppState,
    versions: (Version, Version),
    limits: &[LimitDefinition],
    book: &BookDefinition,
    evaluated_at: UtcTimestamp,
) -> Result<Response, Response> {
    let accepted = accepted_runtime_results(state);
    let mut terms = PricedTerms::new();
    read_priced_terms(
        state.engine.reference_repository().as_ref(),
        book,
        &accepted,
        &mut terms,
    )
    .await;
    let evaluation = evaluate_book(
        book,
        limits,
        &accepted,
        &terms,
        state.engine.runtime_settlement_date(),
    )
    .map_err(|error| {
        tracing::error!(%error, "limit status: evaluation refused a book and its limit set");
        reference_error(ReferenceError::Unavailable)
    })?;
    let items = limits
        .iter()
        .zip(&evaluation.evaluations)
        .map(|(limit, evaluation)| StatusItem {
            measure: &limit.measure,
            lower: limit.lower,
            upper: limit.upper,
            warning_fraction: limit.warning_fraction,
            evaluation,
        })
        .collect();
    let view = StatusView {
        book_id: &book.book_id,
        book_version: versions.0,
        limit_set_version: versions.1,
        evaluated_at,
        snapshot_ids: evaluation.snapshot_ids,
        limits: items,
    };
    Ok(Json(view).into_response())
}

// =============================================================================
// RECORDS (P15b)
// =============================================================================

/// Records a page holds unless the request says otherwise.
const DEFAULT_RECORDS_PAGE: usize = 100;
/// A page stops before a record that would take it past either bound, so the access
/// layer's response scan (4 MiB, 65,536 JSON values) always reads it whole. A record's
/// JSON is at most 128 KiB, so one always fits.
const MAX_RECORDS_PAGE_BYTES: usize = 3 * 1024 * 1024;
const MAX_RECORDS_PAGE_VALUES: usize = 60_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordsQuery {
    #[serde(default)]
    after: u64,
    #[serde(default = "default_records_page")]
    limit: usize,
}

fn default_records_page() -> usize {
    DEFAULT_RECORDS_PAGE
}

/// What a record names beyond the book: read with the record's types, so a new reason
/// that names instruments must be listed in [`recorded_instruments`].
#[derive(Deserialize)]
struct RecordNames {
    measure: LimitMeasure,
    #[serde(default)]
    member: Option<String>,
    #[serde(default)]
    reason: Option<UnknownReason>,
}

/// Each instrument a record names: a `face` limit's instrument, or for a `face` limit on
/// each position the one that set the status, and each instrument its reason lists. A
/// record can name an instrument the book no longer holds.
fn recorded_instruments(names: &RecordNames) -> BTreeSet<&str> {
    let mut instruments = BTreeSet::new();
    if let LimitMeasure::Face { instrument } = &names.measure {
        match instrument {
            LimitMember::One(id) => {
                instruments.insert(id.as_str());
            }
            LimitMember::Each => instruments.extend(names.member.as_deref()),
        }
    }
    match &names.reason {
        Some(
            UnknownReason::UnsupportedQuantityUnit { instruments: list }
            | UnknownReason::MissingResult { instruments: list }
            | UnknownReason::ForeignCurrency { instruments: list }
            | UnknownReason::MeasureNotComputed {
                instruments: list, ..
            }
            | UnknownReason::MissingClassification {
                instruments: list, ..
            },
        ) => instruments.extend(list.first.iter().map(InstrumentId::as_str)),
        Some(UnknownReason::QuoteRefused { instrument, .. }) => {
            instruments.insert(instrument.as_str());
        }
        Some(
            UnknownReason::BookCurrencyRequired {}
            | UnknownReason::ZeroNetValue {}
            | UnknownReason::ZeroGrossValue {}
            | UnknownReason::ArithmeticBound {},
        )
        | None => {}
    }
    instruments
}

/// The JSON values a response scan counts in `value`: it, and each value inside it.
fn json_values(value: &serde_json::Value) -> usize {
    1 + match value {
        serde_json::Value::Object(map) => map.values().map(json_values).sum(),
        serde_json::Value::Array(items) => items.iter().map(json_values).sum(),
        _ => 0,
    }
}

/// `GET /api/v1/books/{book_id}/limits/records?after=&limit=`: the book's recorded status
/// changes with record IDs above `after`, oldest first, each as its `limit_status` frame
/// says it (without `type`), and `next_after`, the cursor for the next page.
///
/// Access is status's read rule on the book, its current instruments and those its
/// current set names, plus each instrument a returned record names. A record naming one
/// the caller cannot read is left out and counted in `withheld`; the cursor still passes
/// it, so a reader is never held behind a record it cannot see. So is a record whose
/// measure or reason this server cannot read (one a newer server wrote), with an error
/// logged.
pub(crate) async fn get_limit_records(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    audit: Option<Extension<Arc<dyn AuditStore>>>,
    Path(raw): Path<String>,
    query: Result<Query<RecordsQuery>, QueryRejection>,
) -> Response {
    match records(&state, &ctx, audit, raw, query).await {
        Ok(response) | Err(response) => response,
    }
}

async fn records(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    audit: Option<Extension<Arc<dyn AuditStore>>>,
    raw: String,
    query: Result<Query<RecordsQuery>, QueryRejection>,
) -> Result<Response, Response> {
    let query = match query {
        Ok(Query(query)) if query.limit > 0 && query.limit <= MAX_ENTITY_PAGE => query,
        _ => {
            return Err(reference_error(ReferenceError::InvalidInput(
                "invalid records query",
            )))
        }
    };
    let id = book_id(raw).map_err(reference_error)?;
    // A book can hold records with no current set: one emptied, or never committed.
    let set_record = state
        .engine
        .reference_repository()
        .get_master(&MasterKey::LimitSet(id.clone()))
        .await
        .map_err(reference_error)?;
    let limits = match &set_record {
        Some(record) => set_of(record).map_err(reference_error)?.limits.as_slice(),
        None => &[],
    };
    authorize_read(state, ctx, STATUS_DATA, &id, limits)
        .await?
        .ok_or_else(|| reference_error(ReferenceError::NotFound))?;
    let Some(Extension(audit)) = audit else {
        return Err(
            crate::error::ApiError::unavailable("this server does not monitor limits")
                .into_response(),
        );
    };
    let page = audit
        .entity_page(LIMIT_STATUS_ENTITY, id.as_str(), query.after, query.limit)
        .await
        .map_err(|error| {
            tracing::error!(%error, book = %id, "limit records: the audit store refused a page");
            reference_error(ReferenceError::Unavailable)
        })?;

    let mut readable: HashMap<String, bool> = HashMap::new();
    let mut shown: Vec<&str> = Vec::new();
    let (mut bytes, mut values, mut withheld) = (0usize, 0usize, 0usize);
    let mut next_after = query.after;
    let mut more = page.more;
    for entry in &page.entries {
        let details = match (entry.event_type.as_str(), &entry.details) {
            (LIMIT_STATUS_EVENT, Some(details)) => details.as_str(),
            // Not a status change: nothing to show, and the cursor passes it.
            _ => {
                next_after = entry.id;
                continue;
            }
        };
        // A record whose instruments cannot be read is withheld, as one the caller cannot
        // read is, and the cursor passes it rather than refusing every page that holds it.
        let parsed = serde_json::from_str::<serde_json::Value>(details).and_then(|value| {
            let names = RecordNames::deserialize(&value)?;
            Ok((value, names))
        });
        let (value, names) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                tracing::error!(%error, record = entry.id, book = %id, "limit records: a record's measure or reason cannot be read; it is withheld");
                withheld += 1;
                next_after = entry.id;
                continue;
            }
        };
        // Each distinct instrument is checked once a page, beside the book.
        let allowed = recorded_instruments(&names).into_iter().all(|instrument| {
            *readable.entry(instrument.to_owned()).or_insert_with(|| {
                authorize(
                    state,
                    ctx,
                    Action::BookRead,
                    STATUS_DATA,
                    &id,
                    [instrument.to_owned()],
                )
                .is_ok()
            })
        });
        if !allowed {
            withheld += 1;
            next_after = entry.id;
            continue;
        }
        let cost = json_values(&value);
        if !shown.is_empty()
            && (bytes + details.len() > MAX_RECORDS_PAGE_BYTES
                || values + cost > MAX_RECORDS_PAGE_VALUES)
        {
            more = true;
            break;
        }
        bytes += details.len();
        values += cost;
        shown.push(details);
        next_after = entry.id;
    }

    // Each record is the JSON the monitor stored, which its frame carried, unchanged.
    let book =
        serde_json::to_string(&id).map_err(|_| reference_error(ReferenceError::Unavailable))?;
    let mut body = String::with_capacity(bytes + shown.len() + 128);
    body.push_str("{\"book_id\":");
    body.push_str(&book);
    body.push_str(",\"records\":[");
    for (index, record) in shown.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        body.push_str(record);
    }
    body.push_str(&format!(
        "],\"withheld\":{withheld},\"next_after\":{next_after},\"more\":{more}}}"
    ));
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        )],
        body,
    )
        .into_response())
}

// =============================================================================
// PRE-TRADE CHECK (P14d)
// =============================================================================

/// What a refused check's rows response says.
const CHECK_REJECTED: &str = "check rejected; nothing was evaluated";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CheckRequest {
    changes: Vec<RowRequest>,
}

/// `POST /api/v1/books/{book_id}/limits/check`: each limit of the current set, evaluated
/// now against the current Live results as if `changes` were applied to the book (D88).
///
/// Each row is a signed quantity added to the held position, in a book snapshot row's
/// grammar (D89). A row for a held instrument changes only its quantity: one that states a
/// different unit, reference version, sector or rating is refused. A position a row brings
/// to exactly zero is dropped, and a new instrument follows the held positions, in row
/// order. The response is the status route's (D90). Nothing is stored, so the route takes
/// no `Idempotency-Key` or `If-Match`. The caller holds `BookRead` on the book, each
/// instrument it holds, each one a limit names and each one a row names (D91).
pub(crate) async fn check_limits(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
    payload: Result<Json<CheckRequest>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = payload else {
        return reference_error(ReferenceError::InvalidInput("invalid limit check JSON"));
    };
    if request.changes.len() > MAX_CHECK_ROWS {
        return typed_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "CHECK_TOO_LARGE",
            "a check takes at most 256 rows",
        );
    }
    match check(&state, &ctx, raw, request.changes).await {
        Ok(response) | Err(response) => response,
    }
}

async fn check(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    raw: String,
    rows: Vec<RowRequest>,
) -> Result<Response, Response> {
    let id = book_id(raw).map_err(reference_error)?;
    let set_record = read_master(state, &MasterKey::LimitSet(id.clone()), None).await?;
    let set = set_of(&set_record).map_err(reference_error)?;
    let book_record = authorize_read(state, ctx, STATUS_DATA, &id, &set.limits)
        .await?
        .ok_or_else(|| reference_error(ReferenceError::NotFound))?;
    let held = book_of(&book_record).map_err(reference_error)?;
    let units: Vec<Option<QuantityUnit>> = rows.iter().map(RowRequest::unit).collect();
    let evaluated_at = now().map_err(reference_error)?;
    let lookup = ReferenceLookup {
        as_of: evaluated_at,
        known_at: evaluated_at,
    };
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let resolved = match resolve_rows(repo, lookup, rows)
        .await
        .map_err(reference_error)?
    {
        Ok(resolved) => resolved,
        Err((errors, truncated)) => return Err(rows_rejected(CHECK_REJECTED, &errors, truncated)),
    };
    // The access layer read each `instrument_id` in the body; this also covers each
    // instrument a row reached through an external identifier.
    let proposed = resolved
        .iter()
        .map(|row| row.position.instrument_id.as_str().to_owned());
    authorize(state, ctx, Action::BookRead, STATUS_DATA, &id, proposed)?;
    let book = applied(held, &resolved, &units)?;
    let versions = (book_record.version, set_record.version);
    evaluated(state, versions, &set.limits, &book, evaluated_at).await
}

/// Why a row may not change a held position, if it may not: a row changes only the
/// quantity, so a unit, reference version or classification it states must be the held one.
fn differs(
    held: &BookPosition,
    row: &BookPosition,
    unit: Option<QuantityUnit>,
) -> Option<&'static str> {
    if unit.is_some_and(|unit| unit != held.unit) {
        return Some("a change keeps the held position's unit");
    }
    if row.reference_version.is_some() && row.reference_version != held.reference_version {
        return Some("a change keeps the held position's reference version");
    }
    if row.sector.is_some() && row.sector != held.sector {
        return Some("a change keeps the held position's sector");
    }
    if row.rating.is_some() && row.rating != held.rating {
        return Some("a change keeps the held position's rating");
    }
    None
}

/// `held` with `rows` applied by D89's rule: each row's quantity is added to the held
/// position, a position a row brings to exactly zero is dropped, and each new instrument
/// follows the held positions in row order. `units` are the units the rows stated.
fn applied(
    held: &BookDefinition,
    rows: &[Resolved],
    units: &[Option<QuantityUnit>],
) -> Result<BookDefinition, Response> {
    let index: HashMap<&str, usize> = held
        .positions
        .iter()
        .enumerate()
        .map(|(at, position)| (position.instrument_id.as_str(), at))
        .collect();
    let mut book = held.clone();
    let mut changed = vec![false; book.positions.len()];
    let mut added = Vec::new();
    let mut errors = Vec::new();
    for (row_index, (row, unit)) in rows.iter().zip(units).enumerate() {
        let row = &row.position;
        let Some(&at) = index.get(row.instrument_id.as_str()) else {
            added.push(row.clone());
            continue;
        };
        let (Some(position), Some(flag)) = (book.positions.get_mut(at), changed.get_mut(at)) else {
            return Err(reference_error(ReferenceError::Unavailable));
        };
        let refused = differs(position, row, *unit).or_else(|| {
            match position
                .quantity
                .decimal()
                .checked_add(row.quantity.decimal())
            {
                Some(sum) => {
                    position.quantity = ExactDecimal::new(sum);
                    *flag = true;
                    None
                }
                None => Some("the changed quantity exceeds exact representation"),
            }
        });
        if let Some(reason) = refused {
            errors.push(RowError {
                index: row_index,
                code: "INVALID_ROW",
                reason,
            });
        }
    }
    if !errors.is_empty() {
        let truncated = errors.len() > MAX_ROW_ERRORS;
        errors.truncate(MAX_ROW_ERRORS);
        return Err(rows_rejected(CHECK_REJECTED, &errors, truncated));
    }
    let mut positions: Vec<BookPosition> = std::mem::take(&mut book.positions)
        .into_iter()
        .zip(changed)
        .filter(|(position, changed)| !(*changed && position.quantity.decimal().is_zero()))
        .map(|(position, _)| position)
        .collect();
    positions.extend(
        added
            .into_iter()
            .filter(|position| !position.quantity.decimal().is_zero()),
    );
    if positions.len() > MAX_BOOK_POSITIONS {
        return Err(too_large());
    }
    book.positions = positions;
    book.validate().map_err(reference_error)?;
    Ok(book)
}

// =============================================================================
// EVALUATION, SHARED WITH THE MONITOR
// =============================================================================

/// Complete accepted runtime results by instrument and priced version.
pub(crate) type Accepted = BTreeMap<String, BTreeMap<u64, PublishedValuation>>;

/// Priced bond terms by instrument and version. A version is immutable, so a kept entry
/// never goes stale.
pub(crate) type PricedTerms = HashMap<(InstrumentId, Version), Arc<BondReferenceData>>;

/// The accepted result a position reads, by analytics by book's rule: the version it
/// pins, or else the latest.
fn position_result<'a>(
    accepted: &'a Accepted,
    position: &BookPosition,
) -> Option<&'a PublishedValuation> {
    let versions = accepted.get(position.instrument_id.as_str())?;
    match position.reference_version {
        Some(pin) => versions.get(&pin.get()),
        None => versions.last_key_value().map(|(_, result)| result),
    }
}

/// The instrument version a result priced.
pub(crate) fn priced_version(valuation: &PublishedValuation) -> Option<(InstrumentId, Version)> {
    match &valuation.result.provenance().subject {
        ValuationSubject::InstrumentVersion {
            instrument_id,
            version,
        } => Some((instrument_id.clone(), *version)),
        _ => None,
    }
}

/// Read into `terms` the priced terms of each position's result that `terms` lacks. A
/// version that cannot be read is left out, with a warning logged, so its position counts
/// as missing, and a later call reads it again.
pub(crate) async fn read_priced_terms(
    repo: &dyn ReferenceRepository,
    book: &BookDefinition,
    accepted: &Accepted,
    terms: &mut PricedTerms,
) {
    for position in &book.positions {
        let Some(key) = position_result(accepted, position).and_then(priced_version) else {
            continue;
        };
        if terms.contains_key(&key) {
            continue;
        }
        let id = convex_core::ids::InstrumentId::new(key.0.as_str());
        match repo.get_version(&id, key.1).await {
            Ok(Some(record)) => {
                if let Some(bond) = record.bond {
                    terms.insert(key, Arc::new(bond));
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(instrument = %position.instrument_id, %error, "limit status: priced terms are unreadable");
            }
        }
    }
}

/// One evaluation of a book's limits, in set order, and the snapshots whose results it
/// read.
pub(crate) struct BookEvaluation {
    pub(crate) evaluations: Vec<LimitEvaluation>,
    pub(crate) snapshot_ids: Vec<String>,
}

/// Evaluate `limits` on `book` against `accepted` results and their priced `terms`.
///
/// A position without an accepted result (by analytics by book's rule) makes each limit
/// that reads market data `unknown`, and so does one whose priced terms are not in `terms`
/// or whose result cannot be read as an analyzer quote (with a warning logged).
pub(crate) fn evaluate_book(
    book: &BookDefinition,
    limits: &[LimitDefinition],
    accepted: &Accepted,
    terms: &PricedTerms,
    settlement: Option<convex_core::contracts::BusinessDate>,
) -> Result<BookEvaluation, convex_engine::EngineError> {
    let mut snapshots = BTreeSet::new();
    let reads: Vec<Option<(BondQuoteOutput, &BondReferenceData)>> = book
        .positions
        .iter()
        .map(|position| {
            let valuation = position_result(accepted, position)?;
            let settlement = settlement?;
            let bond = terms.get(&priced_version(valuation)?)?;
            match convex_engine::runtime_bond_quote(valuation, settlement.date(), bond.currency) {
                Ok(quote) => {
                    let snapshot = &valuation.result.provenance().snapshot_id;
                    snapshots.insert(snapshot.as_str().to_owned());
                    Some((quote, bond.as_ref()))
                }
                Err(error) => {
                    tracing::warn!(instrument = %position.instrument_id, %error, "limit status: runtime result is not a readable quote");
                    None
                }
            }
        })
        .collect();
    let positions: Vec<LimitPosition<'_>> = book
        .positions
        .iter()
        .zip(&reads)
        .map(|(position, read)| LimitPosition {
            instrument_id: &position.instrument_id,
            quantity: position.quantity.decimal(),
            unit: position.unit,
            rating: position.rating.as_deref(),
            quote: read.as_ref().map(|(quote, _)| quote),
            terms: read.as_ref().map(|(_, bond)| LimitTerms::of(bond)),
        })
        .collect();
    let limit_book = LimitBook {
        currency: book
            .currency
            .as_deref()
            .and_then(convex_core::Currency::from_code),
        positions: &positions,
    };
    Ok(BookEvaluation {
        evaluations: evaluate_limits(&limit_book, limits)?,
        snapshot_ids: snapshots.into_iter().collect(),
    })
}
