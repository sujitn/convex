//! Clients' requests for quote over HTTP.
//!
//! - `POST /api/v1/rfqs`: a source sends a batch of requests. Each is answered on its
//!   own: received and priced, a retry of one already received, or refused with a code,
//!   one the caller may not send for its source, book or bond included. The reply is 200
//!   when any was taken, 403 when every one was refused for access, 429 when none was
//!   taken and one was refused for capacity, and 422 otherwise.
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

use std::collections::btree_map::Entry;
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
use convex_core::types::WidthFactor;
use convex_engine::level_pricing::{live_price, live_quote_input, BidAsk, QuotedInput, TradeSize};
use convex_engine::ports::quote_event::QuoteField;
use convex_engine::ports::reference_master::{
    BookId, BookPosition, MasterKey, QuantityUnit, ReferenceLookup, ResolvedBondReference,
};
use convex_engine::ports::reference_repository::{ReferenceError, ReferenceRepository};
use convex_engine::ports::rfq::{
    DeskSide, RfqRequest, RfqState, SourceOutcome, MAX_RFQ_BATCH, MAX_RFQ_BODY_BYTES,
};
use convex_engine::RequestContext;
use serde::{Deserialize, Serialize};

use crate::access::http::authorize_caller;
use crate::access::{
    recorded_subject, AccessContext, AccessControl, AccessError, Action, DataScope, Resource,
    ResourceKind,
};
use crate::books::{book_of, resolve_identifier, typed_error};
use crate::error::ApiError;
use crate::handlers::AppState;
use crate::level_pricing::pricing_rule_json;
use crate::limits::{
    authorize, check_trade, limited_book_record, named_instruments, set_of, traded_book,
    NoTradedBook, PricedTerms, TradedBook, STATUS_DATA,
};
use crate::rfq_desk::{
    receipt_of, Admission, Admitted, AutoPrice, CurveRead, LimitCheck, PricedSide, PricedUnder,
    Receipt, Recorded, Refused, Rfq, RfqDesk, RfqKey, RfqPricing, SentQuote, MAX_OPEN_RFQS,
    NO_RFQ_DESK, RFQ_PRICING_SECONDS,
};

/// What a source needs to send requests and their outcomes.
const INGEST_DATA: &[DataScope] = &[DataScope::Books, DataScope::Reference];

/// One request of an ingest, as answered.
#[derive(Serialize)]
struct Answer {
    index: usize,
    rfq_id: String,
    #[serde(flatten)]
    result: IngestResult,
}

impl Answer {
    /// Whether the request was refused with `code`.
    fn is_refused_with(&self, code: &str) -> bool {
        matches!(&self.result, IngestResult::Refused { code: refused, .. } if *refused == code)
    }
}

#[derive(Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
enum IngestResult {
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

fn refused(code: &'static str, reason: impl Into<String>) -> IngestResult {
    IngestResult::Refused {
        code,
        reason: reason.into(),
    }
}

fn unavailable(message: &str) -> Response {
    ApiError::unavailable(message).into_response()
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
        return unavailable(NO_RFQ_DESK);
    };
    let body = match body {
        Ok(body) => body,
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return typed_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "RFQ_BODY_TOO_LARGE",
                &format!(
                    "an ingest body is at most {} KiB",
                    MAX_RFQ_BODY_BYTES / 1024
                ),
            )
        }
        Err(rejection) => return ApiError::invalid(rejection.body_text()).into_response(),
    };
    // The batch's shape first; each request is decoded on its own, so one malformed
    // request is refused alone.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Batch {
        rfqs: Vec<serde_json::Value>,
    }
    let Ok(batch) = serde_json::from_slice::<Batch>(&body) else {
        return ApiError::invalid("invalid RFQ batch JSON").into_response();
    };
    if batch.rfqs.is_empty() || batch.rfqs.len() > MAX_RFQ_BATCH {
        return typed_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "RFQ_BATCH",
            &format!("a batch carries 1 to {MAX_RFQ_BATCH} requests"),
        );
    }
    if !desk.is_ready() {
        return unavailable("the RFQ desk is expiring the requests its last run left open");
    }
    let Ok(received_at) = crate::reference::now() else {
        return unavailable("the clock is outside the supported range");
    };
    let Ok(context) = state.engine.request_context(None, received_at).await else {
        return unavailable("no activated configuration prices requests for quote");
    };
    let mut intake = Intake {
        state: &state,
        ctx: &ctx,
        desk: &desk,
        received_at,
        context,
        room: MAX_OPEN_RFQS.saturating_sub(desk.open_count().await),
        sender: recorded_subject(&ctx),
        keys: HashSet::new(),
        books: BTreeMap::new(),
        terms: PricedTerms::new(),
    };
    let mut answers = Vec::with_capacity(batch.rfqs.len());
    let mut admissions = Vec::new();
    for (index, item) in batch.rfqs.into_iter().enumerate() {
        let rfq_id = item["rfq_id"].as_str().unwrap_or_default().to_owned();
        let taken = match serde_json::from_value::<RfqRequest>(item) {
            Ok(request) => intake.take(request).await,
            Err(error) => Err(refused(
                "INVALID_INPUT",
                format!("the request is not an RFQ: {error}"),
            )),
        };
        let result = match taken {
            Ok(admission) => {
                let rfq_key = admission.rfq_key.clone();
                admissions.push((index, *admission));
                IngestResult::Accepted {
                    rfq_key,
                    state: RfqState::Priced,
                }
            }
            Err(result) => result,
        };
        answers.push(Answer {
            index,
            rfq_id,
            result,
        });
    }
    let (indices, admissions): (Vec<usize>, Vec<Admission>) = admissions.into_iter().unzip();
    for (index, admitted) in indices.into_iter().zip(desk.admit(admissions).await) {
        let Some(Answer {
            result: IngestResult::Accepted { rfq_key, .. },
            ..
        }) = answers.get(index)
        else {
            continue;
        };
        let rfq_key = rfq_key.clone();
        let result = match admitted {
            Admitted::Accepted => {
                metrics::histogram!(RFQ_PRICING_SECONDS).record(started.elapsed().as_secs_f64());
                continue;
            }
            Admitted::Retry(state) => IngestResult::Retry { rfq_key, state },
            Admitted::Conflict => conflict(),
            Admitted::Full => full(),
            Admitted::Unrecorded(error) => {
                tracing::warn!(%error, "RFQ desk: a request could not be recorded");
                refused(
                    "RFQ_UNRECORDED",
                    "the request could not be recorded; send it again",
                )
            }
        };
        if let Some(answer) = answers.get_mut(index) {
            answer.result = result;
        }
    }
    let status = if answers.iter().any(|answer| {
        matches!(
            answer.result,
            IngestResult::Accepted { .. } | IngestResult::Retry { .. }
        )
    }) {
        StatusCode::OK
    } else if answers
        .iter()
        .all(|answer| answer.is_refused_with("FORBIDDEN"))
    {
        StatusCode::FORBIDDEN
    } else if answers
        .iter()
        .any(|answer| answer.is_refused_with("RFQ_CAPACITY"))
    {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::UNPROCESSABLE_ENTITY
    };
    (status, Json(serde_json::json!({ "rfqs": answers }))).into_response()
}

fn conflict() -> IngestResult {
    refused(
        "RFQ_CONFLICT",
        "the source sent this RFQ id before with other content",
    )
}

fn full() -> IngestResult {
    refused(
        "RFQ_CAPACITY",
        format!("the desk holds {MAX_OPEN_RFQS} open requests"),
    )
}

/// One ingest: what each request is read against, and what the batch has read so far.
struct Intake<'a> {
    state: &'a AppState,
    ctx: &'a Option<Extension<AccessContext>>,
    desk: &'a RfqDesk,
    received_at: UtcTimestamp,
    /// The active configuration, read once for the batch.
    context: RequestContext,
    /// Requests the desk can still open.
    room: usize,
    /// The subject sending the batch, as each receipt records it.
    sender: String,
    /// The batch's keys so far.
    keys: HashSet<RfqKey>,
    /// Each book the batch's requests trade into, read once.
    books: BTreeMap<BookId, Result<Arc<TradedBook>, NoTradedBook>>,
    /// The priced terms the batch's checks have read; a version never changes.
    terms: PricedTerms,
}

impl Intake<'_> {
    /// Check, resolve, price and check `request`, or answer it.
    async fn take(&mut self, request: RfqRequest) -> Result<Box<Admission>, IngestResult> {
        let rfq_key = RfqKey::of(&request);
        if !self.keys.insert(rfq_key.clone()) {
            return Err(refused(
                "RFQ_DUPLICATE",
                "a batch sends each of a source's RFQ ids once",
            ));
        }
        let source = Resource::new(ResourceKind::Source, request.source_id.as_str());
        let book = Resource::new(ResourceKind::Book, request.book_id.as_str());
        if !self.may_send(&[source, book]) {
            return Err(refused(
                "FORBIDDEN",
                "the caller may not send requests for this source or book",
            ));
        }
        // A retry is recognised first: its deadline may have passed since it was received.
        match self.desk.known(&rfq_key).await {
            Ok(Some(known)) if known.receipt.request == request => {
                return Err(IngestResult::Retry {
                    rfq_key,
                    state: known.state,
                })
            }
            Ok(Some(_)) => return Err(conflict()),
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "RFQ desk: a request's records are unreadable");
                return Err(refused(
                    "RFQ_UNREADABLE",
                    "the desk's records could not be read; send it again",
                ));
            }
        }
        request
            .validate(self.received_at)
            .map_err(|refusal| refused(refusal.code(), refusal.reason()))?;
        if self.room == 0 {
            return Err(full());
        }
        let width_factor = self
            .context
            .width_factor(&request.client_tier)
            .ok_or_else(|| {
                refused(
                    "RFQ_CLIENT_TIER",
                    "the active configuration lists no such client tier",
                )
            })?;
        let (reference, instrument_id) = self.resolve(&request).await?;
        let traded = self.traded_book(&request.book_id).await?;
        let auto_price = match self
            .priced_side(&reference, &instrument_id, &request, width_factor)
            .await
        {
            Ok(side) => AutoPrice::Priced(Box::new(side)),
            Err(reason) => AutoPrice::unpriced(reason),
        };
        let position = BookPosition {
            instrument_id: instrument_id.clone(),
            quantity: ExactDecimal::new(request.side.position_change(request.face.decimal())),
            unit: QuantityUnit::Face,
            reference_version: None,
            source_identifier: None,
            sector: None,
            rating: None,
        };
        let check = match check_trade(
            self.state,
            &traded,
            position,
            self.received_at,
            &mut self.terms,
        )
        .await
        {
            Ok(check) => LimitCheck::Evaluated {
                status: check.status,
                statuses: check.statuses,
            },
            Err(reason) => LimitCheck::Unavailable { reason },
        };
        let configuration = PricedUnder {
            configuration_id: self.context.configuration_id().clone(),
            version: self.context.configuration_version(),
            activation_revision: self.context.activation_revision(),
            client_tier: request.client_tier.clone(),
            width_factor,
        };
        self.room -= 1;
        let priced_at = crate::reference::now().unwrap_or(self.received_at);
        Ok(Box::new(Admission {
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
            sender: self.sender.clone(),
        }))
    }

    /// Whether the caller may send requests naming `resources`.
    fn may_send(&self, resources: &[Resource]) -> bool {
        authorize_caller(
            &self.state.access,
            self.ctx,
            Action::RfqIngest,
            INGEST_DATA,
            resources,
        )
        .is_ok()
    }

    /// The bond `request` names, as stored at receipt with the pin of its rule, and its
    /// id. A bond named by an identifier must be one the caller may name.
    async fn resolve(
        &self,
        request: &RfqRequest,
    ) -> Result<(ResolvedBondReference, InstrumentId), IngestResult> {
        let lookup = ReferenceLookup {
            as_of: self.received_at,
            known_at: self.received_at,
        };
        let unreadable = || refused("RFQ_UNREADABLE", "the bond could not be read");
        let instrument_id = match (&request.instrument_id, &request.identifier) {
            (Some(id), _) => id.clone(),
            (None, Some(identifier)) => {
                let repo: &dyn ReferenceRepository =
                    self.state.engine.reference_repository().as_ref();
                match resolve_identifier(repo, identifier, lookup).await {
                    Ok(Ok((id, _))) => id,
                    Ok(Err((code, reason))) => return Err(refused(code, reason)),
                    Err(_) => return Err(unreadable()),
                }
            }
            (None, None) => return Err(refused("RFQ_INSTRUMENT", "an RFQ names its bond once")),
        };
        if !self.may_send(&[Resource::new(
            ResourceKind::Instrument,
            instrument_id.as_str(),
        )]) {
            return Err(refused(
                "FORBIDDEN",
                "the caller may not send requests on this bond",
            ));
        }
        let id: convex_core::ids::InstrumentId = instrument_id.clone().into();
        match self
            .state
            .engine
            .reference_at_for_pricing(&id, lookup)
            .await
        {
            Ok(Some(reference)) if reference.record.bond.is_some() => {
                Ok((reference, instrument_id))
            }
            Ok(_) => Err(refused(
                "RFQ_INSTRUMENT",
                "no bond by this id is stored at receipt",
            )),
            Err(_) => Err(unreadable()),
        }
    }

    /// The book a request's trade would go to, stored and not retired, read once a batch.
    async fn traded_book(&mut self, book_id: &BookId) -> Result<Arc<TradedBook>, IngestResult> {
        let read = match self.books.get(book_id) {
            Some(read) => read.clone(),
            None => {
                let read = traded_book(self.state, book_id).await.map(Arc::new);
                self.books.insert(book_id.clone(), read.clone());
                read
            }
        };
        read.map_err(|missing| match missing {
            NoTradedBook::Unknown => refused("RFQ_BOOK", "no book by this id is stored"),
            NoTradedBook::Retired => refused("RFQ_BOOK", "the book is retired"),
            NoTradedBook::Unreadable => {
                refused("RFQ_UNREADABLE", "the book or its limits could not be read")
            }
        })
    }

    /// The desk side's price for `request` from the bond's Live level, at the rule's width
    /// for the trade's size scaled by `width_factor`, or why there is none.
    async fn priced_side(
        &mut self,
        reference: &ResolvedBondReference,
        instrument_id: &InstrumentId,
        request: &RfqRequest,
        width_factor: WidthFactor,
    ) -> Result<PricedSide, String> {
        let engine = &self.state.engine;
        let bond = reference
            .record
            .bond
            .as_ref()
            .ok_or("the bond's reference is a tombstone")?;
        let version = reference.record.version;
        let accepts_demo = engine
            .configuration_use_cases()
            .is_some_and(|cases| cases.accepts(DataQuality::Demo));
        let records = engine.runtime_results_of([instrument_id.as_str()]);
        let record = records.first().map(Arc::as_ref);
        let desk_side = request.side.desk_side();
        let at = self.received_at;
        let live = |reason: &str| format!("the bond's Live level is unavailable: {reason}");
        if convex_engine::optional_valuation::is_runtime_product(bond) {
            let price =
                live_price(record, instrument_id, version, at, accepts_demo).map_err(live)?;
            let mid = ExactDecimal::new(price.clean_price_mid);
            return Ok(PricedSide {
                desk_side,
                clean_price: mid,
                level: None,
                clean_price_mid: mid,
                mid_only: Some(NO_TWO_WAY),
                quote_input: live_quote_input(record, instrument_id, version, at, accepts_demo)
                    .ok()
                    .cloned(),
                pricing_rule: None,
                curves: Vec::new(),
            });
        }
        let quote_input = live_quote_input(record, instrument_id, version, at, accepts_demo)
            .map_err(live)?
            .clone();
        let rule = engine
            .pinned_rule_for_request(&bond.instrument_id, reference.pricing_rule.clone())
            .await
            .map_err(|error| format!("the bond's rule could not be read: {error}"))?;
        let in_force = engine
            .rule_in_force(&mut self.context, bond, rule)
            .await
            .map_err(|error| error.to_string())?;
        let method = in_force.as_ref().map(|in_force| &in_force.rule.method);
        let level = quote_input.observation.value.value;
        let quoted = match method {
            Some(method) if method.quote_field() == quote_input.field => {
                QuotedInput::quoted_by(method, level)
            }
            None if quote_input.field == QuoteField::MidPrice => QuotedInput::Price(level),
            _ => return Err("the bond's Live quote is not in its rule's field".into()),
        };
        let (discount, curves) = match method {
            Some(method) if method.reads_discount_curve() => {
                let curves = self
                    .context
                    .curves_for(bond)
                    .await
                    .map_err(|error| error.to_string())?;
                let read = self
                    .context
                    .pins()
                    .into_iter()
                    .map(|(curve_id, version)| CurveRead {
                        curve_id: curve_id.to_owned(),
                        version,
                    })
                    .collect();
                (Some(curves.discount), read)
            }
            _ => (None, Vec::new()),
        };
        let size = TradeSize {
            face: request.face.decimal(),
            width_factor,
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
        let is_level = method.is_some_and(|method| method.is_level());
        let mut side = PricedSide {
            desk_side,
            clean_price: ExactDecimal::new(pricing.clean_price_mid),
            level: is_level.then_some(ExactDecimal::new(level)),
            clean_price_mid: ExactDecimal::new(pricing.clean_price_mid),
            mid_only: Some(NO_RULE_WIDTH),
            quote_input: Some(quote_input),
            pricing_rule: in_force
                .as_ref()
                .map(|in_force| pricing_rule_json(in_force, quoted, &pricing)),
            curves,
        };
        if let BidAsk::RuleWidth(two_way) = &pricing.bid_ask {
            let (price, side_level) = match desk_side {
                DeskSide::Bid => (two_way.clean_price_bid, two_way.bid_level),
                DeskSide::Ask => (two_way.clean_price_ask, two_way.ask_level),
            };
            side.clean_price = ExactDecimal::new(price);
            side.level = is_level.then_some(ExactDecimal::new(side_level));
            side.mid_only = None;
        }
        Ok(side)
    }
}

/// Why a price is the mid: the bond's rule sets no two-way width for it.
const NO_RULE_WIDTH: &str = "the bond's rule sets no two-way width";
/// Why a callable's or floating rate note's price is the mid.
const NO_TWO_WAY: &str = "a callable or floating rate note is priced at its Live mid";

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
    let body = match payload {
        Ok(Json(body)) => body,
        Err(rejection) => return ApiError::from(rejection).into_response(),
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
    let record = Recorded::Quoted { quote: &sent };
    move_by(&state, &ctx, desk, raw, record, Sender::Trader).await
}

/// `POST /api/v1/rfqs/:rfq_key/pass`: the trader declines to quote.
pub(crate) async fn pass(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    desk: Option<Extension<Arc<RfqDesk>>>,
    Path(raw): Path<String>,
) -> Response {
    move_by(&state, &ctx, desk, raw, Recorded::Passed, Sender::Trader).await
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
    let body = match payload {
        Ok(Json(body)) => body,
        Err(rejection) => return ApiError::from(rejection).into_response(),
    };
    let record = Recorded::outcome(body.outcome);
    move_by(&state, &ctx, desk, raw, record, Sender::Source).await
}

/// Who sends an answer, and so what it needs.
#[derive(Clone, Copy)]
enum Sender {
    /// `RfqRespond` on the request's book and bond.
    Trader,
    /// `RfqIngest` on the request's source, book and bond.
    Source,
}

impl Sender {
    fn may_move(
        self,
        state: &AppState,
        ctx: &Option<Extension<AccessContext>>,
        receipt: &Receipt,
    ) -> Result<(), crate::access::AccessError> {
        let request = &receipt.request;
        let mut resources = vec![
            Resource::new(ResourceKind::Book, request.book_id.as_str()),
            Resource::new(ResourceKind::Instrument, receipt.instrument_id.as_str()),
        ];
        let (action, data) = match self {
            Self::Trader => (Action::RfqRespond, STATUS_DATA),
            Self::Source => {
                resources.push(Resource::new(
                    ResourceKind::Source,
                    request.source_id.as_str(),
                ));
                (Action::RfqIngest, INGEST_DATA)
            }
        };
        authorize_caller(&state.access, ctx, action, data, &resources)
    }
}

/// Move the request `raw` names as `record` says, sent by `sender`: the reply is the
/// request's key and new state, or why it did not move.
async fn move_by(
    state: &AppState,
    ctx: &Option<Extension<AccessContext>>,
    desk: Option<Extension<Arc<RfqDesk>>>,
    raw: String,
    record: Recorded<'_>,
    sender: Sender,
) -> Response {
    let Some(Extension(desk)) = desk else {
        return unavailable(NO_RFQ_DESK);
    };
    let Some(key) = RfqKey::parse(&raw) else {
        return unknown();
    };
    let moved = desk
        .transition(&key, record, recorded_subject(ctx), |receipt| {
            sender.may_move(state, ctx, receipt)
        })
        .await;
    match moved {
        Ok(state) => Json(serde_json::json!({ "rfq_key": key, "state": state })).into_response(),
        Err(Refused::Unknown) => unknown(),
        Err(Refused::Forbidden(error)) => crate::access::http::response(error),
        Err(Refused::Transition(refused)) => {
            typed_error(StatusCode::CONFLICT, refused.code(), &refused.to_string())
        }
        Err(Refused::Unrecorded(error)) => {
            tracing::warn!(%error, "RFQ desk: a transition could not be recorded");
            unavailable("the transition could not be recorded; nothing changed")
        }
    }
}

fn unknown() -> Response {
    typed_error(
        StatusCode::NOT_FOUND,
        "RFQ_UNKNOWN",
        "no request for quote by this key",
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    /// Only requests in this open state.
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
        return ApiError::invalid("state names an open state").into_response();
    };
    let Some(Extension(desk)) = desk else {
        return unavailable(NO_RFQ_DESK);
    };
    let rfqs = desk.open_rfqs().await;
    let ctx = ctx.as_ref().map(|Extension(ctx)| ctx);
    let repo: &dyn ReferenceRepository = state.engine.reference_repository().as_ref();
    let mut reader = RfqReader::new(&state.access, ctx, repo);
    let mut shown = Vec::with_capacity(rfqs.len());
    let mut withheld = 0usize;
    for rfq in rfqs {
        if query.state.is_some_and(|state| rfq.state != state) {
            continue;
        }
        if reader.is_readable(&rfq).await {
            shown.push(rfq);
        } else {
            withheld += 1;
        }
    }
    Json(serde_json::json!({ "rfqs": shown, "withheld": withheld })).into_response()
}

/// Each instrument a reader of the requests on `book_id` must hold besides a request's
/// bond: every one the book holds or its limits name, as the book's limit status needs.
pub(crate) async fn book_instruments(
    repo: &dyn ReferenceRepository,
    book_id: &BookId,
) -> Result<Arc<[String]>, ReferenceError> {
    let mut instruments = std::collections::BTreeSet::new();
    if let Some(record) = repo
        .get_master(&MasterKey::LimitSet(book_id.clone()))
        .await?
    {
        instruments
            .extend(named_instruments(&set_of(&record)?.limits).map(|id| id.as_str().to_owned()));
    }
    if let Some(record) = repo.get_master(&MasterKey::Book(book_id.clone())).await? {
        instruments.extend(
            book_of(&record)?
                .positions
                .iter()
                .map(|position| position.instrument_id.as_str().to_owned()),
        );
    }
    Ok(instruments.into_iter().collect())
}

/// Whether `ctx` may read `rfq`, as the list reads it: `BookRead` with the limit status's
/// data on its book, each of `instruments` (those [`book_instruments`] read) and its bond.
pub(crate) fn readable(
    access: &AccessControl,
    ctx: Option<&AccessContext>,
    rfq: &Rfq,
    instruments: &[String],
) -> Result<(), AccessError> {
    let ctx = ctx.ok_or(AccessError::Forbidden)?;
    let bond = rfq.instrument_id().as_str();
    let mut resources = Vec::with_capacity(instruments.len() + 2);
    resources.push(Resource::new(ResourceKind::Book, rfq.book_id().as_str()));
    resources.extend(
        instruments
            .iter()
            .map(|id| Resource::new(ResourceKind::Instrument, id.as_str())),
    );
    if !instruments.iter().any(|id| id == bond) {
        resources.push(Resource::new(ResourceKind::Instrument, bond));
    }
    access.authorize(ctx, Action::BookRead, STATUS_DATA, &resources)
}

/// [`readable`] over many requests, each book's instruments read once.
pub(crate) struct RfqReader<'a> {
    access: &'a AccessControl,
    ctx: Option<&'a AccessContext>,
    repo: &'a dyn ReferenceRepository,
    book_instruments: BTreeMap<BookId, Result<Arc<[String]>, ReferenceError>>,
}

impl<'a> RfqReader<'a> {
    pub(crate) fn new(
        access: &'a AccessControl,
        ctx: Option<&'a AccessContext>,
        repo: &'a dyn ReferenceRepository,
    ) -> Self {
        Self {
            access,
            ctx,
            repo,
            book_instruments: BTreeMap::new(),
        }
    }

    /// [`book_instruments`] of `book_id`, read once.
    pub(crate) async fn book_instruments(
        &mut self,
        book_id: &BookId,
    ) -> &Result<Arc<[String]>, ReferenceError> {
        match self.book_instruments.entry(book_id.clone()) {
            Entry::Occupied(read) => read.into_mut(),
            Entry::Vacant(unread) => unread.insert(book_instruments(self.repo, book_id).await),
        }
    }

    /// Whether the caller may read `rfq`; not when its book could not be read to check.
    pub(crate) async fn is_readable(&mut self, rfq: &Rfq) -> bool {
        let (access, ctx) = (self.access, self.ctx);
        self.book_instruments(rfq.book_id())
            .await
            .as_ref()
            .is_ok_and(|instruments| readable(access, ctx, rfq, instruments).is_ok())
    }
}

/// `GET /api/v1/rfqs/:rfq_key`: every entry of one request, in order, with its state, ID
/// and sender. It needs what the list needs of the request.
pub(crate) async fn records(
    State(state): State<Arc<AppState>>,
    ctx: Option<Extension<AccessContext>>,
    desk: Option<Extension<Arc<RfqDesk>>>,
    Path(raw): Path<String>,
) -> Response {
    let Some(Extension(desk)) = desk else {
        return unavailable(NO_RFQ_DESK);
    };
    let Some(key) = RfqKey::parse(&raw) else {
        return unknown();
    };
    let entries = match desk.entries(&key).await {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(%error, "RFQ desk: a request's entries are unreadable");
            return unavailable("the request's entries could not be read");
        }
    };
    let receipt = match entries.first().map(receipt_of).transpose() {
        Ok(Some(Some(receipt))) => receipt,
        Ok(_) => return unknown(),
        Err(_) => return unavailable("the request's entries could not be read"),
    };
    let book_id = &receipt.request.book_id;
    if let Err(response) = limited_book_record(&state, &ctx, book_id).await {
        return response;
    }
    if let Err(response) = authorize(
        &state,
        &ctx,
        Action::BookRead,
        STATUS_DATA,
        book_id,
        [receipt.instrument_id.as_str().to_owned()],
    ) {
        return response;
    }
    let records: Vec<serde_json::Value> = entries
        .iter()
        .filter_map(|entry| {
            let mut record: serde_json::Value =
                serde_json::from_str(entry.details.as_deref()?).ok()?;
            record["record_id"] = serde_json::json!(entry.id);
            record["state"] = serde_json::json!(entry.action);
            record["user"] = serde_json::json!(entry.user);
            Some(record)
        })
        .collect();
    Json(serde_json::json!({ "rfq_key": key, "records": records })).into_response()
}
