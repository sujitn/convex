//! Limit acknowledgements (P15c; D94, D95, D97).
//!
//! A risk manager acknowledges one limit of a book while it is `breach` or `unknown`: the
//! acknowledgement records who has seen it, until when (at most seven days ahead) and why,
//! and changes no bound. A book's acknowledgements are one
//! `MasterKey::LimitAcknowledgements` record per book in the reference repository, as its
//! limit set is: compare-and-set versions, history, idempotency receipts and backup. Each
//! version changes one limit's entry, so these writes take the limit write lock, under
//! which two subjects acknowledging different limits both keep theirs.
//!
//! An entry is in force until it expires, unless it is withdrawn or the book's current
//! set no longer holds the limit exactly as acknowledged. Entries no longer in force are
//! dropped at the record's next version; the history keeps them. Status, the pre-trade
//! check, records and `limit_status` frames carry the entry in force for each `breach` or
//! `unknown` limit; an acknowledgement records and publishes nothing of its own (D97).
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
// Internal early-return errors are the finished HTTP response itself.
#![allow(clippy::result_large_err)]

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use convex_core::contracts::{RequestId, UtcTimestamp, Version};
use convex_engine::ports::limits::{
    acknowledgement_expiry, acknowledgement_note, LimitAcknowledgement, LimitAcknowledgementChange,
    LimitAcknowledgementsDefinition, LimitDefinition, LimitId, LimitStatus,
};
use convex_engine::ports::reference_master::*;
use convex_engine::ports::reference_repository::*;
use convex_engine::ports::storage::AuditStore;
use serde::{Deserialize, Serialize};

use crate::access::{recorded_subject, AccessContext};
use crate::books::{book_id, book_of, typed_error};
use crate::handlers::AppState;
use crate::limit_monitor::{LIMIT_STATUS_ENTITY, LIMIT_STATUS_EVENT};
use crate::limits::{
    authorize_read, authorize_write, commit, commit_time, conflict, current, different_payload,
    evaluate_now, evidence, read_master, retained, set_of, HistoryQuery, MAX_HISTORY_PAGE,
    STATUS_DATA,
};
use crate::reference::{key, now, reference_error, with_reference_version};

pub(crate) fn acknowledgements_of_record(
    record: &MasterRecord,
) -> Result<&LimitAcknowledgementsDefinition, ReferenceError> {
    match &record.definition {
        MasterDefinition::LimitAcknowledgements(record) => Ok(record),
        _ => Err(ReferenceError::Unsupported("not limit acknowledgements")),
    }
}

// =============================================================================
// VIEWS
// =============================================================================

#[derive(Serialize)]
struct EntryView<'a> {
    limit_id: &'a LimitId,
    #[serde(flatten)]
    entry: &'a LimitAcknowledgement,
}

#[derive(Serialize)]
struct RecordView<'a> {
    book_id: &'a BookId,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<Version>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recorded_at: Option<UtcTimestamp>,
    /// What this version changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    change: Option<&'a LimitAcknowledgementChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    evidence_reference: Option<&'a str>,
    /// For the current acknowledgements: the time they are in force at.
    #[serde(skip_serializing_if = "Option::is_none")]
    in_force_at: Option<UtcTimestamp>,
    acknowledgements: Vec<EntryView<'a>>,
}

fn entries<'a>(list: impl IntoIterator<Item = &'a LimitAcknowledgement>) -> Vec<EntryView<'a>> {
    list.into_iter()
        .map(|entry| EntryView {
            limit_id: &entry.limit.limit_id,
            entry,
        })
        .collect()
}

/// One stored version, as written.
fn version_response(record: &MasterRecord, status: StatusCode) -> Response {
    let stored = match acknowledgements_of_record(record) {
        Ok(stored) => stored,
        Err(error) => return reference_error(error),
    };
    let view = RecordView {
        book_id: &stored.book_id,
        version: Some(record.version),
        recorded_at: Some(record.recorded_at),
        change: Some(&stored.change),
        evidence_reference: Some(&stored.provenance.evidence_reference),
        in_force_at: None,
        acknowledgements: entries(&stored.acknowledgements),
    };
    with_reference_version((status, Json(view)).into_response(), record.version)
}

// =============================================================================
// WRITES
// =============================================================================

/// Whether `status` may be acknowledged (D95).
fn acknowledgeable(status: LimitStatus) -> bool {
    matches!(status, LimitStatus::Breach | LimitStatus::Unknown)
}

/// The book's entries still in force at `at` under `limits`, without `limit_id`'s: what a
/// new version keeps.
fn kept(
    existing: Option<&MasterRecord>,
    limit_id: &LimitId,
    at: UtcTimestamp,
    limits: &[LimitDefinition],
) -> Result<Vec<LimitAcknowledgement>, ReferenceError> {
    let Some(record) = existing else {
        return Ok(Vec::new());
    };
    Ok(acknowledgements_of_record(record)?
        .acknowledgements
        .iter()
        .filter(|entry| entry.limit.limit_id != *limit_id && entry.in_force(at, limits))
        .cloned()
        .collect())
}

async fn write(
    state: &AppState,
    idempotency_key: RequestId,
    existing: Option<&MasterRecord>,
    definition: LimitAcknowledgementsDefinition,
    at: UtcTimestamp,
) -> Result<MasterRecord, Response> {
    let change = MasterChange {
        expected_version: existing.map(|record| record.version),
        definition: MasterDefinition::LimitAcknowledgements(Box::new(definition)),
    };
    commit(state, idempotency_key, vec![change], at)
        .await
        .map_err(reference_error)?
        .into_iter()
        .next()
        .ok_or_else(|| reference_error(ReferenceError::Unavailable))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AcknowledgeRequest {
    limit_id: LimitId,
    expires_at: UtcTimestamp,
    note: String,
    #[serde(default)]
    record_id: Option<u64>,
}

/// `POST /api/v1/books/{book_id}/limits/acknowledgements`: acknowledge one limit of the
/// current set while it is `breach` or `unknown` (D95).
///
/// `Idempotency-Key` is required; there is no `If-Match`, since each acknowledgement
/// replaces only its own limit's entry. The caller holds status's read rule on the book
/// and `LimitWrite` on the book and on each instrument the limit names. The limit,
/// evaluated now, must be `breach` or `unknown` (409 `LIMIT_NOT_ACKNOWLEDGEABLE`). The
/// expiry is after the acknowledgement and at most seven days after it. A `record_id`,
/// when given, must be a recorded status change of this book and limit (422
/// `LIMIT_RECORD_MISMATCH`). A new acknowledgement of a limit replaces the one in force.
/// Answers `201` with the version written.
pub(crate) async fn acknowledge_limit(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    audit: Option<Extension<Arc<dyn AuditStore>>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<AcknowledgeRequest>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = payload else {
        return reference_error(ReferenceError::InvalidInput("invalid acknowledgement JSON"));
    };
    let (id, idempotency_key) = match (book_id(raw), key(&headers)) {
        (Ok(id), Ok(idempotency_key)) => (id, idempotency_key),
        (Err(error), _) | (_, Err(error)) => return reference_error(error),
    };
    if let Err(error) = acknowledgement_note(&request.note) {
        return reference_error(error);
    }
    if request.record_id == Some(0) {
        return reference_error(ReferenceError::InvalidInput("a record id is at least 1"));
    }
    let subject = recorded_subject(&ctx);
    let _writes = state.limit_writes.lock().await;
    match acknowledge(&state, &ctx, audit, id, idempotency_key, request, subject).await {
        Ok(response) | Err(response) => response,
    }
}

async fn acknowledge(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    audit: Option<Extension<Arc<dyn AuditStore>>>,
    id: BookId,
    idempotency_key: RequestId,
    request: AcknowledgeRequest,
    subject: String,
) -> Result<Response, Response> {
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let set_record = read_master(state, &MasterKey::LimitSet(id.clone()), None).await?;
    let set = set_of(&set_record).map_err(reference_error)?;
    let book_record = authorize_read(state, ctx, STATUS_DATA, &id, &set.limits)
        .await?
        .ok_or_else(|| reference_error(ReferenceError::NotFound))?;
    let Some((index, limit)) = set
        .limits
        .iter()
        .enumerate()
        .find(|(_, limit)| limit.limit_id == request.limit_id)
    else {
        return Err(typed_error(
            StatusCode::NOT_FOUND,
            "LIMIT_NOT_FOUND",
            "the book's current limit set has no limit with this id",
        ));
    };
    authorize_write(state, ctx, &id, std::slice::from_ref(limit))?;
    let record_key = MasterKey::LimitAcknowledgements(id.clone());
    // An exact retry returns the retained version; any other reuse of the key conflicts.
    if let Some(record) = retained(repo, &idempotency_key, &record_key)
        .await
        .map_err(reference_error)?
    {
        let stored = acknowledgements_of_record(&record).map_err(reference_error)?;
        let made = matches!(&stored.change, LimitAcknowledgementChange::Acknowledged { limit_id } if *limit_id == request.limit_id);
        let same = made
            && stored.entry(&request.limit_id).is_some_and(|entry| {
                entry.acknowledged_by == subject
                    && entry.expires_at == request.expires_at
                    && entry.note == request.note
                    && entry.record_id == request.record_id
            });
        if !same {
            return Err(reference_error(different_payload()));
        }
        return Ok(version_response(&record, StatusCode::CREATED));
    }
    let existing = current(repo, record_key).await.map_err(reference_error)?;
    let at = commit_time(existing.iter().map(|r| r.recorded_at)).map_err(reference_error)?;
    acknowledgement_expiry(at, request.expires_at).map_err(reference_error)?;
    let book = book_of(&book_record).map_err(reference_error)?;
    let evaluation = evaluate_now(state, book, &set.limits).await?;
    let status = evaluation
        .evaluations
        .get(index)
        .map(|evaluation| evaluation.status)
        .ok_or_else(|| reference_error(ReferenceError::Unavailable))?;
    if !acknowledgeable(status) {
        return Err(conflict(
            "LIMIT_NOT_ACKNOWLEDGEABLE",
            "a limit is acknowledged only while it is in breach or unknown",
        ));
    }
    if let Some(record_id) = request.record_id {
        recorded_change(audit, &id, &request.limit_id, record_id).await?;
    }
    let mut acknowledgements =
        kept(existing.as_ref(), &request.limit_id, at, &set.limits).map_err(reference_error)?;
    acknowledgements.push(LimitAcknowledgement {
        limit: limit.clone(),
        limit_set_version: set_record.version,
        acknowledged_by: subject,
        acknowledged_at: at,
        expires_at: request.expires_at,
        note: request.note,
        record_id: request.record_id,
    });
    // In set order, so a version reads as the set does.
    acknowledgements.sort_by_key(|entry| {
        set.limits
            .iter()
            .position(|limit| limit.limit_id == entry.limit.limit_id)
    });
    let definition = LimitAcknowledgementsDefinition {
        book_id: id,
        acknowledgements,
        change: LimitAcknowledgementChange::Acknowledged {
            limit_id: request.limit_id,
        },
        effective: EffectiveInterval {
            from: at,
            until: None,
        },
        provenance: evidence(&idempotency_key).map_err(reference_error)?,
    };
    let record = write(state, idempotency_key, existing.as_ref(), definition, at).await?;
    Ok(version_response(&record, StatusCode::CREATED))
}

/// What a record must name to be evidence for an acknowledgement.
#[derive(Deserialize)]
struct RecordedLimit {
    limit_id: LimitId,
}

/// `record_id` must be a recorded status change of `book`'s `limit_id`: one read of the
/// book's index (P15b). Without a monitor there is no store to read.
async fn recorded_change(
    audit: Option<Extension<Arc<dyn AuditStore>>>,
    book: &BookId,
    limit_id: &LimitId,
    record_id: u64,
) -> Result<(), Response> {
    let Some(Extension(audit)) = audit else {
        return Err(crate::error::ApiError::unavailable(
            "this server does not monitor limits, so a record id cannot be checked",
        )
        .into_response());
    };
    let page = audit
        .entity_page(
            LIMIT_STATUS_ENTITY,
            book.as_str(),
            record_id.saturating_sub(1),
            1,
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, book = %book, "limit acknowledgements: the audit store refused a read");
            reference_error(ReferenceError::Unavailable)
        })?;
    let names = page.entries.first().is_some_and(|entry| {
        entry.id == record_id
            && entry.event_type == LIMIT_STATUS_EVENT
            && entry
                .details
                .as_deref()
                .and_then(|details| serde_json::from_str::<RecordedLimit>(details).ok())
                .is_some_and(|recorded| recorded.limit_id == *limit_id)
    });
    if names {
        Ok(())
    } else {
        Err(typed_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "LIMIT_RECORD_MISMATCH",
            "record_id names no recorded status change of this book's limit",
        ))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WithdrawRequest {
    limit_id: LimitId,
}

/// `POST /api/v1/books/{book_id}/limits/acknowledgements/withdraw`: end the
/// acknowledgement in force for one limit, recording who ended it.
///
/// The body names the limit. `Idempotency-Key` is required. The caller needs what
/// acknowledging needs: status's read rule on the book, and `LimitWrite` on the book and
/// each instrument the acknowledged limit names. Any such subject may withdraw it, not
/// only its maker. Answers `200` with the version written, or 404
/// `LIMIT_ACKNOWLEDGEMENT_NOT_FOUND` when none is in force.
pub(crate) async fn withdraw_acknowledgement(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<WithdrawRequest>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = payload else {
        return reference_error(ReferenceError::InvalidInput("invalid withdrawal JSON"));
    };
    let (id, idempotency_key) = match (book_id(raw), key(&headers)) {
        (Ok(id), Ok(idempotency_key)) => (id, idempotency_key),
        (Err(error), _) | (_, Err(error)) => return reference_error(error),
    };
    let subject = recorded_subject(&ctx);
    let _writes = state.limit_writes.lock().await;
    match withdraw(&state, &ctx, id, idempotency_key, request.limit_id, subject).await {
        Ok(response) | Err(response) => response,
    }
}

async fn withdraw(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    id: BookId,
    idempotency_key: RequestId,
    limit_id: LimitId,
    subject: String,
) -> Result<Response, Response> {
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let set_record = current(repo, MasterKey::LimitSet(id.clone()))
        .await
        .map_err(reference_error)?;
    let limits = match &set_record {
        Some(record) => set_of(record).map_err(reference_error)?.limits.clone(),
        None => Vec::new(),
    };
    let record_key = MasterKey::LimitAcknowledgements(id.clone());
    let existing = current(repo, record_key.clone())
        .await
        .map_err(reference_error)?;
    let stored = existing
        .as_ref()
        .map(acknowledgements_of_record)
        .transpose()
        .map_err(reference_error)?;
    // The limit as acknowledged, or as the set holds it now.
    let target: Option<LimitDefinition> = stored
        .and_then(|stored| stored.entry(&limit_id))
        .map(|entry| entry.limit.clone())
        .or_else(|| {
            limits
                .iter()
                .find(|limit| limit.limit_id == limit_id)
                .cloned()
        });
    let mut named = limits.clone();
    named.extend(target.clone());
    authorize_read(state, ctx, STATUS_DATA, &id, &named)
        .await?
        .ok_or_else(|| reference_error(ReferenceError::NotFound))?;
    authorize_write(state, ctx, &id, target.as_slice())?;
    if let Some(record) = retained(repo, &idempotency_key, &record_key)
        .await
        .map_err(reference_error)?
    {
        let stored = acknowledgements_of_record(&record).map_err(reference_error)?;
        let same = matches!(&stored.change, LimitAcknowledgementChange::Withdrawn { limit_id: withdrawn, by } if *withdrawn == limit_id && *by == subject);
        if !same {
            return Err(reference_error(different_payload()));
        }
        return Ok(version_response(&record, StatusCode::OK));
    }
    let at = commit_time(existing.iter().map(|r| r.recorded_at)).map_err(reference_error)?;
    let in_force = stored
        .and_then(|stored| stored.entry(&limit_id))
        .is_some_and(|entry| entry.in_force(at, &limits));
    if !in_force {
        return Err(typed_error(
            StatusCode::NOT_FOUND,
            "LIMIT_ACKNOWLEDGEMENT_NOT_FOUND",
            "no acknowledgement of this limit is in force",
        ));
    }
    let acknowledgements =
        kept(existing.as_ref(), &limit_id, at, &limits).map_err(reference_error)?;
    let definition = LimitAcknowledgementsDefinition {
        book_id: id,
        acknowledgements,
        change: LimitAcknowledgementChange::Withdrawn {
            limit_id,
            by: subject,
        },
        effective: EffectiveInterval {
            from: at,
            until: None,
        },
        provenance: evidence(&idempotency_key).map_err(reference_error)?,
    };
    let record = write(state, idempotency_key, existing.as_ref(), definition, at).await?;
    Ok(version_response(&record, StatusCode::OK))
}

// =============================================================================
// READS
// =============================================================================

/// The book's current set's limits, and the read rule on them and on `also`.
async fn readable(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    id: &BookId,
    also: &[LimitDefinition],
) -> Result<Vec<LimitDefinition>, Response> {
    let set_record = current(
        state.engine.reference_repository().as_ref(),
        MasterKey::LimitSet(id.clone()),
    )
    .await
    .map_err(reference_error)?;
    let limits = match &set_record {
        Some(record) => set_of(record).map_err(reference_error)?.limits.clone(),
        None => Vec::new(),
    };
    let mut named = limits.clone();
    named.extend_from_slice(also);
    authorize_read(state, ctx, STATUS_DATA, id, &named)
        .await?
        .ok_or_else(|| reference_error(ReferenceError::NotFound))?;
    Ok(limits)
}

/// `GET /api/v1/books/{book_id}/limits/acknowledgements`: the acknowledgements in force
/// now, under status's read rule, with the current version in `ETag`. A book with none has
/// an empty list, and no version before its first.
pub(crate) async fn get_acknowledgements(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
) -> Response {
    let read = async {
        let id = book_id(raw).map_err(reference_error)?;
        let limits = readable(&state, &ctx, &id, &[]).await?;
        let record = current(
            state.engine.reference_repository().as_ref(),
            MasterKey::LimitAcknowledgements(id.clone()),
        )
        .await
        .map_err(reference_error)?;
        let at = now().map_err(reference_error)?;
        let Some(record) = record else {
            let view = RecordView {
                book_id: &id,
                version: None,
                recorded_at: None,
                change: None,
                evidence_reference: None,
                in_force_at: Some(at),
                acknowledgements: Vec::new(),
            };
            return Ok(Json(view).into_response());
        };
        let stored = acknowledgements_of_record(&record).map_err(reference_error)?;
        // In force entries name only limits of the current set, which the read rule covers.
        let view = RecordView {
            book_id: &stored.book_id,
            version: Some(record.version),
            recorded_at: Some(record.recorded_at),
            change: Some(&stored.change),
            evidence_reference: Some(&stored.provenance.evidence_reference),
            in_force_at: Some(at),
            acknowledgements: entries(
                stored
                    .acknowledgements
                    .iter()
                    .filter(|entry| entry.in_force(at, &limits)),
            ),
        };
        Ok::<_, Response>(with_reference_version(
            Json(view).into_response(),
            record.version,
        ))
    };
    match read.await {
        Ok(response) | Err(response) => response,
    }
}

/// `GET /api/v1/books/{book_id}/limits/acknowledgements/versions/{version}`: one version
/// as written, entries no longer in force included. The read rule also covers each
/// instrument its entries' limits name.
pub(crate) async fn get_acknowledgements_version(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path((raw, version)): Path<(String, u64)>,
) -> Response {
    let read = async {
        let id = book_id(raw).map_err(reference_error)?;
        let version = Version::new(version)
            .map_err(|_| reference_error(ReferenceError::InvalidInput("invalid version")))?;
        let record = read_master(
            &state,
            &MasterKey::LimitAcknowledgements(id.clone()),
            Some(version),
        )
        .await?;
        let stored = acknowledgements_of_record(&record).map_err(reference_error)?;
        let named: Vec<LimitDefinition> = stored
            .acknowledgements
            .iter()
            .map(|entry| entry.limit.clone())
            .collect();
        readable(&state, &ctx, &id, &named).await?;
        Ok::<_, Response>(version_response(&record, StatusCode::OK))
    };
    match read.await {
        Ok(response) | Err(response) => response,
    }
}

/// `GET /api/v1/books/{book_id}/limits/acknowledgements/history`: version summaries from
/// `from`, oldest first, under status's read rule: what each changed, who changed it and
/// how many entries it holds, not the entries.
pub(crate) async fn get_acknowledgements_history(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    Path(raw): Path<String>,
    query: Result<Query<HistoryQuery>, QueryRejection>,
) -> Response {
    let read = async {
        let query = match query {
            Ok(Query(query)) if query.limit > 0 && query.limit <= MAX_HISTORY_PAGE => query,
            _ => {
                return Err(reference_error(ReferenceError::InvalidInput(
                    "invalid history query",
                )))
            }
        };
        let id = book_id(raw).map_err(reference_error)?;
        let from = Version::new(query.from)
            .map_err(|_| reference_error(ReferenceError::InvalidInput("invalid version")))?;
        readable(&state, &ctx, &id, &[]).await?;
        let key = MasterKey::LimitAcknowledgements(id);
        let repo = state.engine.reference_repository();
        let current = repo
            .get_master(&key)
            .await
            .map_err(reference_error)?
            .ok_or_else(|| reference_error(ReferenceError::NotFound))?
            .version;
        let last = from
            .get()
            .saturating_add(query.limit as u64 - 1)
            .min(current.get());
        let mut versions = Vec::new();
        for number in from.get()..=last {
            let version =
                Version::new(number).map_err(|_| reference_error(ReferenceError::Unavailable))?;
            let record = repo
                .get_master_version(&key, version)
                .await
                .map_err(reference_error)?
                .ok_or_else(|| reference_error(ReferenceError::Unavailable))?;
            let stored = acknowledgements_of_record(&record).map_err(reference_error)?;
            let (limit_id, by) = match &stored.change {
                LimitAcknowledgementChange::Acknowledged { limit_id } => (
                    limit_id,
                    stored
                        .entry(limit_id)
                        .map(|entry| entry.acknowledged_by.as_str()),
                ),
                LimitAcknowledgementChange::Withdrawn { limit_id, by } => {
                    (limit_id, Some(by.as_str()))
                }
            };
            let action = match &stored.change {
                LimitAcknowledgementChange::Acknowledged { .. } => "acknowledged",
                LimitAcknowledgementChange::Withdrawn { .. } => "withdrawn",
            };
            versions.push(serde_json::json!({
                "version": record.version,
                "recorded_at": record.recorded_at,
                "action": action,
                "limit_id": limit_id,
                "by": by,
                "acknowledgement_count": stored.acknowledgements.len(),
            }));
        }
        let next_from = (last < current.get()).then_some(last + 1);
        Ok(Json(serde_json::json!({
            "versions": versions,
            "current_version": current,
            "next_from": next_from,
        }))
        .into_response())
    };
    match read.await {
        Ok(response) | Err(response) => response,
    }
}
