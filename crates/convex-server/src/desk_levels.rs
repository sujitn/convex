//! The desk's levels: `POST /api/v1/market/levels` marks the level of a bond quoted by a
//! yield or a spread, and `GET /api/v1/market/levels` reads every mark back.
//!
//! A mark is a `manual` quote on the desk's level source, the source the Live plan reads
//! those bonds' levels from. This route is that source's only writer: the quote ingress
//! refuses a feed's event on it. The route numbers each bond's quotes in one session per
//! process, so two pages marking at once cannot interleave. A Live runtime keeps no quote
//! across a restart, and neither does this route, so the marks it reads back are the
//! levels the runtime holds.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use convex_core::contracts::{ExactDecimal, UtcTimestamp};
use convex_core::ids::InstrumentId;
use convex_engine::ports::quote_event::{
    FieldUpdate, QuoteChange, QuoteEvent, QuoteEventKind, QuoteField, QuoteKey, QuoteQuality,
    QuoteValue,
};
use convex_engine::ports::reference_master::ReferenceLookup;
use convex_engine::ports::snapshot::QuoteRequirement;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::ApiError;
use crate::handlers::AppState;
use crate::market::{IngressError, QuoteIngress, RowError, MAX_BATCH};

/// The session every mark of this process is sent in.
const DESK_SESSION: &str = "desk";
/// Refused marks reported per response.
const MAX_ROW_ERRORS: usize = 64;

/// Why a plan cannot take the desk's levels.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeskLevelsError {
    /// No bond of the plan reads its level from the source.
    #[error("no bond of the plan reads its level from desk_level_source {0}")]
    Unused(String),
    /// A quote on the source reads something other than one level.
    #[error("the plan reads {0} from desk_level_source as more than one yield or spread")]
    NotALevel(String),
}

/// One bond the desk marks: its level's field, and the marks this process sent.
struct DeskBond {
    field: QuoteField,
    /// The last sequence sent; 0 before the first mark.
    sequence: u64,
    last: Option<DeskMark>,
}

/// A mark as queued for the runtime, its level as sent.
#[derive(Clone, Serialize)]
struct DeskMark {
    level: ExactDecimal,
    marked_at: UtcTimestamp,
}

/// The desk's level source and the bonds whose levels it carries.
pub struct DeskLevels {
    source: String,
    bonds: Mutex<HashMap<InstrumentId, DeskBond>>,
}

impl DeskLevels {
    /// The bonds of the plan whose quotes come from `source`, each read by one yield or
    /// spread field. A plan with none, or with another quote on it, is refused: a typo
    /// would otherwise mark nothing, and a price on the source is not the desk's level.
    pub fn from_plan(source: &str, quotes: &[QuoteRequirement]) -> Result<Self, DeskLevelsError> {
        let mut bonds = HashMap::new();
        for quote in quotes.iter().filter(|quote| quote.key.source == source) {
            let instrument_id = quote.key.instrument_id.clone();
            let field = match quote.fields.as_slice() {
                [field @ (QuoteField::MidYield | QuoteField::MidSpread)] => *field,
                _ => return Err(DeskLevelsError::NotALevel(instrument_id.as_str().into())),
            };
            bonds.insert(
                instrument_id,
                DeskBond {
                    field,
                    sequence: 0,
                    last: None,
                },
            );
        }
        if bonds.is_empty() {
            return Err(DeskLevelsError::Unused(source.into()));
        }
        Ok(Self {
            source: source.into(),
            bonds: Mutex::new(bonds),
        })
    }

    /// The desk's level source.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Queue `marks` as one batch, numbering each bond's next quote, or queue nothing.
    /// The lock is held across the submit, so sequences follow queue order.
    fn mark(
        &self,
        ingress: &QuoteIngress,
        marks: &[(InstrumentId, QuoteField, ExactDecimal)],
    ) -> Result<(usize, i64), MarkError> {
        let mut bonds = self.bonds.lock().map_err(|_| MarkError::Unavailable)?;
        let mut events = Vec::with_capacity(marks.len());
        for (instrument_id, field, level) in marks {
            let bond = bonds.get(instrument_id).ok_or(MarkError::Unavailable)?;
            events.push(QuoteEvent {
                schema_version: 1,
                key: QuoteKey {
                    instrument_id: instrument_id.clone(),
                    source: self.source.clone(),
                },
                session: DESK_SESSION.into(),
                sequence: bond.sequence + 1,
                source_time_ms: 0,
                receive_time_ms: 0,
                quality: QuoteQuality::Manual,
                entitled: true,
                entitlement_tags: Vec::new(),
                evidence_reference: None,
                event: QuoteEventKind::Image {
                    fields: vec![QuoteChange {
                        field: *field,
                        update: FieldUpdate::Set(QuoteValue {
                            value: level.decimal(),
                            unit: field.unit(),
                        }),
                    }],
                },
            });
        }
        let receipt = ingress.submit_desk(events).map_err(MarkError::Ingress)?;
        let marked_at = UtcTimestamp::from_datetime(
            chrono::DateTime::from_timestamp_millis(receipt.receive_time_ms)
                .ok_or(MarkError::Unavailable)?,
        )
        .map_err(|_| MarkError::Unavailable)?;
        for (instrument_id, _, level) in marks {
            if let Some(bond) = bonds.get_mut(instrument_id) {
                bond.sequence += 1;
                bond.last = Some(DeskMark {
                    level: *level,
                    marked_at,
                });
            }
        }
        Ok((receipt.accepted, receipt.receive_time_ms))
    }
}

enum MarkError {
    Ingress(IngressError),
    Unavailable,
}

/// `POST /api/v1/market/levels` body.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeskLevelBatch {
    /// The desk's level source, named so access scopes the write to it.
    source: String,
    /// 1 through [`MAX_BATCH`] marks, one a bond.
    levels: Vec<DeskLevel>,
}

/// One bond's level, absolute: sending it twice marks it twice at the same level.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeskLevel {
    instrument_id: InstrumentId,
    /// The rule's field, `mid_yield` or `mid_spread`, so a level in the wrong unit is
    /// refused rather than read as the other.
    field: QuoteField,
    /// A decimal yield (0.0525) or basis points of spread, as the field reads.
    level: ExactDecimal,
}

/// Mark the desk's levels for the Live runtime. 202 means queued, not yet priced: each
/// bond's result arrives on the WebSocket. Each level is checked against its bond's rule
/// in force now: a bond the desk does not quote, a field other than its rule's, or a
/// level outside its method's range refuses the whole batch with 422 `QUOTES_REJECTED`.
pub(crate) async fn post_levels(
    State(state): State<Arc<AppState>>,
    desk: Option<Extension<Arc<DeskLevels>>>,
    ingress: Option<Extension<Arc<QuoteIngress>>>,
    payload: Result<Json<DeskLevelBatch>, JsonRejection>,
) -> Response {
    let batch = match payload {
        Ok(Json(batch)) => batch,
        Err(rejection) => return ApiError::invalid(rejection.body_text()).into_response(),
    };
    let (Some(Extension(desk)), Some(Extension(ingress))) = (desk, ingress) else {
        return ApiError::unavailable("this server takes no desk levels").into_response();
    };
    if batch.source != desk.source {
        return ApiError::invalid("levels are marked on the desk's level source only")
            .into_response();
    }
    if batch.levels.is_empty() || batch.levels.len() > MAX_BATCH {
        return ApiError::invalid(format!("a batch marks 1 to {MAX_BATCH} levels")).into_response();
    }
    let mut seen = HashSet::new();
    if !batch
        .levels
        .iter()
        .all(|level| seen.insert(&level.instrument_id))
    {
        return ApiError::invalid("a batch marks each bond once").into_response();
    }
    let fields: HashMap<InstrumentId, QuoteField> = match desk.bonds.lock() {
        Ok(bonds) => bonds
            .iter()
            .map(|(id, bond)| (id.clone(), bond.field))
            .collect(),
        Err(_) => return ApiError::unavailable("desk levels are unavailable").into_response(),
    };
    let now = match UtcTimestamp::from_datetime(chrono::Utc::now()) {
        Ok(now) => now,
        Err(_) => return ApiError::unavailable("the server clock is unreadable").into_response(),
    };
    let lookup = ReferenceLookup {
        as_of: now,
        known_at: now,
    };
    let mut rows = Vec::new();
    let mut refused_count = 0usize;
    let mut marks = Vec::with_capacity(batch.levels.len());
    for (index, level) in batch.levels.iter().enumerate() {
        let refusal = match fields.get(&level.instrument_id) {
            None => Some((
                "NOT_DESK_QUOTED",
                "the plan does not read this bond's level from the desk",
            )),
            Some(field) if *field != level.field => Some((
                "WRONG_LEVEL_FIELD",
                "the bond's rule quotes it by its other level field",
            )),
            Some(_) => match in_range(&state, &level.instrument_id, lookup, level).await {
                Ok(()) => None,
                Err(LevelCheck::OutOfRange) => Some((
                    "LEVEL_OUT_OF_RANGE",
                    "the level is outside its method's range",
                )),
                Err(LevelCheck::NoRule) => Some((
                    "NOT_DESK_QUOTED",
                    "the bond's rule in force does not quote it by a level",
                )),
                Err(LevelCheck::Unavailable) => {
                    return ApiError::unavailable("the bond's rule could not be read")
                        .into_response()
                }
            },
        };
        match refusal {
            Some((code, reason)) => {
                refused_count += 1;
                if rows.len() < MAX_ROW_ERRORS {
                    rows.push(RowError {
                        index,
                        code,
                        reason,
                    });
                }
            }
            None => marks.push((level.instrument_id.clone(), level.field, level.level)),
        }
    }
    if refused_count > 0 {
        return crate::market::ingress_error(IngressError::Rejected {
            truncated: refused_count > rows.len(),
            rows,
        });
    }
    match desk.mark(&ingress, &marks) {
        Ok((accepted, receive_time_ms)) => (
            StatusCode::ACCEPTED,
            Json(json!({"accepted": accepted, "receive_time_ms": receive_time_ms})),
        )
            .into_response(),
        Err(MarkError::Ingress(error)) => crate::market::ingress_error(error),
        Err(MarkError::Unavailable) => {
            ApiError::unavailable("desk levels are unavailable").into_response()
        }
    }
}

enum LevelCheck {
    OutOfRange,
    NoRule,
    Unavailable,
}

/// Whether `level` is within the range of the method the bond's rule in force quotes it
/// by, as a request pricing it would check.
async fn in_range(
    state: &AppState,
    instrument_id: &InstrumentId,
    lookup: ReferenceLookup,
    level: &DeskLevel,
) -> Result<(), LevelCheck> {
    let reference = state
        .engine
        .reference_at_for_pricing(instrument_id, lookup)
        .await
        .map_err(|_| LevelCheck::Unavailable)?
        .ok_or(LevelCheck::NoRule)?;
    let method = reference
        .pricing_rule
        .map(|rule| rule.method)
        .ok_or(LevelCheck::NoRule)?;
    if method.level_field() != Some(level.field) {
        return Err(LevelCheck::NoRule);
    }
    convex_engine::level_pricing::check_level(&method, level.level.decimal())
        .map_err(|_| LevelCheck::OutOfRange)
}

/// `GET /api/v1/market/levels`: the desk's source and each bond's last mark in this
/// process, `null` before its first.
pub(crate) async fn get_levels(desk: Option<Extension<Arc<DeskLevels>>>) -> Response {
    let Some(Extension(desk)) = desk else {
        return ApiError::unavailable("this server takes no desk levels").into_response();
    };
    let Ok(bonds) = desk.bonds.lock() else {
        return ApiError::unavailable("desk levels are unavailable").into_response();
    };
    let mut levels: Vec<_> = bonds.iter().collect();
    levels.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
    let levels: Vec<_> = levels
        .into_iter()
        .map(|(instrument_id, bond)| {
            json!({
                "instrument_id": instrument_id,
                "field": bond.field,
                "mark": bond.last,
            })
        })
        .collect();
    Json(json!({"source": desk.source, "levels": levels})).into_response()
}
