//! The desk's levels: `POST /api/v1/market/levels` marks the level of a bond quoted by a
//! yield or a spread, and `GET /api/v1/market/levels` reads every mark back.
//!
//! A mark is a `manual` quote on the desk's level source, the source the Live plan reads
//! those bonds' levels from. The quote ingress owns that source: it refuses a feed's event
//! on it, and stamps each mark's session, sequence and times under the lock that orders
//! every quote, so two pages marking at once cannot interleave. A Live runtime keeps no
//! quote across a restart, and neither does the ingress, so the marks it reads back are
//! the levels the runtime holds.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use convex_core::contracts::{ExactDecimal, UtcTimestamp};
use convex_core::ids::InstrumentId;
use convex_engine::level_pricing::check_level;
use convex_engine::ports::quote_event::{
    FieldUpdate, QuoteChange, QuoteEvent, QuoteEventKind, QuoteField, QuoteKey, QuoteQuality,
    QuoteValue,
};
use convex_engine::ports::reference_master::ReferenceLookup;
use convex_engine::ports::snapshot::QuoteRequirement;
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::handlers::AppState;
use crate::market::{accepted, ingress_error, rejected, QuoteIngress, RowError, MAX_BATCH};

/// The session every mark of this process is sent in.
const DESK_SESSION: &str = "desk";

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

/// The desk's level source and the level field of each bond the plan reads from it.
pub struct DeskLevels {
    source: String,
    fields: HashMap<InstrumentId, QuoteField>,
    /// The same bonds in instrument id order, as they are read back.
    order: Vec<InstrumentId>,
}

/// A mark as the ingress queued it, its level as sent.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct DeskMark {
    level: ExactDecimal,
    marked_at: UtcTimestamp,
}

impl DeskLevels {
    /// The bonds of the plan whose quotes come from `source`, each read by one yield or
    /// spread field. A plan with none, or with another quote on it, is refused: a typo
    /// would otherwise mark nothing, and a price on the source is not the desk's level.
    pub fn from_plan(source: &str, quotes: &[QuoteRequirement]) -> Result<Self, DeskLevelsError> {
        let mut fields = HashMap::new();
        for quote in quotes.iter().filter(|quote| quote.key.source == source) {
            let instrument_id = quote.key.instrument_id.clone();
            let field = match quote.fields.as_slice() {
                [field @ (QuoteField::MidYield | QuoteField::MidSpread)] => *field,
                _ => return Err(DeskLevelsError::NotALevel(instrument_id.as_str().into())),
            };
            fields.insert(instrument_id, field);
        }
        if fields.is_empty() {
            return Err(DeskLevelsError::Unused(source.into()));
        }
        let mut order: Vec<_> = fields.keys().cloned().collect();
        order.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        Ok(Self {
            source: source.into(),
            fields,
            order,
        })
    }

    /// The desk's level source.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The mark `level` as the desk's quote, numbered `sequence` and observed `at`.
    pub(crate) fn quote(&self, level: &DeskLevel, sequence: u64, at_ms: i64) -> QuoteEvent {
        QuoteEvent {
            schema_version: 1,
            key: QuoteKey {
                instrument_id: level.instrument_id.clone(),
                source: self.source.clone(),
            },
            session: DESK_SESSION.into(),
            sequence,
            source_time_ms: at_ms,
            receive_time_ms: at_ms,
            quality: QuoteQuality::Manual,
            entitled: true,
            entitlement_tags: Vec::new(),
            evidence_reference: None,
            event: QuoteEventKind::Image {
                fields: vec![QuoteChange {
                    field: level.field,
                    update: FieldUpdate::Set(QuoteValue {
                        value: level.level.decimal(),
                        unit: level.field.unit(),
                    }),
                }],
            },
        }
    }
}

impl DeskMark {
    /// The mark `level` records once queued at `marked_at`.
    pub(crate) fn queued(level: &DeskLevel, marked_at: UtcTimestamp) -> Self {
        Self {
            level: level.level,
            marked_at,
        }
    }
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
pub(crate) struct DeskLevel {
    pub(crate) instrument_id: InstrumentId,
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
    ingress: Option<Extension<Arc<QuoteIngress>>>,
    payload: Result<Json<DeskLevelBatch>, JsonRejection>,
) -> Response {
    let batch = match payload {
        Ok(Json(batch)) => batch,
        Err(rejection) => return ApiError::from(rejection).into_response(),
    };
    let Some((ingress, desk)) = ingress
        .as_ref()
        .and_then(|Extension(ingress)| Some((ingress, ingress.desk_levels()?)))
    else {
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
    let now = match crate::curves::known_at_or_now(None) {
        Ok(now) => now,
        Err(error) => return error.into_response(),
    };
    let lookup = ReferenceLookup {
        as_of: now,
        known_at: now,
    };
    let mut rows = Vec::new();
    for (index, level) in batch.levels.iter().enumerate() {
        match refusal(&state, desk, lookup, level).await {
            Ok(None) => {}
            Ok(Some((code, reason))) => rows.push(RowError {
                index,
                code,
                reason,
            }),
            Err(error) => return error.into_response(),
        }
    }
    if let Err(error) = rejected(rows) {
        return ingress_error(error);
    }
    match ingress.mark_desk(&batch.levels) {
        Ok(receipt) => accepted(receipt),
        Err(error) => ingress_error(error),
    }
}

/// Why `level` is refused, if it is: a bond the plan does not read from the desk, a field
/// other than the plan's, or a level outside the range of the method its rule in force
/// quotes it by, as a request pricing it would check.
async fn refusal(
    state: &AppState,
    desk: &DeskLevels,
    lookup: ReferenceLookup,
    level: &DeskLevel,
) -> Result<Option<(&'static str, &'static str)>, ApiError> {
    match desk.fields.get(&level.instrument_id) {
        None => {
            return Ok(Some((
                "NOT_DESK_QUOTED",
                "the plan does not read this bond's level from the desk",
            )))
        }
        Some(field) if *field != level.field => {
            return Ok(Some((
                "WRONG_LEVEL_FIELD",
                "the bond's rule quotes it by its other level field",
            )))
        }
        Some(_) => {}
    }
    let rule = state
        .engine
        .reference_repository()
        .pricing_rule_for_valuation(&level.instrument_id, lookup)
        .await
        .map_err(|_| ApiError::unavailable("the bond's rule could not be read"))?;
    let Some(method) = rule
        .map(|rule| rule.method)
        .filter(|method| method.level_field() == Some(level.field))
    else {
        return Ok(Some((
            "NOT_DESK_QUOTED",
            "the bond's rule in force does not quote it by a level",
        )));
    };
    Ok(check_level(&method, level.level.decimal()).err().map(|_| {
        (
            "LEVEL_OUT_OF_RANGE",
            "the level is outside its method's range",
        )
    }))
}

/// One bond as `GET /api/v1/market/levels` reads it back.
#[derive(Serialize)]
struct MarkedBond<'a> {
    instrument_id: &'a InstrumentId,
    field: QuoteField,
    mark: Option<&'a DeskMark>,
}

#[derive(Serialize)]
struct DeskLevelsReply<'a> {
    source: &'a str,
    levels: Vec<MarkedBond<'a>>,
}

/// `GET /api/v1/market/levels`: the desk's source and each bond's last mark in this
/// process, in instrument id order, `null` before its first.
pub(crate) async fn get_levels(ingress: Option<Extension<Arc<QuoteIngress>>>) -> Response {
    let Some((ingress, desk)) = ingress
        .as_ref()
        .and_then(|Extension(ingress)| Some((ingress, ingress.desk_levels()?)))
    else {
        return ApiError::unavailable("this server takes no desk levels").into_response();
    };
    let marks = match ingress.desk_marks() {
        Ok(marks) => marks,
        Err(error) => return ingress_error(error),
    };
    let levels = desk
        .order
        .iter()
        .filter_map(|instrument_id| {
            Some(MarkedBond {
                instrument_id,
                field: *desk.fields.get(instrument_id)?,
                mark: marks.get(instrument_id),
            })
        })
        .collect();
    Json(DeskLevelsReply {
        source: &desk.source,
        levels,
    })
    .into_response()
}
