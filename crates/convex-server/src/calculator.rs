//! The bond calculator: `POST /api/v1/bonds/:instrument_id/calculate`. Given one level of
//! a stored bond, by any method, it reports the others.
//!
//! Transport only. For a fixed bullet, zero or sinking-fund bond, the level is priced by
//! the engine's level stage, whatever the bond's rule quotes it by: a clean price is taken
//! as given (a sinking-fund bond's per 100 current face, valued per 100 original face at
//! its factor), and a yield, Z-spread or benchmark spread is priced through the same
//! definitions as a quote.
//! The router then values that price under the approved context at `known_at`, mid only,
//! with the G-spread over the set the bond's rule names. A callable is calculated on its
//! workouts, from a price or a yield to worst, with each workout's yield; it reads no
//! curve. A floating rate note from a price or the discount margin its rule quotes, and a
//! callable the HW1F model values from an OAS whatever its rule, are valued as
//! `GET /quotes/:instrument_id` values them: by the runtime's request valuation, a
//! callable's OAS on HW1F trees. The
//! calculator is read-only and stores nothing; it reports the reference,
//! configuration, activation, curve, rule and set versions it read in headers, as
//! `GET /quotes/:instrument_id` does.

use std::sync::Arc;

use axum::{
    extract::{rejection::JsonRejection, Path, State},
    response::{IntoResponse, Response},
    Json,
};
use convex_analytics::workflow::PerpetualWorkout;
use convex_core::contracts::{SourceId, UtcTimestamp};
use convex_core::ids::InstrumentId;
use convex_core::Date;
use convex_engine::level_pricing::{BenchmarkYield, PricingRuleInForce, QuotedInput};
use convex_engine::perpetual_valuation::{
    calculate_perpetual, PerpetualPath, PERPETUAL_SNAPSHOT_MODEL,
};
use convex_engine::ports::{
    level_pricing::{
        is_router_valued, is_workout_callable, PricingMethod, NOT_A_WORKOUT_CALLABLE,
        RULE_DOES_NOT_APPLY,
    },
    reference_data::{BondReferenceData, BondType},
    reference_master::{ReferenceLookup, ResolvedBondReference},
};
use convex_engine::pricing_router::PricingInput;
use convex_engine::workout_valuation::{
    calculate_on_workouts, Redemption, Workout, WORKOUT_SNAPSHOT_MODEL,
};
use convex_engine::{PricingEngine, RequestContext};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::curves::known_at_or_now;
use crate::error::ApiError;
use crate::handlers::{
    bond_quote_body, runtime_composed, value_stored, with_stored_quote_headers, AppState,
};
use crate::level_pricing::{benchmark_json, rule_head_json, rule_read_json, rule_with_set_json};

/// The largest calculator request body, in bytes.
pub(crate) const MAX_CALCULATOR_REQUEST_BYTES: usize = 64 * 1024;

/// One calculation: a level of the bond, by the method it is quoted in, at a settlement.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculatorRequest {
    /// What `level` is quoted by. It need not be the method the bond's rule quotes.
    pub method: PricingMethod,
    /// The level, in the method's unit: a clean price per 100 face for `price`, a decimal
    /// yield for `yield`, and basis points for `z_spread`, `benchmark_spread`,
    /// `discount_margin` and `oas`.
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
/// The reply is the bond's model's measures at the level: the router's for a bullet or
/// zero, the workout model's for a callable by price or yield, the HW1F model's for a
/// callable by OAS, the floating model's for a note. It adds the `input` as priced and the
/// bond's `pricing_rule` as read when it has one; for a bullet, zero or callable on its
/// workouts, each `workout` with the `worst`.
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
    let mut reference = engine
        .reference_at_for_pricing(
            &InstrumentId::new(instrument_id),
            ReferenceLookup {
                as_of: known_at,
                known_at,
            },
        )
        .await?
        .ok_or_else(not_found)?;
    let bond = reference.record.bond.as_ref().ok_or_else(not_found)?;
    // A bond revised since its rule was committed so that the rule no longer applies is
    // refused, whatever the method.
    if reference.is_rule_misapplied() {
        return Err(ApiError::invalid(RULE_DOES_NOT_APPLY));
    }
    let is_floating = bond.bond_type == BondType::FloatingRate;
    // A note, and a callable's OAS on HW1F trees, are valued by the runtime.
    let is_by_runtime = is_floating
        || (bond.bond_type == BondType::FixedCallable && request.method == PricingMethod::Oas {});
    let is_on_workouts = is_workout_callable(bond);
    let is_perpetual = bond.is_perpetual();
    if !is_by_runtime && !is_on_workouts && !is_perpetual && !is_router_valued(bond) {
        // Only the bonds the router, the workout models, the HW1F model or the floating
        // model values are calculated.
        return Err(ApiError::unsupported_product(format!(
            "the calculator values fixed bullets and zeros with no embedded terms, sinking-fund bonds with mandatory sinks, floating rate notes, callables on their workouts, callables by OAS and perpetuals: {NOT_A_WORKOUT_CALLABLE}"
        )));
    }
    if is_by_runtime {
        // The runtime reads a note's margin in its rule's field, so it takes one only under
        // a margin rule; a callable takes an OAS whatever its rule. A level's range is
        // checked as for a quote.
        if is_floating
            && request.method.is_level()
            && reference.pricing_method() != Some(&request.method)
        {
            return Err(ApiError::invalid(
                "a floating rate note is calculated from its price, or from a discount margin under a discount_margin pricing rule",
            ));
        }
        runtime_composed(engine)?;
    }
    let mut context = engine
        .request_context(request.configuration.as_ref(), known_at)
        .await?;
    // The runtime's valuation reads the rule's pin from the reference again.
    let pin = if is_by_runtime {
        reference.pricing_rule.clone()
    } else {
        reference.pricing_rule.take()
    };
    let in_force = engine
        .rule_in_force_mid_only(&mut context, bond, pin)
        .await?;
    let body = if is_by_runtime {
        calculate_by_runtime(
            engine,
            &mut context,
            &request,
            &reference,
            known_at,
            in_force.as_ref(),
        )
        .await?
    } else if is_on_workouts {
        calculate_callable(&request, &reference, bond, in_force.as_ref())?
    } else if is_perpetual {
        calculate_perpetual_quote(engine, &request, &reference, bond, in_force.as_ref())?
    } else {
        let bond = reference.record.bond.take().ok_or_else(not_found)?;
        calculate_by_router(engine, &mut context, &request, bond, in_force.as_ref()).await?
    };
    with_stored_quote_headers(
        Json(body).into_response(),
        reference.record.version,
        &context,
        known_at,
        in_force.as_ref(),
    )
}

/// A fixed bullet, zero or sinking-fund bond: the level priced by the stage, then valued by
/// the router under the context's curves.
async fn calculate_by_router(
    engine: &PricingEngine,
    context: &mut RequestContext,
    request: &CalculatorRequest,
    bond: BondReferenceData,
    in_force: Option<&PricingRuleInForce>,
) -> Result<Value, ApiError> {
    let curves = context.curves_for(&bond).await?;
    let mut pricing = engine.price_by_method(
        &bond,
        in_force,
        &request.method,
        request.level,
        request.settlement_date,
        Some(&curves.discount),
    )?;
    let maturity_date = bond
        .maturity_date
        .ok_or_else(|| ApiError::calculation("the router values dated bonds only"))?;
    let mut input = PricingInput::with_mid_price(
        bond,
        request.settlement_date,
        Some(pricing.clean_price_mid),
        Some(curves.discount),
        curves.benchmark,
        pricing.g_spread_set.take(),
        None,
    );
    pricing
        .bid_ask
        .apply(&mut input, context.config().bid_ask_spread.as_ref());
    let quote = engine
        .pricing_router()
        .price(&input)
        .map_err(|e| ApiError::calculation(format!("Pricing failed: {e}")))?;
    let mut body = bond_quote_body(&quote)?;
    body["input"] = input_json(request, pricing.benchmark.as_ref());
    if let Some(in_force) = in_force {
        body["pricing_rule"] = rule_read_json(in_force, &pricing);
    }
    // A bullet, zero or sinking-fund bond has no option, so its one workout, and its
    // worst, is its yield to maturity: a sinking-fund bond's on its sinking cash.
    let maturity = workout_json(Redemption::Maturity, maturity_date, quote.ytm_mid);
    body["worst"] = maturity.clone();
    body["workouts"] = json!([maturity]);
    Ok(body)
}

/// A callable on its workouts, from a price or a yield to worst: its valuation, each call's
/// and maturity's yield at its clean price, and the worst with the years to it.
fn calculate_callable(
    request: &CalculatorRequest,
    reference: &ResolvedBondReference,
    bond: &BondReferenceData,
    in_force: Option<&PricingRuleInForce>,
) -> Result<Value, ApiError> {
    let calculation = calculate_on_workouts(
        reference,
        &request.method,
        request.level,
        request.settlement_date,
    )?;
    let mut body = bond_quote_body(&calculation.bond_quote(bond, request.settlement_date))?;
    body["input"] = input_json(request, None);
    if let Some(in_force) = in_force {
        body["pricing_rule"] = rule_head_json(WORKOUT_SNAPSHOT_MODEL, in_force);
    }
    let row = |workout: &Workout| {
        workout_json(
            workout.redemption,
            workout.date,
            Decimal::from_f64_retain(workout.decimal_yield),
        )
    };
    body["workouts"] = calculation.workouts.iter().map(row).collect();
    body["worst"] = row(&calculation.worst);
    body["worst"]["years"] = json!(calculation.valuation.years_to_worst);
    Ok(body)
}

/// A perpetual on its workouts, from a price or a yield to worst: its valuation, each call's
/// yield through the horizon and the perpetuity's at its clean price, the worst, and the
/// G-spread at the worst call over the set its rule names.
fn calculate_perpetual_quote(
    engine: &PricingEngine,
    request: &CalculatorRequest,
    reference: &ResolvedBondReference,
    bond: &BondReferenceData,
    in_force: Option<&PricingRuleInForce>,
) -> Result<Value, ApiError> {
    let calculation = calculate_perpetual(
        reference,
        &request.method,
        request.level,
        request.settlement_date,
    )?;
    let (g_spread, g_spread_refused) = match engine.perpetual_g_spread(
        bond,
        in_force,
        &calculation.valuation,
        request.settlement_date,
    ) {
        Some(Ok(spread)) => (Some(spread), None),
        Some(Err(refused)) => (None, Some(refused.to_string())),
        None => (None, None),
    };
    let mut quote = calculation.bond_quote(bond, request.settlement_date);
    quote.g_spread_mid = g_spread;
    let mut body = bond_quote_body(&quote)?;
    body["input"] = input_json(request, None);
    if let Some(in_force) = in_force {
        body["pricing_rule"] = rule_with_set_json(
            PERPETUAL_SNAPSHOT_MODEL,
            in_force,
            g_spread_refused.as_deref(),
        );
    }
    let row = |path: &PerpetualPath| perpetual_path_json(path, calculation.call_price);
    body["workouts"] = calculation.paths.iter().map(row).collect();
    body["worst"] = row(&calculation.worst);
    body["horizon"] = json!(calculation.horizon);
    Ok(body)
}

/// One path of a perpetual: a call at its price, as a callable's, or the perpetuity, with
/// the yield to it.
fn perpetual_path_json(path: &PerpetualPath, call_price: Decimal) -> Value {
    let decimal_yield = Decimal::from_f64_retain(path.decimal_yield);
    match path.workout {
        PerpetualWorkout::Call { date } => {
            workout_json(Redemption::Call { call_price }, date, decimal_yield)
        }
        PerpetualWorkout::Perpetuity => json!({"kind": "perpetuity", "yield": decimal_yield}),
    }
}

/// A floating rate note from a price or its rule's discount margin, or a callable from an
/// OAS whatever its rule, valued as `GET /quotes/:id` values it: an unpublished snapshot of
/// the curves its model reads and a note's fixings, valued by the runtime.
async fn calculate_by_runtime(
    engine: &PricingEngine,
    context: &mut RequestContext,
    request: &CalculatorRequest,
    reference: &ResolvedBondReference,
    known_at: UtcTimestamp,
    in_force: Option<&PricingRuleInForce>,
) -> Result<Value, ApiError> {
    let quoted = if request.method == (PricingMethod::Oas {}) {
        QuotedInput::Oas(request.level)
    } else {
        QuotedInput::quoted_by(&request.method, request.level)
    };
    let valued = value_stored(
        engine,
        context,
        reference,
        request.settlement_date,
        known_at,
        Some(quoted),
    )
    .await?;
    let mut body = valued.body;
    body["input"] = input_json(request, None);
    // The rule as read, with the model the request was valued on: an OAS puts a callable
    // on HW1F trees whatever its rule.
    if let Some(in_force) = in_force {
        body["pricing_rule"] = rule_head_json(&valued.model_id, in_force);
    }
    Ok(body)
}

/// The level as sent, and the benchmark a spread was quoted to.
fn input_json(request: &CalculatorRequest, benchmark: Option<&BenchmarkYield>) -> Value {
    let mut input = json!({"method": request.method, "level": request.level});
    if let Some(benchmark) = benchmark {
        input["benchmark"] = benchmark_json(benchmark);
    }
    input
}

/// One workout: maturity, or a call at its price, with the yield to its date.
fn workout_json(redemption: Redemption, date: Date, decimal_yield: Option<Decimal>) -> Value {
    match redemption {
        Redemption::Maturity => json!({"kind": "maturity", "date": date, "yield": decimal_yield}),
        Redemption::Call { call_price } => json!({
            "kind": "call",
            "date": date,
            "call_price": call_price,
            "yield": decimal_yield,
        }),
    }
}
