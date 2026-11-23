//! Clients' requests for quote over HTTP.
//!
//! - `POST /api/v1/rfqs`: a source sends a batch of requests. Each is answered on its
//!   own: received and priced, a retry of one already received, or refused with a code.
//!   The reply is 200 when any was taken, 429 when none was and one was refused for
//!   capacity, and 422 otherwise.
//! - `POST /api/v1/rfqs/:rfq_key/quote` and `/pass`: a trader answers.
//! - `POST /api/v1/rfqs/:rfq_key/outcome`: the source reports the client's outcome or a
//!   cancellation.
//! - `GET /api/v1/rfqs`: the open requests, by deadline; `GET /api/v1/rfqs/:rfq_key`: one
//!   request's entries.
//!
//! A request is priced at receipt under the active configuration: the bond's Live level
//! through its rule's stage, at the width of the rule for the trade's size scaled by the
//! client tier's factor, on the side of the desk's quote the client trades on. A bond with
//! no two-way width is priced at the mid, flagged, and one that cannot be priced is still
//! priced, with the reason, for the trader to set the price. Each is checked against its
//! book's limits as if done.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use convex_core::contracts::{DataQuality, ExactDecimal, InstrumentId, UtcTimestamp};
use convex_engine::level_pricing::{
    live_price, live_quote_input, BidAsk, QuotedInput, RulePricing, TradeSize,
};
use convex_engine::ports::level_pricing::PricingMethod;
use convex_engine::ports::quote_event::QuoteField;
use convex_engine::ports::reference_master::{
    BookId, BookPosition, MasterKey, MasterRecord, QuantityUnit, ReferenceLookup,
    ResolvedBondReference,
};
use convex_engine::ports::reference_repository::ReferenceRepository;
use convex_engine::ports::rfq::{
    DeskSide, RfqBatch, RfqEvent, RfqRequest, RfqState, RfqTransitionError, SourceOutcome,
    MAX_RFQ_BATCH,
};
use convex_engine::ports::storage::MAX_ENTITY_PAGE;
use convex_engine::{PricingEngine, RequestContext};
use serde::{Deserialize, Serialize};

use crate::access::{AccessContext, Action, DataScope, Resource, ResourceKind};
use crate::books::{book_of, resolve_identifier, typed_error};
use crate::handlers::AppState;
use crate::level_pricing::pricing_rule_json;
use crate::limits::{authorize, check_trade, current, limited_book_record, STATUS_DATA};
use crate::rfq_desk::{
    Admission, AutoPrice, CurveRead, LimitCheck, OpenRfq, PricedSide, PricedUnder, Receipt,
    Refused, RfqDesk, RfqKey, RfqPricing, SentQuote, MAX_OPEN_RFQS, RFQ_ENTITY,
    RFQ_PRICING_SECONDS,
};

/// What a source needs to send requests and their outcomes.
const INGEST_DATA: &[DataScope] = &[DataScope::Books, DataScope::Reference];

/// One request of an ingest, as answered.
#[derive(Serialize)]
struct Answer {
    index: usize,
    rfq_id: String,
    #[serde(flatten)]
    outcome: Outcome,
}

#[derive(Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
enum Outcome {
    /// Received and priced.
    Accepted {
        rfq_key: RfqKey,
        state: RfqState,
    },
    /// The source sent it before, with the same content.
    Retry {
        rfq_key: RfqKey,
        state: RfqState,
    },
    Refused {
        code: &'static str,
        reason: String,
    },
}

impl Outcome {
    fn refused(code: &'static str, reason: impl Into<String>) -> Self {
        Self::Refused {
            code,
            reason: reason.into(),
        }
    }
}

/// A request checked, resolved and priced, or answered without being admitted.
enum Taken {
    Admit(Box<Admission>),
    Answered(Outcome),
}

fn answered(code: &'static str, reason: impl Into<String>) -> Taken {
    Taken::Answered(Outcome::refused(code, reason))
}

fn unavailable(message: &str) -> Response {
    typed_error(StatusCode::SERVICE_UNAVAILABLE, "RFQ_UNAVAILABLE", message)
}

/// `POST /api/v1/rfqs`.
pub(crate) async fn ingest(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    desk: Option<Extension<Arc<RfqDesk>>>,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let started = Instant::now();
    let Some(Extension(desk)) = desk else {
        return unavailable("this server takes no requests for quote");
    };
    let Ok(body) = body else {
        return typed_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "RFQ_BODY_TOO_LARGE",
            "an ingest body is at most 128 KiB",
        );
    };
    let Ok(batch) = serde_json::from_slice::<RfqBatch>(&body) else {
        return typed_error(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "invalid RFQ batch JSON",
        );
    };
    if batch.rfqs.is_empty() || batch.rfqs.len() > MAX_RFQ_BATCH {
        return typed_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "RFQ_BATCH",
            "a batch carries 1 to 64 requests",
        );
    }
    if !desk.is_ready() {
        return unavailable("the RFQ desk is expiring the requests its last run left open");
    }
    let Ok(received_at) = crate::reference::now() else {
        return unavailable("the clock is outside the supported range");
    };
    let _ingest = desk.hold_ingest().await;
    let Ok(mut context) = state.engine.request_context(None, received_at).await else {
        return unavailable("no activated configuration prices requests for quote");
    };
    let mut room = MAX_OPEN_RFQS.saturating_sub(desk.open_count().await);
    let mut keys = HashSet::new();
    let mut answers: Vec<Option<Answer>> = Vec::with_capacity(batch.rfqs.len());
    let mut admitted = Vec::new();
    for (index, request) in batch.rfqs.into_iter().enumerate() {
        let rfq_id = request.rfq_id.clone();
        let taken = Intake {
            state: &state,
            ctx: &ctx,
            desk: &desk,
            received_at,
            room,
        }
        .take(&mut context, request, &mut keys)
        .await;
        match taken {
            Taken::Admit(admission) => {
                room = room.saturating_sub(1);
                admitted.push((index, rfq_id, *admission));
                answers.push(None);
            }
            Taken::Answered(outcome) => answers.push(Some(Answer {
                index,
                rfq_id,
                outcome,
            })),
        }
    }
    let (named, admissions): (Vec<_>, Vec<_>) = admitted
        .into_iter()
        .map(|(index, rfq_id, admission)| ((index, rfq_id, admission.rfq_key.clone()), admission))
        .unzip();
    let recorded = desk.admit(admissions).await;
    for ((index, rfq_id, rfq_key), recorded) in named.into_iter().zip(recorded) {
        let outcome = match recorded {
            Ok(state) => {
                metrics::histogram!(RFQ_PRICING_SECONDS).record(started.elapsed().as_secs_f64());
                Outcome::Accepted { rfq_key, state }
            }
            Err(error) => {
                tracing::warn!(%error, "RFQ desk: a request could not be recorded");
                Outcome::refused(
                    "RFQ_UNRECORDED",
                    "the request could not be recorded; send it again",
                )
            }
        };
        if let Some(slot) = answers.get_mut(index) {
            *slot = Some(Answer {
                index,
                rfq_id,
                outcome,
            });
        }
    }
    let answers: Vec<Answer> = answers.into_iter().flatten().collect();
    let status = if answers.iter().any(|answer| {
        matches!(
            answer.outcome,
            Outcome::Accepted { .. } | Outcome::Retry { .. }
        )
    }) {
        StatusCode::OK
    } else if answers.iter().any(|answer| {
        matches!(
            answer.outcome,
            Outcome::Refused {
                code: "RFQ_CAPACITY",
                ..
            }
        )
    }) {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::UNPROCESSABLE_ENTITY
    };
    (status, Json(serde_json::json!({ "rfqs": answers }))).into_response()
}

/// What one ingest reads each request against.
struct Intake<'a> {
    state: &'a AppState,
    ctx: &'a Option<Extension<AccessContext>>,
    desk: &'a RfqDesk,
    received_at: UtcTimestamp,
    /// Requests the desk can still open.
    room: usize,
}

impl Intake<'_> {
    /// Check, resolve and price `request`, or answer it. `keys` are the batch's keys so far.
    async fn take(
        &self,
        context: &mut RequestContext,
        request: RfqRequest,
        keys: &mut HashSet<RfqKey>,
    ) -> Taken {
        if let Err(refusal) = request.validate(self.received_at) {
            return answered(refusal.code(), refusal.reason());
        }
        let rfq_key = RfqKey::of(&request);
        if !keys.insert(rfq_key.clone()) {
            return answered(
                "RFQ_DUPLICATE",
                "a batch sends each of a source's RFQ ids once",
            );
        }
        let source = Resource::new(ResourceKind::Source, request.source_id.as_str());
        if !self.allowed(Action::RfqIngest, INGEST_DATA, &[source]) {
            return answered(
                "FORBIDDEN",
                "the caller may not send requests for this source",
            );
        }
        match self.desk.known(&rfq_key).await {
            Ok(Some(known)) if known.request == request => {
                return Taken::Answered(Outcome::Retry {
                    rfq_key,
                    state: known.state,
                })
            }
            Ok(Some(_)) => {
                return answered(
                    "RFQ_CONFLICT",
                    "the source sent this RFQ id before with other content",
                )
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "RFQ desk: a request's records are unreadable");
                return answered(
                    "RFQ_UNREADABLE",
                    "the desk's records could not be read; send it again",
                );
            }
        }
        if self.room == 0 {
            return answered("RFQ_CAPACITY", "the desk holds 256 open requests");
        }
        let Some(width_factor) = context.width_factor(&request.client_tier) else {
            return answered(
                "RFQ_CLIENT_TIER",
                "the active configuration lists no such client tier",
            );
        };
        let reference = match self.resolve(&request).await {
            Ok(reference) => reference,
            Err(outcome) => return Taken::Answered(outcome),
        };
        let book = match self.book(&request.book_id).await {
            Ok(book) => book,
            Err(outcome) => return Taken::Answered(outcome),
        };
        let Ok(instrument_id) = InstrumentId::new(reference.record.instrument_id.as_str()) else {
            return answered(
                "RFQ_INSTRUMENT",
                "the bond's id is not a valid instrument id",
            );
        };
        let auto_price = auto_price(
            &self.state.engine,
            context,
            &reference,
            &request,
            width_factor,
            self.received_at,
        )
        .await;
        let position = BookPosition {
            instrument_id: instrument_id.clone(),
            quantity: ExactDecimal::new(request.side.position_change(request.face.decimal())),
            unit: QuantityUnit::Face,
            reference_version: None,
            source_identifier: None,
            sector: None,
            rating: None,
        };
        let check = match check_trade(self.state, &book, position, self.received_at).await {
            Ok(status) => LimitCheck::Evaluated { status },
            Err(reason) => LimitCheck::Unavailable { reason },
        };
        let configuration = PricedUnder {
            configuration_id: context.configuration_id().clone(),
            version: context.configuration_version(),
            activation_revision: context.activation_revision(),
            client_tier: request.client_tier.clone(),
            width_factor,
        };
        let priced_at = crate::reference::now().unwrap_or(self.received_at);
        Taken::Admit(Box::new(Admission {
            rfq_key,
            receipt: Receipt {
                request,
                instrument_id,
                instrument_version: reference.record.version,
                received_at: self.received_at,
            },
            pricing: RfqPricing {
                configuration,
                auto_price,
                check,
                priced_at,
            },
        }))
    }

    fn allowed(&self, action: Action, data: &[DataScope], resources: &[Resource]) -> bool {
        self.ctx.as_ref().is_some_and(|Extension(ctx)| {
            self.state
                .access
                .authorize(ctx, action, data, resources)
                .is_ok()
        })
    }

    /// The bond `request` names, as stored at receipt with the pin of its rule. A bond named
    /// by an identifier must be one the caller may name.
    async fn resolve(&self, request: &RfqRequest) -> Result<ResolvedBondReference, Outcome> {
        let lookup = ReferenceLookup {
            as_of: self.received_at,
            known_at: self.received_at,
        };
        let unreadable = || Outcome::refused("RFQ_UNREADABLE", "the bond could not be read");
        let instrument_id = match (&request.instrument_id, &request.identifier) {
            (Some(id), _) => id.clone(),
            (None, Some(identifier)) => {
                let repo: &dyn ReferenceRepository =
                    self.state.engine.reference_repository().as_ref();
                match resolve_identifier(repo, identifier, lookup).await {
                    Ok(Ok((id, _))) => id,
                    Ok(Err((code, reason))) => return Err(Outcome::refused(code, reason)),
                    Err(_) => return Err(unreadable()),
                }
            }
            (None, None) => {
                return Err(Outcome::refused(
                    "RFQ_INSTRUMENT",
                    "an RFQ names its bond once",
                ))
            }
        };
        let bond = Resource::new(ResourceKind::Instrument, instrument_id.as_str());
        if !self.allowed(Action::RfqIngest, INGEST_DATA, &[bond]) {
            return Err(Outcome::refused(
                "FORBIDDEN",
                "the caller may not send requests on this bond",
            ));
        }
        let id: convex_core::ids::InstrumentId = instrument_id.into();
        match self
            .state
            .engine
            .reference_at_for_pricing(&id, lookup)
            .await
        {
            Ok(Some(reference)) if reference.record.bond.is_some() => Ok(reference),
            Ok(_) => Err(Outcome::refused(
                "RFQ_INSTRUMENT",
                "no bond by this id is stored at receipt",
            )),
            Err(_) => Err(unreadable()),
        }
    }

    /// The book a request's trade would go to: stored and not retired.
    async fn book(&self, book_id: &BookId) -> Result<MasterRecord, Outcome> {
        let repo: &dyn ReferenceRepository = self.state.engine.reference_repository().as_ref();
        let record = current(repo, MasterKey::Book(book_id.clone()))
            .await
            .map_err(|_| Outcome::refused("RFQ_UNREADABLE", "the book could not be read"))?
            .ok_or_else(|| Outcome::refused("RFQ_BOOK", "no book by this id is stored"))?;
        let retired = book_of(&record)
            .map_err(|_| Outcome::refused("RFQ_UNREADABLE", "the book could not be read"))?
            .is_retired();
        if retired {
            return Err(Outcome::refused("RFQ_BOOK", "the book is retired"));
        }
        Ok(record)
    }
}

/// Why a price is the mid: the bond's rule sets no two-way width for it.
const NO_RULE_WIDTH: &str = "the bond's rule sets no two-way width";
/// Why a callable's or floating rate note's price is the mid.
const NO_TWO_WAY: &str = "a callable or floating rate note is priced at its Live mid";

/// The desk side's price for `request` from the bond's Live level, under `context`'s
/// configuration, at the rule's width for the trade's size scaled by `width_factor`.
async fn auto_price(
    engine: &PricingEngine,
    context: &mut RequestContext,
    reference: &ResolvedBondReference,
    request: &RfqRequest,
    width_factor: convex_core::types::WidthFactor,
    received_at: UtcTimestamp,
) -> AutoPrice {
    match priced_side(
        engine,
        context,
        reference,
        request,
        width_factor,
        received_at,
    )
    .await
    {
        Ok(side) => AutoPrice::Priced(Box::new(side)),
        Err(reason) => AutoPrice::Unpriced { reason },
    }
}

async fn priced_side(
    engine: &PricingEngine,
    context: &mut RequestContext,
    reference: &ResolvedBondReference,
    request: &RfqRequest,
    width_factor: convex_core::types::WidthFactor,
    received_at: UtcTimestamp,
) -> Result<PricedSide, String> {
    let bond = reference
        .record
        .bond
        .as_ref()
        .ok_or("the bond's reference is a tombstone")?;
    let instrument_id = InstrumentId::new(bond.instrument_id.as_str())
        .map_err(|_| "the bond's id is not a valid instrument id")?;
    let version = reference.record.version;
    let accepts_demo = engine
        .configuration_use_cases()
        .is_some_and(|cases| cases.accepts(DataQuality::Demo));
    let records = engine.runtime_results_of([bond.instrument_id.as_str()]);
    let record = records.first().map(Arc::as_ref);
    let desk_side = request.side.desk_side();
    let live = |reason: &str| format!("the bond's Live level is unavailable: {reason}");
    if convex_engine::optional_valuation::is_runtime_product(bond) {
        let price =
            live_price(record, &instrument_id, version, received_at, accepts_demo).map_err(live)?;
        let quote_input =
            live_quote_input(record, &instrument_id, version, received_at, accepts_demo)
                .ok()
                .cloned();
        return Ok(PricedSide {
            desk_side,
            clean_price: ExactDecimal::new(price.clean_price_mid),
            level: None,
            clean_price_mid: ExactDecimal::new(price.clean_price_mid),
            mid_only: Some(NO_TWO_WAY),
            quote_input,
            pricing_rule: None,
            curves: Vec::new(),
        });
    }
    let quote_input = live_quote_input(record, &instrument_id, version, received_at, accepts_demo)
        .map_err(live)?
        .clone();
    let rule = engine
        .pinned_rule_for_request(&bond.instrument_id, reference.pricing_rule.clone())
        .await
        .map_err(|error| format!("the bond's rule could not be read: {error}"))?;
    let in_force = engine
        .rule_in_force(context, bond, rule)
        .await
        .map_err(|error| error.to_string())?;
    let level = quote_input.observation.value.value;
    let quoted = match &in_force {
        Some(in_force) if in_force.rule.method.quote_field() == quote_input.field => {
            QuotedInput::quoted_by(&in_force.rule.method, level)
        }
        None if quote_input.field == QuoteField::MidPrice => QuotedInput::Price(level),
        _ => return Err("the bond's Live quote is not in its rule's field".into()),
    };
    // Only a Z-spread reads a curve: the configuration's discount curve.
    let discount = match in_force.as_ref().map(|in_force| &in_force.rule.method) {
        Some(PricingMethod::ZSpread {}) => Some(
            context
                .curves_for(bond)
                .await
                .map_err(|error| error.to_string())?
                .discount,
        ),
        _ => None,
    };
    let size = TradeSize {
        face: request.face.decimal(),
        factor: width_factor,
    };
    let pricing = engine
        .price_for_trade(
            bond,
            in_force.as_ref(),
            quoted,
            size,
            request.settlement_date.date(),
            discount.as_ref(),
        )
        .map_err(|error| error.to_string())?;
    let is_level = in_force
        .as_ref()
        .is_some_and(|in_force| in_force.rule.method.is_level());
    let (clean_price, side_level, mid_only) = desk_side_of(&pricing, desk_side, level, is_level);
    Ok(PricedSide {
        desk_side,
        clean_price: ExactDecimal::new(clean_price),
        level: side_level.map(ExactDecimal::new),
        clean_price_mid: ExactDecimal::new(pricing.clean_price_mid),
        mid_only,
        quote_input: Some(quote_input),
        pricing_rule: in_force
            .as_ref()
            .map(|in_force| pricing_rule_json(in_force, quoted, &pricing)),
        curves: context
            .pins()
            .into_iter()
            .map(|(curve_id, version)| CurveRead {
                curve_id: curve_id.to_owned(),
                version,
            })
            .collect(),
    })
}

/// The clean price and level of `side` of `pricing`, and why it is the mid when it is: a
/// two-way at the rule's width, or the mid. `mid_level` is the level quoted.
fn desk_side_of(
    pricing: &RulePricing,
    side: DeskSide,
    mid_level: rust_decimal::Decimal,
    is_level: bool,
) -> (
    rust_decimal::Decimal,
    Option<rust_decimal::Decimal>,
    Option<&'static str>,
) {
    match &pricing.bid_ask {
        BidAsk::RuleWidth(two_way) => {
            let (price, level) = match side {
                DeskSide::Bid => (two_way.clean_price_bid, two_way.bid_level),
                DeskSide::Ask => (two_way.clean_price_ask, two_way.ask_level),
            };
            (price, is_level.then_some(level), None)
        }
        BidAsk::ConfigurationWidth | BidAsk::MidOnly => (
            pricing.clean_price_mid,
            is_level.then_some(mid_level),
            Some(NO_RULE_WIDTH),
        ),
    }
}

/// `POST /api/v1/rfqs/:rfq_key/quote` body.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuoteBody {
    /// The clean price sent on the desk's side: the auto price, or the trader's.
    clean_price: ExactDecimal,
}

/// `POST /api/v1/rfqs/:rfq_key/outcome` body.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutcomeBody {
    outcome: SourceOutcome,
}

/// `POST /api/v1/rfqs/:rfq_key/quote`: the trader sends a clean price on the desk's side,
/// as priced or adjusted.
pub(crate) async fn quote(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    desk: Option<Extension<Arc<RfqDesk>>>,
    Path(raw): Path<String>,
    payload: Result<Json<QuoteBody>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = payload else {
        return typed_error(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "invalid quote JSON",
        );
    };
    if body.clean_price.decimal() <= rust_decimal::Decimal::ZERO {
        return typed_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "RFQ_PRICE",
            "a quote's clean price is positive",
        );
    }
    let Ok(quoted_at) = crate::reference::now() else {
        return unavailable("the clock is outside the supported range");
    };
    let sent = SentQuote {
        clean_price: body.clean_price,
        quoted_at,
    };
    respond(&state, &ctx, desk, raw, RfqEvent::Quoted, Some(sent)).await
}

/// `POST /api/v1/rfqs/:rfq_key/pass`: the trader declines to quote.
pub(crate) async fn pass(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    desk: Option<Extension<Arc<RfqDesk>>>,
    Path(raw): Path<String>,
) -> Response {
    respond(&state, &ctx, desk, raw, RfqEvent::Passed, None).await
}

/// `POST /api/v1/rfqs/:rfq_key/outcome`: the source reports the client's outcome, or that
/// the client withdrew the request.
pub(crate) async fn outcome(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    desk: Option<Extension<Arc<RfqDesk>>>,
    Path(raw): Path<String>,
    payload: Result<Json<OutcomeBody>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = payload else {
        return typed_error(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "invalid outcome JSON",
        );
    };
    let event = RfqEvent::from(body.outcome);
    let Some(Extension(desk)) = desk else {
        return unavailable("this server takes no requests for quote");
    };
    let Some(key) = RfqKey::parse(&raw) else {
        return unknown();
    };
    let fingerprint = ctx
        .as_ref()
        .and_then(|Extension(ctx)| ctx.subject_fingerprint());
    let moved = desk
        .transition(&key, event, None, fingerprint, |rfq| {
            let request = &rfq.receipt.request;
            authorize_resources(
                &state,
                &ctx,
                Action::RfqIngest,
                INGEST_DATA,
                vec![
                    Resource::new(ResourceKind::Source, request.source_id.as_str()),
                    Resource::new(ResourceKind::Book, request.book_id.as_str()),
                    Resource::new(ResourceKind::Instrument, rfq.receipt.instrument_id.as_str()),
                ],
            )
        })
        .await;
    moved_response(moved)
}

/// A trader's `event` on the request `raw` names: the trader must hold `RfqRespond` on its
/// book and bond.
async fn respond(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    desk: Option<Extension<Arc<RfqDesk>>>,
    raw: String,
    event: RfqEvent,
    sent: Option<SentQuote>,
) -> Response {
    let Some(Extension(desk)) = desk else {
        return unavailable("this server takes no requests for quote");
    };
    let Some(key) = RfqKey::parse(&raw) else {
        return unknown();
    };
    let fingerprint = ctx
        .as_ref()
        .and_then(|Extension(ctx)| ctx.subject_fingerprint());
    let moved = desk
        .transition(&key, event, sent, fingerprint, |rfq| {
            authorize(
                state,
                ctx,
                Action::RfqRespond,
                STATUS_DATA,
                &rfq.receipt.request.book_id,
                [rfq.receipt.instrument_id.as_str().to_owned()],
            )
        })
        .await;
    moved_response(moved)
}

fn authorize_resources(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    action: Action,
    data: &[DataScope],
    resources: Vec<Resource>,
) -> Result<(), Response> {
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

fn unknown() -> Response {
    typed_error(
        StatusCode::NOT_FOUND,
        "RFQ_UNKNOWN",
        "no request for quote by this key",
    )
}

/// A transition's reply: the request as it now stands, or why it did not move.
fn moved_response(moved: Result<OpenRfq, Refused>) -> Response {
    match moved {
        Ok(rfq) => Json(serde_json::json!({
            "rfq_key": rfq.rfq_key,
            "state": rfq.state,
        }))
        .into_response(),
        Err(Refused::Unknown) => unknown(),
        Err(Refused::Forbidden(response)) => response,
        Err(Refused::Transition(refused)) => {
            let code = match refused {
                RfqTransitionError::Final(_) => "RFQ_FINAL",
                RfqTransitionError::NotAllowed(_) => "RFQ_TRANSITION",
            };
            typed_error(StatusCode::CONFLICT, code, &refused.to_string())
        }
        Err(Refused::Unrecorded(error)) => {
            tracing::warn!(%error, "RFQ desk: a transition could not be recorded");
            unavailable("the transition could not be recorded; nothing changed")
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    /// Only requests in this open state.
    #[serde(default)]
    state: Option<RfqState>,
}

/// `GET /api/v1/rfqs`: every open request the caller may read, by deadline, optionally
/// only those in one state. Each needs what a book's limit status needs, since it carries
/// its check, and its bond; the others are left out and counted in `withheld`.
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    desk: Option<Extension<Arc<RfqDesk>>>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Response {
    let Ok(Query(query)) = query else {
        return typed_error(
            StatusCode::BAD_REQUEST,
            "INVALID_INPUT",
            "state names an open state",
        );
    };
    let Some(Extension(desk)) = desk else {
        return unavailable("this server takes no requests for quote");
    };
    let mut rfqs: Vec<OpenRfq> = desk
        .open_rfqs()
        .await
        .into_iter()
        .filter(|rfq| query.state.is_none_or(|state| rfq.state == state))
        .collect();
    rfqs.sort_by(|a, b| {
        a.receipt
            .request
            .deadline
            .cmp(&b.receipt.request.deadline)
            .then_with(|| a.rfq_key.cmp(&b.rfq_key))
    });
    let mut readable: BTreeMap<BookId, bool> = BTreeMap::new();
    let mut shown = Vec::with_capacity(rfqs.len());
    let mut withheld = 0usize;
    for rfq in rfqs {
        let book_id = &rfq.receipt.request.book_id;
        let book_readable = match readable.get(book_id) {
            Some(readable) => *readable,
            None => {
                let ok = limited_book_record(&state, &ctx, book_id).await.is_ok();
                readable.insert(book_id.clone(), ok);
                ok
            }
        };
        let bond_readable = book_readable
            && authorize(
                &state,
                &ctx,
                Action::BookRead,
                STATUS_DATA,
                book_id,
                [rfq.receipt.instrument_id.as_str().to_owned()],
            )
            .is_ok();
        if bond_readable {
            shown.push(rfq);
        } else {
            withheld += 1;
        }
    }
    Json(serde_json::json!({ "rfqs": shown, "withheld": withheld })).into_response()
}

/// `GET /api/v1/rfqs/:rfq_key`: every entry of one request, in order. It needs what the
/// list needs of the request.
pub(crate) async fn records(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    desk: Option<Extension<Arc<RfqDesk>>>,
    Path(raw): Path<String>,
) -> Response {
    let Some(Extension(desk)) = desk else {
        return unavailable("this server takes no requests for quote");
    };
    let Some(key) = RfqKey::parse(&raw) else {
        return unknown();
    };
    let page = match desk
        .records()
        .entity_page(RFQ_ENTITY, key.as_str(), 0, MAX_ENTITY_PAGE)
        .await
    {
        Ok(page) => page,
        Err(error) => {
            tracing::warn!(%error, "RFQ desk: a request's entries are unreadable");
            return unavailable("the request's entries could not be read");
        }
    };
    let entries: Vec<serde_json::Value> = page
        .entries
        .iter()
        .filter_map(|entry| {
            let mut details: serde_json::Value =
                serde_json::from_str(entry.details.as_deref()?).ok()?;
            details["record_id"] = serde_json::json!(entry.id);
            Some(details)
        })
        .collect();
    let Some(first) = entries.first() else {
        return unknown();
    };
    #[derive(Deserialize)]
    struct Named {
        request: RfqRequest,
        instrument_id: InstrumentId,
    }
    let Ok(named) = serde_json::from_value::<Named>(first.clone()) else {
        return unavailable("the request's entries could not be read");
    };
    let book_id = &named.request.book_id;
    if let Err(response) = limited_book_record(&state, &ctx, book_id).await {
        return response;
    }
    if let Err(response) = authorize(
        &state,
        &ctx,
        Action::BookRead,
        STATUS_DATA,
        book_id,
        [named.instrument_id.as_str().to_owned()],
    ) {
        return response;
    }
    Json(serde_json::json!({ "rfq_key": key, "records": entries })).into_response()
}
