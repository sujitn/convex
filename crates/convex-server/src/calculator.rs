//! The bond calculator: `POST /api/v1/bonds/:instrument_id/calculate`. Given one level of
//! a stored bond, by any method, it reports the others.
//!
//! Transport only. The level is priced by the engine's level stage, whatever the bond's
//! rule quotes it by: a clean price is taken as given, and a yield, Z-spread or benchmark
//! spread is priced through the same definitions as a quote. The router then values that
//! price under the approved context at `known_at`, mid only, with the G-spread over the
//! set the bond's rule names. The calculator is read-only and stores nothing; it reports
//! the reference, configuration, activation, curve, rule and set versions it read in
//! headers, as `GET /quotes/:instrument_id` does. Fixed bullets and zeros are calculated;
//! a zero by its price only.

use std::sync::Arc;

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    response::{IntoResponse, Response},
    Json,
};
use convex_core::contracts::{SourceId, UtcTimestamp};
use convex_core::ids::InstrumentId;
use convex_core::Date;
use convex_engine::level_pricing::RulePricing;
use convex_engine::ports::{
    level_pricing::{is_quoted_two_way, PricingMethod},
    output::BondQuoteOutput,
    reference_master::ReferenceLookup,
};
use convex_engine::pricing_router::PricingInput;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::curves::known_at_or_now;
use crate::error::ApiError;
use crate::handlers::{bond_quote_json, with_stored_quote_headers, AppState};
use crate::level_pricing::{benchmark_json, rule_read_json};

/// The largest calculator request body, in bytes.
pub(crate) const MAX_CALCULATOR_REQUEST_BYTES: usize = 64 * 1024;

/// Why a bond is not calculated: only the bonds the router values are.
const NOT_CALCULATED: &str = "the calculator values fixed bullets and zeros with no embedded terms; callables, floating rate notes and other bonds are not calculated";

/// One calculation: a level of the bond, by the method it is quoted in, at a settlement.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculatorRequest {
    /// What `level` is quoted by. It need not be the method the bond's rule quotes.
    pub method: PricingMethod,
    /// The level, in the method's unit: a clean price per 100 face for `price`, a decimal
    /// yield for `yield`, and basis points for `z_spread` and `benchmark_spread`.
    pub level: Decimal,
    /// The settlement date the level is quoted for.
    pub settlement_date: Date,
    /// Activated configuration whose curves and conventions value the bond. Defaults to
    /// the one the runtime runs.
    pub configuration: Option<SourceId>,
    /// Information cutoff for the reference, rule, set and curve revisions. Defaults to
    /// now.
    pub known_at: Option<UtcTimestamp>,
}

/// Calculate a stored bond from one level under its approved valuation context, mid only.
/// The reply is the router's measures at the level's price, the `input` as priced, the
/// bond's `pricing_rule` as read when it has one, and each `workout` with the `worst`.
pub async fn calculate_bond(
    State(state): State<Arc<AppState>>,
    Path(instrument_id): Path<String>,
    request: Result<Json<CalculatorRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let started = std::time::Instant::now();
    let response = calculate(&state, &instrument_id, request).await;
    convex_engine::telemetry::observe(convex_engine::telemetry::BOND_CALCULATOR_SECONDS, started);
    response
}

async fn calculate(
    state: &AppState,
    instrument_id: &str,
    request: Result<Json<CalculatorRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) = request?;
    let engine = &state.engine;
    let known_at = known_at_or_now(request.known_at)?;
    let not_found = || ApiError::not_found(format!("Bond not found: {instrument_id}"));
    let reference = engine
        .reference_at_for_pricing(
            &InstrumentId::new(instrument_id),
            ReferenceLookup {
                as_of: known_at,
                known_at,
            },
        )
        .await?
        .ok_or_else(not_found)?;
    let bond = reference.record.bond.ok_or_else(not_found)?;
    if !is_quoted_two_way(&bond) {
        return Err(ApiError::unsupported_product(NOT_CALCULATED));
    }
    let mut context = engine
        .request_context(request.configuration.as_ref(), known_at)
        .await?;
    let in_force = engine
        .rule_in_force_mid_only(&mut context, reference.pricing_rule)
        .await?;
    let curves = context.curves_for(&bond).await?;
    let mut pricing = engine.price_by_method(
        &bond,
        in_force
            .as_ref()
            .and_then(|in_force| in_force.benchmark_set.as_deref()),
        &request.method,
        request.level,
        request.settlement_date,
        Some(&curves.discount),
    )?;
    let mut input = PricingInput::with_mid_price(
        bond,
        request.settlement_date,
        Some(pricing.clean_price_mid),
        Some(curves.discount),
        curves.benchmark,
        pricing.government_curve.take(),
        None,
    );
    pricing
        .bid_ask
        .apply(&mut input, context.config().bid_ask_spread.as_ref());
    let quote = engine
        .pricing_router()
        .price(&input)
        .map_err(|e| ApiError::calculation(format!("Pricing failed: {e}")))?;
    let mut body =
        bond_quote_json(&quote).map_err(|_| ApiError::unavailable("quote serialization"))?;
    body["input"] = input_json(&request, &pricing);
    if let Some(in_force) = &in_force {
        body["pricing_rule"] = rule_read_json(in_force, &pricing);
    }
    add_workouts(&mut body, &quote, input.bond.maturity_date);
    with_stored_quote_headers(
        Json(body).into_response(),
        reference.record.version,
        &context,
        known_at,
        in_force.as_ref(),
    )
}

/// The level as sent, and the benchmark a spread was quoted to.
fn input_json(request: &CalculatorRequest, pricing: &RulePricing) -> Value {
    let mut input = json!({"method": request.method, "level": request.level});
    if let Some(benchmark) = &pricing.benchmark {
        input["benchmark"] = benchmark_json(benchmark);
    }
    input
}

/// Each date the bond can be worked out to, with its yield, and the worst of them. A
/// bullet or zero is redeemed only at maturity, so its one workout, and its worst, is its
/// yield to maturity.
fn add_workouts(body: &mut Value, quote: &BondQuoteOutput, maturity_date: Date) {
    let maturity = json!({"kind": "maturity", "date": maturity_date, "yield": quote.ytm_mid});
    body["worst"] = maturity.clone();
    body["workouts"] = json!([maturity]);
}
