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
    InstrumentId, RequestId, SourceId, UtcTimestamp, ValuationSubject, Version,
};
use convex_engine::limits::{evaluate_limits, LimitBook, LimitPosition, LimitTerms};
use convex_engine::ports::limits::{
    validate_limits, LimitDefinition, LimitEvaluation, LimitMeasure, LimitMember,
    LimitProposalClosure, LimitProposalDefinition, LimitSetDefinition,
};
use convex_engine::ports::output::BondQuoteOutput;
use convex_engine::ports::reference_data::BondReferenceData;
use convex_engine::ports::reference_master::*;
use convex_engine::ports::reference_repository::*;
use serde::{Deserialize, Serialize};

use crate::access::{recorded_subject, AccessContext, Action, DataScope, Resource, ResourceKind};
use crate::books::{accepted_runtime_results, book_id, book_of, optional_expected, typed_error};
use crate::handlers::AppState;
use crate::reference::{expected_version, key, now, reference_error, with_reference_version};

/// Books with a non-empty limit set: the monitor watches each of them (D87).
pub(crate) const MAX_LIMITED_BOOKS: usize = convex_engine::configuration::MAX_PLAN_BOOKS;
/// Largest page of limit set or proposal history.
const MAX_HISTORY_PAGE: usize = 500;
/// Source recorded on every limit set and proposal.
const LIMITS_API: &str = "limits-api";

// =============================================================================
// ERRORS AND SHARED CHECKS
// =============================================================================

fn conflict(code: &'static str, message: &str) -> Response {
    typed_error(StatusCode::CONFLICT, code, message)
}

fn set_of(record: &MasterRecord) -> Result<&LimitSetDefinition, ReferenceError> {
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

/// The refusal of a generic reference write that names a limit set or proposal: D85's
/// two subjects are enforced only on the book limit routes.
pub(crate) fn limits_route_required() -> Response {
    typed_error(
        StatusCode::FORBIDDEN,
        "LIMITS_ROUTE_REQUIRED",
        "limit sets and proposals are written only through /api/v1/books/{book_id}/limits",
    )
}

/// The instruments a limit names, which the response scan does not read.
fn named_instruments(limits: &[LimitDefinition]) -> impl Iterator<Item = &InstrumentId> {
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
/// proposal, returned with `202 Accepted`, that a different subject approves; a new
/// proposal replaces an open one. Under local development it commits at once (`201` or
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
    match propose(&state, id, idempotency_key, base, request.limits, subject).await {
        Ok(response) => response,
        Err(response) => response,
    }
}

async fn propose(
    state: &AppState,
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
                set.limits == limits && set.proposed_by == subject && set.proposal_version.is_none()
            }
            MasterDefinition::LimitProposal(proposal) => {
                proposal.limits == limits
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

/// The open proposal `If-Match` names, or the response that refuses the request.
async fn open_proposal(
    repo: &dyn ReferenceRepository,
    id: &BookId,
    expected: Version,
) -> Result<(MasterRecord, LimitProposalDefinition), Response> {
    let record = current(repo, MasterKey::LimitProposal(id.clone()))
        .await
        .map_err(reference_error)?
        .ok_or_else(|| reference_error(ReferenceError::NotFound))?;
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
/// approver must be a different subject from the proposer, and the limit set must still be
/// the version the proposal replaces. The set and the closed proposal commit in one
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
    let (record, proposal) = open_proposal(repo, &id, expected).await?;
    authorize_write(state, ctx, &id, &proposal.limits)?;
    if !state.access.is_local_development() && proposal.proposed_by == subject {
        return Err(typed_error(
            StatusCode::FORBIDDEN,
            "LIMIT_SELF_APPROVAL",
            "a limit change is approved by a different subject from its proposer",
        ));
    }
    let set = current(repo, MasterKey::LimitSet(id.clone()))
        .await
        .map_err(reference_error)?;
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
    let (record, proposal) = open_proposal(repo, &id, expected).await?;
    authorize_write(state, ctx, &id, &proposal.limits)?;
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

/// A position's accepted result, read as an analyzer quote, with its priced bond terms.
struct Read {
    quote: BondQuoteOutput,
    bond: BondReferenceData,
}

/// `GET /api/v1/books/{book_id}/limits/status`: each limit of the current set, evaluated
/// now against the current Live results.
///
/// A position without an accepted result (by analytics by book's rule) makes each limit
/// that reads market data `unknown`, and so does one whose result cannot be read as an
/// analyzer quote. The response carries the book and set versions, the time and the
/// snapshots read.
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
    let book_record = authorize_read(
        state,
        ctx,
        &[DataScope::Books, DataScope::Results],
        &id,
        &set.limits,
    )
    .await?
    .ok_or_else(|| reference_error(ReferenceError::NotFound))?;
    let book = book_of(&book_record).map_err(reference_error)?;
    let evaluated_at = now().map_err(reference_error)?;
    let settlement = state.engine.runtime_settlement_date();
    let repo = state.engine.reference_repository();
    let mut accepted = accepted_runtime_results(state);
    let mut snapshots = BTreeSet::new();
    let mut reads: Vec<Option<Read>> = Vec::with_capacity(book.positions.len());
    for position in &book.positions {
        let result = accepted
            .get_mut(position.instrument_id.as_str())
            .and_then(|versions| match position.reference_version {
                Some(pin) => versions.remove(&pin.get()),
                None => versions.pop_last().map(|(_, result)| result),
            });
        let (Some(valuation), Some(settlement)) = (result, settlement) else {
            reads.push(None);
            continue;
        };
        let provenance = valuation.result.provenance();
        let ValuationSubject::InstrumentVersion { version, .. } = &provenance.subject else {
            reads.push(None);
            continue;
        };
        let priced = repo
            .get_version(
                &convex_core::ids::InstrumentId::new(position.instrument_id.as_str()),
                *version,
            )
            .await
            .map_err(reference_error)?;
        let Some(bond) = priced.and_then(|record| record.bond) else {
            reads.push(None);
            continue;
        };
        match convex_engine::runtime_bond_quote(&valuation, settlement.date(), bond.currency) {
            Ok(quote) => {
                snapshots.insert(provenance.snapshot_id.as_str().to_owned());
                reads.push(Some(Read { quote, bond }));
            }
            Err(error) => {
                tracing::warn!(instrument = %position.instrument_id, %error, "limit status: runtime result is not a readable quote");
                reads.push(None);
            }
        }
    }
    let positions: Vec<LimitPosition<'_>> = book
        .positions
        .iter()
        .zip(&reads)
        .map(|(position, read)| LimitPosition {
            instrument_id: &position.instrument_id,
            quantity: position.quantity.decimal(),
            unit: position.unit,
            rating: position.rating.as_deref(),
            quote: read.as_ref().map(|r| &r.quote),
            terms: read.as_ref().map(|r| LimitTerms::of(&r.bond)),
        })
        .collect();
    let limit_book = LimitBook {
        currency: book
            .currency
            .as_deref()
            .and_then(convex_core::Currency::from_code),
        positions: &positions,
    };
    let evaluations = evaluate_limits(&limit_book, &set.limits).map_err(|error| {
        tracing::error!(%error, "limit status: evaluation refused a stored book and limit set");
        reference_error(ReferenceError::Unavailable)
    })?;
    let limits = set
        .limits
        .iter()
        .zip(&evaluations)
        .map(|(limit, evaluation)| StatusItem {
            measure: &limit.measure,
            lower: limit.lower,
            upper: limit.upper,
            warning_fraction: limit.warning_fraction,
            evaluation,
        })
        .collect();
    let view = StatusView {
        book_id: &id,
        book_version: book_record.version,
        limit_set_version: set_record.version,
        evaluated_at,
        snapshot_ids: snapshots.into_iter().collect(),
        limits,
    };
    Ok(Json(view).into_response())
}
