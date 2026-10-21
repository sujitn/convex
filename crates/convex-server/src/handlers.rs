//! Request handlers.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Datelike;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_analytics::risk::{Duration as AnalyticsDuration, KeyRateDuration, KeyRateDurations};
use convex_bonds::types::BondIdentifiers;
use convex_core::Currency;
use convex_core::Date;
use convex_engine::{Portfolio, Position, PricingEngine};
use convex_ext_file::{InMemoryPortfolioStore, PortfolioFilter, StoredPortfolio, StoredPosition};
use convex_portfolio::{
    active_weights,
    // Key rate duration
    aggregate_key_rate_exposure,
    analyze_basket,
    // Benchmark comparison
    benchmark_comparison,
    bucket_by_country,
    bucket_by_currency,
    bucket_by_issuer,
    bucket_by_maturity,
    bucket_by_rating,
    bucket_by_sector,
    build_creation_basket,
    // Credit quality analytics
    calculate_credit_quality,
    // Liquidity analytics
    calculate_liquidity_metrics,
    calculate_migration_risk,
    // ETF analytics
    calculate_sec_yield,
    cs01_contributions,
    duration_contributions,
    duration_difference_by_sector,
    dv01_contributions,
    estimate_tracking_error,
    liquidity_distribution,
    spread_contributions,
    spread_difference_by_sector,
    AnalyticsConfig,
    BucketContribution,
    BucketMetrics,
    Classification,
    CreditRating,
    Cs01Contributions,
    DurationContributions,
    DurationMeasure,
    Dv01Contributions,
    Holding,
    HoldingAnalytics,
    HoldingBuilder,
    HoldingContribution,
    RatingBucket,
    RatingInfo,
    SecYieldInput,
    Sector,
    SpreadContributions,
    SpreadMeasure,
};

use crate::websocket::WebSocketState;
use convex_core::ids::{EtfId, InstrumentId, PortfolioId};
use convex_engine::ports::output::{BondQuoteOutput, EtfQuoteOutput, PortfolioAnalyticsOutput};
use convex_engine::ports::reference_data::{BondReferenceData, EtfHoldingEntry, EtfHoldings};

/// Application state.
pub struct AppState {
    /// Bounded supplied-input calculations, using the engine's concurrency setting.
    pub(crate) analytics_slots: Arc<tokio::sync::Semaphore>,
    /// Shared current authentication/authorization boundary.
    pub access: Arc<crate::access::AccessControl>,
    /// The pricing engine
    pub engine: Arc<PricingEngine>,
    /// WebSocket state for real-time streaming
    pub ws_state: WebSocketState,
    /// Portfolio store (for CRUD operations)
    pub portfolio_store: Arc<InMemoryPortfolioStore>,
}

type PortfolioHandlerError = (StatusCode, Json<serde_json::Value>);
fn portfolio_result<T, E: std::fmt::Display>(
    result: Result<T, E>,
) -> Result<T, PortfolioHandlerError> {
    result.map_err(|error| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"code":"INVALID_PORTFOLIO_INPUT","error":error.to_string()})),
        )
    })
}

/// Health check response.
#[derive(Serialize)]
pub struct HealthResponse {
    status: String,
    version: String,
}

/// Health check handler.
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Readiness fails independently of process liveness.
pub async fn readiness(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    crate::health::readiness(State(state)).await
}
/// Full low-cardinality runtime lifecycle and input condition.
pub async fn runtime_health(
    State(state): State<Arc<AppState>>,
) -> Json<convex_engine::RuntimeHealth> {
    Json(state.engine.runtime_health())
}
/// Canonical display resynchronization, including explicit errors and stale retained values.
pub async fn runtime_results(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<convex_engine::ports::runtime::PublishedValuation>> {
    Json(state.engine.runtime_results())
}
/// Supplied-context request uses precisely the runtime's quantitative path.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeValuationRequest {
    /// Verified frozen information set.
    pub snapshot: convex_engine::ports::snapshot::MarketSnapshot,
    /// Complete context, including settlement/build/measures.
    pub context: convex_engine::context::ValuationContext,
    /// Correlation identity.
    pub request_id: convex_core::contracts::RequestId,
    /// Explicit calculation time; cannot precede the context cutoffs.
    pub calculated_at: convex_core::contracts::UtcTimestamp,
}
/// Evaluate immutable supplied inputs; this never admits trading or edits current runtime state.
pub async fn runtime_value(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RuntimeValuationRequest>,
) -> impl IntoResponse {
    match state
        .engine
        .evaluate_context(
            request.snapshot,
            request.context,
            request.request_id,
            request.calculated_at,
        )
        .await
    {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(convex_engine::ports::TraitError::CapacityExceeded) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error":"RUNTIME_CAPACITY_EXCEEDED"})),
        )
            .into_response(),
        Err(_) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":"INVALID_RUNTIME_CONTEXT"})),
        )
            .into_response(),
    }
}

/// Error response.
#[derive(Serialize)]
pub struct ErrorResponse {
    error: String,
}

impl ErrorResponse {
    #[allow(dead_code)]
    fn new(message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
        }
    }
}

/// Query parameters for single bond quote.
#[derive(Debug, Deserialize)]
pub struct BondQuoteQuery {
    /// Settlement date (YYYY-MM-DD). Defaults to the `known_at` date.
    pub settlement_date: Option<String>,
    /// Market price for spread calculations.
    pub market_price: Option<Decimal>,
    /// Information cutoff for reference and curve revisions. Defaults to now.
    pub known_at: Option<convex_core::contracts::UtcTimestamp>,
}

pub(crate) fn bond_quote_json(
    quote: &convex_engine::ports::output::BondQuoteOutput,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serde_json::to_value(quote)?;
    value["dirty_price_mid"] = serde_json::json!(quote.dirty_price_mid());
    value["dirty_price_bid"] = serde_json::json!(quote.dirty_price_bid());
    value["dirty_price_ask"] = serde_json::json!(quote.dirty_price_ask());
    Ok(value)
}

/// Price a stored bond with the curves its applicable pricing configuration
/// names, current at `known_at` (now when absent). The response reports the
/// reference, configuration and curve versions consumed; a missing or stale
/// curve refuses the request rather than pricing without it.
pub async fn get_bond_quote(
    State(state): State<Arc<AppState>>,
    Path(instrument_id): Path<String>,
    query: Result<axum::extract::Query<BondQuoteQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<axum::response::Response, crate::error::ApiError> {
    use crate::curves::{known_at_or_now, rfc3339, with_header};
    use crate::error::ApiError;
    use convex_engine::ports::reference_master::ReferenceLookup;
    use convex_engine::pricing_router::PricingInput;

    let axum::extract::Query(query) = query.map_err(|e| ApiError::invalid(e.body_text()))?;
    let known_at = known_at_or_now(query.known_at)?;
    let lookup = ReferenceLookup {
        as_of: known_at,
        known_at,
    };
    let reference = state
        .engine
        .reference_at_for_pricing(&InstrumentId::new(&instrument_id), lookup)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Bond not found: {instrument_id}")))?;
    let bond = reference
        .bond
        .ok_or_else(|| ApiError::not_found(format!("Bond not found: {instrument_id}")))?;
    let market_price = query.market_price.ok_or_else(|| {
        ApiError::missing_input("market_price is required; no model or market value is substituted")
    })?;
    let settlement_date = match query.settlement_date {
        Some(ref text) => parse_date(text).map_err(ApiError::invalid)?,
        None => {
            let day = known_at.datetime().date_naive();
            Date::from_ymd(day.year(), day.month(), day.day())
                .map_err(|e| ApiError::invalid(e.to_string()))?
        }
    };
    let curves = state.engine.pricing_curves(&bond, known_at).await?;
    let input = PricingInput {
        bond,
        settlement_date,
        market_price_bid: None,
        market_price_mid: Some(market_price),
        market_price_ask: None,
        discount_curve: Some(curves.discount),
        benchmark_curve: curves.benchmark,
        government_curve: None,
        volatility: None,
        bid_ask_config: curves.config.bid_ask_spread.clone(),
    };
    let quote = state
        .engine
        .pricing_router()
        .price(&input)
        .map_err(|e| ApiError::calculation(format!("Pricing failed: {e}")))?;
    // Retired with REST-triggered WebSocket messages in P3. A historical
    // cutoff is not a live quote, so it is never broadcast.
    if query.known_at.is_none() {
        state.ws_state.publish_bond_quote(quote.clone());
    }
    let value =
        bond_quote_json(&quote).map_err(|_| ApiError::unavailable("quote serialization"))?;
    let pins: Vec<_> = curves
        .curves
        .iter()
        .map(|(id, version)| format!("{id}@{}", version.get()))
        .collect();
    let config = format!("{}@{}", curves.config.config_id, curves.config.version);
    let response = Json(value).into_response();
    let response = with_header(
        response,
        "x-convex-reference-version",
        &reference.version.get().to_string(),
    )?;
    let response = with_header(response, "x-convex-pricing-config", &config)?;
    let response = with_header(response, "x-convex-curve-versions", &pins.join(","))?;
    with_header(response, "x-convex-known-at", &rfc3339(known_at))
}

// =============================================================================
// SINGLE BOND PRICING (POST)
// =============================================================================

/// Request for single bond pricing.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SingleBondPricingRequest {
    /// Bond reference data
    pub bond: BondReferenceData,
    /// Settlement date (YYYY-MM-DD)
    pub settlement_date: String,
    /// Market price (optional, for spread calculations)
    pub market_price: Option<Decimal>,
    /// Explicit supplied discount curve and currency; no repository alias resolution.
    pub discount_curve: Option<BondCurveInput>,
    /// Explicit supplied swap benchmark, independently mapped.
    pub swap_curve: Option<BondCurveInput>,
    /// Actual government benchmark yields.
    pub government_curve: Option<GovernmentCurveInput>,
    /// Explicit model volatility; optional products currently require CVX-055.
    pub volatility: Option<Decimal>,
}

/// Exact supplied curve revision mapped to a currency and pricing role by the request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BondCurveInput {
    /// Currency must match the instrument.
    pub currency: Currency,
    /// Validated ACT/365F continuous-zero snapshot record, including version/policies.
    pub curve: convex_engine::ports::snapshot::SnapshotCurve,
}

/// Supplied securities and observed yields, rather than inferred discount zeros.
pub type GovernmentCurveInput = convex_analytics::dto::GovernmentCurveSpec;

/// Price a single bond (POST endpoint).
///
/// Use this when you have bond reference data and want to price on-demand.
pub async fn price_single_bond(
    State(state): State<Arc<AppState>>,
    request: Result<Json<SingleBondPricingRequest>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    use convex_engine::curve_builder::BuiltCurve;
    use convex_engine::pricing_router::PricingInput;

    let request = match request {
        Ok(Json(request)) => request,
        Err(error) => {
            return (
                error.status(),
                Json(
                    serde_json::json!({"code":"INVALID_INPUT","error":error.body_text().chars().take(2048).collect::<String>()}),
                ),
            )
        }
    };

    if let Err(error) = state.engine.validate_inline_reference(&request.bond).await {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": error.to_string(), "code": "INVALID_REFERENCE"})),
        );
    }

    // Parse settlement date
    let settlement_date = match parse_date(&request.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    let resolve = |supplied: &Option<BondCurveInput>| -> Result<Option<BuiltCurve>, String> {
        supplied
            .as_ref()
            .map(|value| {
                if value.currency != request.bond.currency {
                    return Err("curve currency does not match bond".into());
                }
                if value.curve.reference_date.date() > settlement_date {
                    return Err("curve reference follows settlement".into());
                }
                if matches!(
                    value.curve.quality,
                    convex_core::contracts::DataQuality::Invalid
                        | convex_core::contracts::DataQuality::Stale
                ) || value
                    .curve
                    .recorded_at
                    .datetime()
                    .signed_duration_since(value.curve.source_time.datetime())
                    .num_milliseconds()
                    > value.curve.max_age_ms
                {
                    return Err("supplied curve is invalid or stale at publication".into());
                }
                BuiltCurve::from_snapshot(&value.curve).map_err(|e| e.to_string())
            })
            .transpose()
    };
    if let (Some(discount), Some(swap)) = (&request.discount_curve, &request.swap_curve) {
        if discount.curve.curve_id == swap.curve.curve_id
            && discount.curve.version == swap.curve.version
            && discount.curve != swap.curve
        {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(
                    serde_json::json!({"code":"INVALID_INPUT","error":"conflicting contents for the same curve revision"}),
                ),
            );
        }
    }
    let (discount, swap) = match (
        resolve(&request.discount_curve),
        resolve(&request.swap_curve),
    ) {
        (Ok(discount), Ok(swap)) => (discount, swap),
        (Err(error), _) | (_, Err(error)) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"code":"INVALID_INPUT","error":error})),
            )
        }
    };
    let government = match request.government_curve {
        Some(value) => {
            match convex_analytics::bond_quote::government_curve(
                &value,
                settlement_date,
                request.bond.currency,
            ) {
                Ok(curve) => Some(curve),
                Err(error) => {
                    return (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        Json(serde_json::json!({"code":"INVALID_INPUT","error":error.to_string()})),
                    )
                }
            }
        }
        None => None,
    };
    let curve_versions: Vec<_> = [("discount",request.discount_curve.as_ref()),("swap",request.swap_curve.as_ref())].into_iter().filter_map(|(role,input)|input.map(|v|serde_json::json!({"role":role,"currency":v.currency,"curve_id":v.curve.curve_id,"version":v.curve.version,"source":v.curve.source,"model_id":v.curve.model_id}))).collect();
    let input = PricingInput::with_mid_price(
        request.bond,
        settlement_date,
        request.market_price,
        discount,
        swap,
        government,
        request.volatility,
    );

    // Price the bond
    let router = state.engine.pricing_router();
    match router.price(&input) {
        Ok(quote) => {
            // Publish to WebSocket subscribers
            state.ws_state.publish_bond_quote(quote.clone());
            match bond_quote_json(&quote) {
                Ok(mut response) => {
                    if !curve_versions.is_empty() {
                        response["curve_versions"] = serde_json::json!(curve_versions);
                    }
                    (StatusCode::OK, Json(response))
                }
                Err(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(
                        serde_json::json!({"code":"UNAVAILABLE","error":"quote serialization failed"}),
                    ),
                ),
            }
        }
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "code":"INVALID_INPUT", "error": format!("Pricing failed: {}", e)
            })),
        ),
    }
}

// =============================================================================
// BATCH PRICING
// =============================================================================

/// Request for batch pricing.
#[derive(Debug, Deserialize)]
pub struct BatchPricingRequest {
    /// Bonds to price
    pub bonds: Vec<BondPricingItem>,
    /// Settlement date (YYYY-MM-DD)
    pub settlement_date: String,
    /// Use parallel processing
    #[serde(default = "default_parallel")]
    pub parallel: bool,
}

fn default_parallel() -> bool {
    true
}

/// Individual bond in batch request.
#[derive(Debug, Deserialize)]
pub struct BondPricingItem {
    /// Bond reference data
    pub bond: BondReferenceData,
    /// Market price (optional, for spread calculations)
    pub market_price: Option<Decimal>,
}

/// Response for batch pricing.
#[derive(Debug, Serialize)]
pub struct BatchPricingResponse {
    /// Successful quotes
    pub quotes: Vec<BondQuoteOutput>,
    /// Errors
    pub errors: Vec<PricingError>,
    /// Statistics
    pub stats: BatchStats,
}

/// Pricing error for a specific bond.
#[derive(Debug, Clone, Serialize)]
pub struct PricingError {
    /// Instrument ID
    pub instrument_id: String,
    /// Error message
    pub error: String,
}

/// Batch processing statistics.
#[derive(Debug, Serialize)]
pub struct BatchStats {
    /// Total bonds submitted
    pub total: usize,
    /// Successfully priced
    pub succeeded: usize,
    /// Failed
    pub failed: usize,
    /// Elapsed time in milliseconds
    pub elapsed_ms: u64,
    /// Throughput (bonds per second)
    pub bonds_per_second: f64,
}

/// Batch pricing handler.
pub async fn batch_price(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BatchPricingRequest>,
) -> impl IntoResponse {
    use convex_engine::pricing_router::PricingInput;
    use std::time::Instant;

    // Parse settlement date
    let settlement_date = match parse_date(&request.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    // Build pricing inputs
    if request.bonds.len() > 1000 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":"batch limit is 1000 bonds"})),
        );
    }
    let mut validation_errors = Vec::with_capacity(request.bonds.len());
    for item in &request.bonds {
        validation_errors.push(
            state
                .engine
                .validate_inline_reference(&item.bond)
                .await
                .err(),
        );
    }
    let inputs: Vec<PricingInput> = request
        .bonds
        .into_iter()
        .map(|item| {
            PricingInput::with_mid_price(
                item.bond,
                settlement_date,
                item.market_price,
                None, // discount_curve
                None, // benchmark_curve
                None, // government_curve
                None, // volatility
            )
        })
        .collect();

    let router = state.engine.pricing_router();
    let start = Instant::now();

    // Execute pricing (parallel or sequential)
    let instrument_ids: Vec<_> = inputs
        .iter()
        .map(|input| input.bond.instrument_id.as_str().to_owned())
        .collect();
    let valid_inputs: Vec<_> = inputs
        .into_iter()
        .zip(&validation_errors)
        .filter(|(_, error)| error.is_none())
        .map(|(input, _)| input)
        .collect();
    let mut valid_results = if request.parallel {
        router.price_batch_parallel(&valid_inputs)
    } else {
        router.price_batch(&valid_inputs)
    }
    .into_iter();
    let results: Vec<_> = validation_errors
        .into_iter()
        .map(|error| match error {
            Some(error) => Err(convex_engine::EngineError::PricingError(error.to_string())),
            None => valid_results.next().unwrap_or_else(|| {
                Err(convex_engine::EngineError::Internal(
                    "missing batch result".into(),
                ))
            }),
        })
        .collect();

    let elapsed = start.elapsed();
    let elapsed_ms = elapsed.as_millis() as u64;

    // Separate successes and failures
    let mut quotes = Vec::new();
    let mut errors = Vec::new();

    for (result, instrument_id) in results.into_iter().zip(instrument_ids) {
        match result {
            Ok(quote) => {
                // Publish to WebSocket subscribers
                state.ws_state.publish_bond_quote(quote.clone());
                quotes.push(quote);
            }
            Err(e) => errors.push(PricingError {
                instrument_id,
                error: e.to_string(),
            }),
        }
    }

    let total = quotes.len() + errors.len();
    let bonds_per_second = if elapsed_ms > 0 {
        total as f64 / (elapsed_ms as f64 / 1000.0)
    } else {
        total as f64
    };

    let response = BatchPricingResponse {
        quotes,
        errors: errors.clone(),
        stats: BatchStats {
            total,
            succeeded: total - errors.len(),
            failed: errors.len(),
            elapsed_ms,
            bonds_per_second,
        },
    };

    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap()),
    )
}

// =============================================================================
// ETF iNAV
// =============================================================================

/// Request for ETF iNAV calculation.
#[derive(Debug, Deserialize)]
pub struct EtfInavRequest {
    /// ETF holdings
    pub holdings: EtfHoldingsInput,
    /// Bond prices (from separate pricing or market data)
    pub bond_prices: Vec<BondQuoteOutput>,
    /// Settlement date (YYYY-MM-DD)
    pub settlement_date: String,
}

/// ETF holdings input (simplified for API).
#[derive(Debug, Deserialize)]
pub struct EtfHoldingsInput {
    /// ETF identifier
    pub etf_id: String,
    /// ETF name
    pub name: String,
    /// Base currency (defaults to USD if not specified)
    #[serde(default = "default_currency")]
    pub currency: String,
    /// As-of date (YYYY-MM-DD)
    pub as_of_date: String,
    /// Holdings
    pub holdings: Vec<HoldingEntryInput>,
    /// Total market value
    pub total_market_value: Decimal,
    /// Shares outstanding
    pub shares_outstanding: Decimal,
    /// NAV per share
    pub nav_per_share: Option<Decimal>,
}

fn default_currency() -> String {
    "USD".to_string()
}

/// Holding entry input.
#[derive(Debug, Deserialize)]
pub struct HoldingEntryInput {
    /// Instrument ID
    pub instrument_id: String,
    /// Weight (0-1)
    pub weight: Decimal,
    /// Shares held
    pub shares: Decimal,
    /// Market value
    pub market_value: Decimal,
    /// Notional value
    pub notional_value: Decimal,
    /// Accrued interest
    pub accrued_interest: Option<Decimal>,
}

/// Calculate iNAV for an ETF.
pub async fn calculate_inav(
    State(state): State<Arc<AppState>>,
    Json(request): Json<EtfInavRequest>,
) -> impl IntoResponse {
    // Parse dates
    let settlement_date = match parse_date(&request.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    let as_of_date = match parse_date(&request.holdings.as_of_date) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid as_of_date: {}", e) })),
            );
        }
    };

    // Convert to internal types
    let holdings = EtfHoldings {
        etf_id: EtfId::new(&request.holdings.etf_id),
        name: request.holdings.name,
        currency: parse_currency(&request.holdings.currency),
        as_of_date,
        holdings: request
            .holdings
            .holdings
            .into_iter()
            .map(|h| EtfHoldingEntry {
                instrument_id: InstrumentId::new(&h.instrument_id),
                weight: h.weight,
                shares: h.shares,
                market_value: h.market_value,
                notional_value: h.notional_value,
                accrued_interest: h.accrued_interest,
            })
            .collect(),
        total_market_value: request.holdings.total_market_value,
        shares_outstanding: request.holdings.shares_outstanding,
        nav_per_share: request.holdings.nav_per_share,
        last_updated: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
        source: "api".to_string(),
    };

    let etf_pricer = state.engine.etf_pricer();

    match etf_pricer.calculate_inav(&holdings, &request.bond_prices, settlement_date) {
        Ok(output) => {
            // Publish to WebSocket subscribers
            state.ws_state.publish_etf_quote(output.clone());
            (StatusCode::OK, Json(serde_json::json!(output)))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// Batch calculate iNAV for multiple ETFs.
#[derive(Debug, Deserialize)]
pub struct BatchEtfInavRequest {
    /// List of ETFs with their holdings
    pub etfs: Vec<EtfHoldingsInput>,
    /// Bond prices (shared across all ETFs)
    pub bond_prices: Vec<BondQuoteOutput>,
    /// Settlement date (YYYY-MM-DD)
    pub settlement_date: String,
}

/// Batch iNAV response.
#[derive(Debug, Serialize)]
pub struct BatchEtfInavResponse {
    /// Successful calculations
    pub results: Vec<EtfQuoteOutput>,
    /// Errors
    pub errors: Vec<EtfInavError>,
}

/// ETF iNAV error.
#[derive(Debug, Serialize)]
pub struct EtfInavError {
    /// ETF ID
    pub etf_id: String,
    /// Error message
    pub error: String,
}

/// Batch iNAV calculation.
pub async fn batch_calculate_inav(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BatchEtfInavRequest>,
) -> impl IntoResponse {
    // Parse settlement date
    let settlement_date = match parse_date(&request.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    // Convert all holdings
    let mut holdings_list = Vec::new();
    for etf_input in request.etfs {
        let as_of_date = match parse_date(&etf_input.as_of_date) {
            Ok(d) => d,
            Err(_) => settlement_date, // Default to settlement
        };

        holdings_list.push(EtfHoldings {
            etf_id: EtfId::new(&etf_input.etf_id),
            name: etf_input.name,
            currency: parse_currency(&etf_input.currency),
            as_of_date,
            holdings: etf_input
                .holdings
                .into_iter()
                .map(|h| EtfHoldingEntry {
                    instrument_id: InstrumentId::new(&h.instrument_id),
                    weight: h.weight,
                    shares: h.shares,
                    market_value: h.market_value,
                    notional_value: h.notional_value,
                    accrued_interest: h.accrued_interest,
                })
                .collect(),
            total_market_value: etf_input.total_market_value,
            shares_outstanding: etf_input.shares_outstanding,
            nav_per_share: etf_input.nav_per_share,
            last_updated: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            source: "api".to_string(),
        });
    }

    let etf_pricer = state.engine.etf_pricer();
    let results =
        etf_pricer.calculate_inav_batch(&holdings_list, &request.bond_prices, settlement_date);

    let mut outputs = Vec::new();
    let mut errors = Vec::new();

    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(output) => {
                // Publish to WebSocket subscribers
                state.ws_state.publish_etf_quote(output.clone());
                outputs.push(output);
            }
            Err(e) => errors.push(EtfInavError {
                etf_id: holdings_list
                    .get(i)
                    .map(|h| h.etf_id.as_str().to_string())
                    .unwrap_or_else(|| format!("index_{}", i)),
                error: e.to_string(),
            }),
        }
    }

    let response = BatchEtfInavResponse {
        results: outputs,
        errors,
    };

    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap()),
    )
}

// =============================================================================
// PORTFOLIO ANALYTICS
// =============================================================================

/// Request for portfolio analytics.
#[derive(Debug, Deserialize)]
pub struct PortfolioAnalyticsRequest {
    /// Portfolio definition
    pub portfolio: PortfolioInput,
    /// Bond prices
    pub bond_prices: Vec<BondQuoteOutput>,
}

/// Portfolio input (simplified for API).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortfolioInput {
    /// Portfolio identifier
    pub portfolio_id: String,
    /// Portfolio name
    pub name: String,
    /// Reporting currency (USD, EUR, GBP, etc.)
    #[serde(default = "default_currency")]
    pub currency: String,
    /// Positions
    pub positions: Vec<PositionInput>,
}

/// Position input.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionInput {
    /// Instrument ID
    pub instrument_id: String,
    /// Notional/face value
    pub notional: Decimal,
    /// Sector classification
    pub sector: Option<String>,
    /// Credit rating
    pub rating: Option<String>,
    /// Country code (ISO 3166-1 alpha-3)
    pub country: Option<String>,
    /// Issuer name or ID
    pub issuer: Option<String>,
    /// Currency code (ISO 4217)
    pub currency: Option<String>,
}

/// Calculate analytics for a single portfolio.
pub async fn calculate_portfolio_analytics(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PortfolioAnalyticsRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    let portfolio = portfolio_result(convert_portfolio_input(&request.portfolio))?;
    let analyzer = state.engine.portfolio_analyzer();
    let output = portfolio_result(analyzer.calculate(&portfolio, &request.bond_prices))?;
    state.ws_state.publish_portfolio_analytics(output.clone());
    Ok((StatusCode::OK, Json(output)))
}

/// Request for batch portfolio analytics.
#[derive(Debug, Deserialize)]
pub struct BatchPortfolioAnalyticsRequest {
    /// List of portfolios
    pub portfolios: Vec<PortfolioInput>,
    /// Bond prices (shared across all portfolios)
    pub bond_prices: Vec<BondQuoteOutput>,
}

/// Response for batch portfolio analytics.
#[derive(Debug, Serialize)]
pub struct BatchPortfolioAnalyticsResponse {
    /// Exactly one outcome per input, in the original request order.
    pub outcomes: Vec<PortfolioAnalyticsOutcome>,
}

/// An ordered portfolio result or its original diagnostic.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PortfolioAnalyticsOutcome {
    /// A complete checked calculation for this input.
    Success {
        /// Zero-based request position, including repeated portfolio IDs.
        input_index: usize,
        /// Original supplied portfolio identity.
        portfolio_id: String,
        /// Complete native result with exact currency fields.
        result: Box<PortfolioAnalyticsOutput>,
    },
    /// A failed calculation retained at its original request position.
    Error {
        /// Zero-based request position, including repeated portfolio IDs.
        input_index: usize,
        /// Original supplied portfolio identity.
        portfolio_id: String,
        /// Actual admission or native calculation diagnostic.
        error: String,
    },
}

/// Batch calculate analytics for multiple portfolios.
pub async fn batch_calculate_portfolio_analytics(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BatchPortfolioAnalyticsRequest>,
) -> impl IntoResponse {
    let analyzer = state.engine.portfolio_analyzer();
    let mut outcomes = Vec::with_capacity(request.portfolios.len());
    for (input_index, input) in request.portfolios.iter().enumerate() {
        let result = convert_portfolio_input(input).and_then(|portfolio| {
            analyzer
                .calculate(&portfolio, &request.bond_prices)
                .map_err(|e| e.to_string())
        });
        match result {
            Ok(output) => {
                state.ws_state.publish_portfolio_analytics(output.clone());
                outcomes.push(PortfolioAnalyticsOutcome::Success {
                    input_index,
                    portfolio_id: input.portfolio_id.clone(),
                    result: Box::new(output),
                });
            }
            Err(error) => outcomes.push(PortfolioAnalyticsOutcome::Error {
                input_index,
                portfolio_id: input.portfolio_id.clone(),
                error,
            }),
        }
    }
    let response = BatchPortfolioAnalyticsResponse { outcomes };
    (StatusCode::OK, Json(response))
}

/// Request for duration contribution analysis.
#[derive(Debug, Deserialize)]
pub struct DurationContributionRequest {
    /// Portfolio definition
    pub portfolio: PortfolioInput,
    /// Bond prices
    pub bond_prices: Vec<BondQuoteOutput>,
}

/// Duration contribution entry.
#[derive(Debug, Serialize)]
pub struct DurationContributionEntry {
    /// Instrument ID
    pub instrument_id: String,
    /// Position weight
    pub weight: Decimal,
    /// Duration contribution
    pub contribution: Decimal,
}

/// Response for duration contribution analysis.
#[derive(Debug, Serialize)]
pub struct DurationContributionResponse {
    /// Contributions by position
    pub contributions: Vec<DurationContributionEntry>,
    /// Total portfolio duration
    pub total_duration: Decimal,
}

/// Calculate duration contribution for each position.
pub async fn calculate_duration_contribution(
    State(state): State<Arc<AppState>>,
    Json(request): Json<DurationContributionRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        let portfolio = portfolio_result(convert_portfolio_input(&request.portfolio))?;

        let analyzer = state.engine.portfolio_analyzer();
        let contributions =
            portfolio_result(analyzer.duration_contribution(&portfolio, &request.bond_prices))?;

        let mut exact_total = convex_math::decimal::BoundedDecimal::zero();
        for (_, _, value) in &contributions {
            portfolio_result(
                exact_total.add(&convex_math::decimal::BoundedDecimal::decimal(*value)),
            )?;
        }
        let total_duration = portfolio_result(exact_total.finish())?;
        let entries: Vec<DurationContributionEntry> = contributions
            .into_iter()
            .map(|(id, weight, contrib)| DurationContributionEntry {
                instrument_id: id.as_str().to_string(),
                weight,
                contribution: contrib,
            })
            .collect();

        let response = DurationContributionResponse {
            contributions: entries,
            total_duration,
        };

        (
            StatusCode::OK,
            Json(serde_json::to_value(response).unwrap()),
        )
    })
}

// =============================================================================
// RISK CONTRIBUTIONS (convex-portfolio)
// =============================================================================

/// Request for risk contribution analysis.
#[derive(Debug, Deserialize)]
pub struct RiskContributionsRequest {
    /// Portfolio definition
    pub portfolio: PortfolioInput,
    /// Bond prices with analytics
    pub bond_prices: Vec<BondQuoteOutput>,
    /// Contribution types to calculate (default: all)
    #[serde(default = "default_contribution_types")]
    pub contribution_types: Vec<String>,
    /// Required for requested duration allocation; no implicit family fallback.
    pub duration_measure: Option<DurationMeasure>,
    /// Required for requested spread allocation; no implicit spread substitution.
    pub spread_measure: Option<SpreadMeasure>,
}

fn default_contribution_types() -> Vec<String> {
    vec![
        "duration".to_string(),
        "dv01".to_string(),
        "spread".to_string(),
        "cs01".to_string(),
    ]
}

/// Serializable holding contribution for API response.
#[derive(Debug, Serialize)]
pub struct ApiHoldingContribution {
    /// Holding identifier
    pub id: String,
    /// Market value weight (0-1)
    pub weight: Option<f64>,
    /// Absolute contribution value
    pub contribution: f64,
    /// Contribution as percentage of total (0-100)
    pub contribution_pct: Option<f64>,
}

/// Serializable bucket contribution for API response.
#[derive(Debug, Serialize)]
pub struct ApiBucketContribution {
    /// Bucket name
    pub name: String,
    /// Number of holdings in bucket
    pub count: usize,
    /// Total weight of holdings (0-1)
    pub weight: Option<f64>,
    /// Absolute contribution
    pub contribution: f64,
    /// Contribution percentage (0-100)
    pub contribution_pct: Option<f64>,
}

/// Duration contributions response.
#[derive(Debug, Serialize)]
pub struct ApiDurationContributions {
    /// Explicit supplied measure used for every member.
    pub measure: DurationMeasure,
    /// Contributions by holding
    pub by_holding: Vec<ApiHoldingContribution>,
    /// Contributions by sector
    pub by_sector: Vec<ApiBucketContribution>,
    /// Contributions by rating
    pub by_rating: Vec<ApiBucketContribution>,
    /// Portfolio weighted average duration
    pub portfolio_duration: f64,
    /// Total portfolio market value
    pub total_market_value: String,
}

/// DV01 contributions response.
#[derive(Debug, Serialize)]
pub struct ApiDv01Contributions {
    /// Contributions by holding
    pub by_holding: Vec<ApiHoldingContribution>,
    /// Contributions by sector
    pub by_sector: Vec<ApiBucketContribution>,
    /// Contributions by rating
    pub by_rating: Vec<ApiBucketContribution>,
    /// Total portfolio DV01
    pub total_dv01: String,
    /// Total portfolio market value
    pub total_market_value: String,
}

/// Spread contributions response.
#[derive(Debug, Serialize)]
pub struct ApiSpreadContributions {
    /// Explicit supplied measure used for every member.
    pub measure: SpreadMeasure,
    /// Contributions by holding
    pub by_holding: Vec<ApiHoldingContribution>,
    /// Contributions by sector
    pub by_sector: Vec<ApiBucketContribution>,
    /// Contributions by rating
    pub by_rating: Vec<ApiBucketContribution>,
    /// Portfolio weighted average spread
    pub portfolio_spread: f64,
    /// Total portfolio market value
    pub total_market_value: String,
}

/// CS01 contributions response.
#[derive(Debug, Serialize)]
pub struct ApiCs01Contributions {
    /// Contributions by holding
    pub by_holding: Vec<ApiHoldingContribution>,
    /// Contributions by sector
    pub by_sector: Vec<ApiBucketContribution>,
    /// Contributions by rating
    pub by_rating: Vec<ApiBucketContribution>,
    /// Total portfolio CS01
    pub total_cs01: String,
    /// Total portfolio market value
    pub total_market_value: String,
}

/// Combined risk contributions response.
#[derive(Debug, Serialize)]
pub struct RiskContributionsResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Duration contributions (if requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<ApiDurationContributions>,
    /// DV01 contributions (if requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dv01: Option<ApiDv01Contributions>,
    /// Spread contributions (if requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spread: Option<ApiSpreadContributions>,
    /// CS01 contributions (if requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cs01: Option<ApiCs01Contributions>,
    /// Number of holdings processed
    pub num_holdings: usize,
    /// Timestamp
    pub timestamp: i64,
}

/// Calculate risk contributions for a portfolio.
///
/// Returns duration, DV01, spread, and CS01 contributions broken down
/// by holding, sector, and rating.
pub async fn calculate_risk_contributions(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<RiskContributionsRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        // Convert to holdings
        let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
            Ok(h) => h,
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e })),
                ));
            }
        };

        if holdings.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(
                    serde_json::json!({ "error": "No valid holdings found (positions must have matching bond prices)" }),
                ),
            ));
        }

        let config = AnalyticsConfig::default();
        let types: std::collections::HashSet<_> = request
            .contribution_types
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        portfolio_result(
            (request.contribution_types.len() <= 4
                && types.len() == request.contribution_types.len()
                && types
                    .iter()
                    .all(|s| matches!(s.as_str(), "duration" | "dv01" | "spread" | "cs01")))
            .then_some(())
            .ok_or("bounded unique supported contribution types required"),
        )?;

        // Calculate requested contribution types
        let duration = if types.contains("duration") || types.is_empty() {
            Some(convert_duration_contributions(&portfolio_result(
                duration_contributions(
                    &holdings,
                    &config,
                    portfolio_result(
                        request
                            .duration_measure
                            .ok_or("duration_measure is required"),
                    )?,
                ),
            )?))
        } else {
            None
        };

        let dv01 = if types.contains("dv01") || types.is_empty() {
            Some(convert_dv01_contributions(&portfolio_result(
                dv01_contributions(&holdings, &config),
            )?))
        } else {
            None
        };

        let spread = if types.contains("spread") || types.is_empty() {
            Some(convert_spread_contributions(&portfolio_result(
                spread_contributions(
                    &holdings,
                    &config,
                    portfolio_result(request.spread_measure.ok_or("spread_measure is required"))?,
                ),
            )?))
        } else {
            None
        };

        let cs01 = if types.contains("cs01") || types.is_empty() {
            Some(convert_cs01_contributions(&portfolio_result(
                cs01_contributions(&holdings, &config),
            )?))
        } else {
            None
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let response = RiskContributionsResponse {
            portfolio_id: request.portfolio.portfolio_id.clone(),
            duration,
            dv01,
            spread,
            cs01,
            num_holdings: holdings.len(),
            timestamp: now,
        };

        (
            StatusCode::OK,
            Json(serde_json::to_value(response).unwrap()),
        )
    })
}

/// Convert the complete supplied quote cohort without price/identifier/FX fallbacks.
fn convert_to_holdings(
    portfolio: &PortfolioInput,
    bond_prices: &[BondQuoteOutput],
) -> Result<Vec<Holding>, String> {
    use convex_math::decimal::BoundedDecimal;
    use std::collections::{HashMap, HashSet};
    let checked = convert_portfolio_input(portfolio)?;
    if bond_prices.len() > 65_536 {
        return Err("supplied quote bound exceeded".into());
    }
    let wanted = portfolio
        .positions
        .iter()
        .map(|p| p.instrument_id.as_str())
        .collect::<HashSet<_>>();
    let mut prices = HashMap::new();
    for quote in bond_prices {
        if wanted.contains(quote.instrument_id.as_str())
            && prices.insert(quote.instrument_id.as_str(), quote).is_some()
        {
            return Err(format!("duplicate quote for {}", quote.instrument_id));
        }
    }
    let mut date = None;
    let mut holdings = Vec::with_capacity(portfolio.positions.len());
    for position in &portfolio.positions {
        let id = &position.instrument_id;
        let quote = *prices
            .get(id.as_str())
            .ok_or_else(|| format!("missing quote for {id}"))?;
        if quote.currency != checked.currency {
            return Err(format!("{id}: foreign quote requires explicit dated FX"));
        }
        if date.is_some_and(|d| d != quote.settlement_date) {
            return Err(format!("{id}: mixed quote settlement dates"));
        }
        date = Some(quote.settlement_date);
        if quote.is_stale
            || quote.timestamp < 0
            || quote.quality > 100
            || quote.source.trim().is_empty()
            || quote.source.len() > 8192
            || quote.pricing_spec.trim().is_empty()
            || quote.pricing_spec.len() > 8192
        {
            return Err(format!(
                "{id}: valid non-stale supplied quote lineage required"
            ));
        }
        let clean = quote
            .clean_price_mid
            .ok_or_else(|| format!("{id}: actual clean mid price required"))?;
        let accrued = quote
            .accrued_interest
            .ok_or_else(|| format!("{id}: actual accrued per100 required"))?;
        if clean < Decimal::ZERO {
            return Err(format!("{id}: negative clean price"));
        }
        let mut dirty = BoundedDecimal::decimal(clean);
        dirty
            .add(&BoundedDecimal::decimal(accrued))
            .map_err(|e| e.to_string())?;
        if dirty.is_negative() {
            return Err(format!("{id}: negative dirty price"));
        }
        let convert = |value: Option<Decimal>| -> Result<Option<f64>, String> {
            value
                .map(|v| {
                    v.to_f64()
                        .filter(|f| f.is_finite() && (v.is_zero() || *f != 0.0))
                        .ok_or_else(|| {
                            format!("{id}: finite non-erased binary64 diagnostic required")
                        })
                })
                .transpose()
        };
        let mut analytics = HoldingAnalytics::new();
        for (slot, value) in [
            (&mut analytics.modified_duration, quote.modified_duration),
            (&mut analytics.effective_duration, quote.effective_duration),
            (&mut analytics.macaulay_duration, quote.macaulay_duration),
            (&mut analytics.spread_duration, quote.spread_duration),
            (&mut analytics.convexity, quote.convexity),
            (
                &mut analytics.effective_convexity,
                quote.effective_convexity,
            ),
            (&mut analytics.dv01, quote.dv01),
            (&mut analytics.cs01, quote.cs01),
            (&mut analytics.ytm, quote.ytm_mid),
            (&mut analytics.ytw, quote.ytw),
            (&mut analytics.ytc, quote.ytc),
            (&mut analytics.z_spread, quote.z_spread_mid),
            (&mut analytics.oas, quote.oas_mid),
            (&mut analytics.g_spread, quote.g_spread_mid),
            (&mut analytics.i_spread, quote.i_spread_mid),
            (&mut analytics.asw, quote.asw_mid),
        ] {
            *slot = convert(value)?;
        }
        if let Some(points) = &quote.key_rate_durations {
            if points.len() > 1024 {
                return Err(format!("{id}: key-rate factor bound exceeded"));
            }
            let mut rows = Vec::with_capacity(points.len());
            let mut seen = HashSet::new();
            for (label, value) in points {
                let factor = convex_core::contracts::FactorTenor::new(label.clone())
                    .map_err(|e| format!("{id}: {e}"))?;
                let text = factor.as_str();
                let split = text.len() - 1;
                let number = text[..split]
                    .parse::<u32>()
                    .map_err(|_| format!("{id}: explicit positive M/Y factors required"))?;
                let years=match text.as_bytes()[split] {b'Y'=>f64::from(number),b'M'=>f64::from(number)/12.0,_=>return Err(format!("{id}: dated day/week factor mapping is unavailable on this diagnostic route"))};
                if !seen.insert(years.to_bits()) {
                    return Err(format!("{id}: duplicate or aliased factor mapping"));
                }
                rows.push(convex_analytics::risk::KeyRateDuration {
                    tenor: years,
                    duration: convex_analytics::risk::Duration::new(*value),
                });
            }
            analytics.key_rate_durations =
                Some(convex_analytics::risk::KeyRateDurations::new(rows));
        }
        let mut classification = convex_portfolio::Classification::new();
        if let Some(value) = &position.sector {
            classification =
                classification.with_sector(convex_portfolio::SectorInfo::from_composite(
                    parse_sector(value)
                        .ok_or_else(|| format!("{id}: unknown supplied sector {value}"))?,
                ));
        }
        if let Some(value) = &position.rating {
            classification =
                classification.with_rating(convex_portfolio::RatingInfo::from_composite(
                    parse_rating(value)
                        .ok_or_else(|| format!("{id}: unknown supplied rating {value}"))?,
                ));
        }
        if let Some(value) = &position.country {
            classification = classification.with_country(value);
        }
        if let Some(value) = &position.issuer {
            classification = classification.with_issuer(value);
        }
        let identifiers = match &quote.isin {
            Some(value) => BondIdentifiers::from_isin_str(value)
                .map_err(|e| format!("{id}: invalid supplied ISIN: {e}"))?,
            None => BondIdentifiers::new(),
        };
        let holding = HoldingBuilder::new()
            .id(id)
            .identifiers(identifiers)
            .par_amount(position.notional)
            .market_price(clean)
            .accrued_interest(accrued)
            .currency(quote.currency)
            .analytics(analytics)
            .classification(classification)
            .build()
            .map_err(|e| e.to_string())?;
        holdings.push(holding);
    }
    if holdings.len() != portfolio.positions.len() {
        return Err("quote/position cardinality mismatch".into());
    }
    Ok(holdings)
}

/// Parse sector string to Sector enum.
fn parse_sector(s: &str) -> Option<Sector> {
    match s.to_lowercase().as_str() {
        "government" | "govt" | "sovereign" => Some(Sector::Government),
        "agency" | "agencies" | "gse" => Some(Sector::Agency),
        "corporate" | "corp" => Some(Sector::Corporate),
        "financial" | "financials" | "banking" => Some(Sector::Financial),
        "utility" | "utilities" => Some(Sector::Utility),
        "municipal" | "muni" => Some(Sector::Municipal),
        "supranational" | "supra" => Some(Sector::Supranational),
        "abs" | "asset-backed" | "assetbacked" => Some(Sector::AssetBacked),
        "mbs" | "mortgage-backed" | "mortgagebacked" | "rmbs" => Some(Sector::MortgageBacked),
        "covered" | "covered-bond" | "coveredbond" => Some(Sector::CoveredBond),
        "other" => Some(Sector::Other),
        _ => None,
    }
}

/// Parse rating string to CreditRating enum.
fn parse_rating(s: &str) -> Option<convex_portfolio::CreditRating> {
    use convex_portfolio::CreditRating;
    match s.to_uppercase().as_str() {
        "AAA" => Some(CreditRating::AAA),
        "AA+" => Some(CreditRating::AAPlus),
        "AA" => Some(CreditRating::AA),
        "AA-" => Some(CreditRating::AAMinus),
        "A+" => Some(CreditRating::APlus),
        "A" => Some(CreditRating::A),
        "A-" => Some(CreditRating::AMinus),
        "BBB+" => Some(CreditRating::BBBPlus),
        "BBB" => Some(CreditRating::BBB),
        "BBB-" => Some(CreditRating::BBBMinus),
        "BB+" => Some(CreditRating::BBPlus),
        "BB" => Some(CreditRating::BB),
        "BB-" => Some(CreditRating::BBMinus),
        "B+" => Some(CreditRating::BPlus),
        "B" => Some(CreditRating::B),
        "B-" => Some(CreditRating::BMinus),
        "CCC+" => Some(CreditRating::CCCPlus),
        "CCC" => Some(CreditRating::CCC),
        "CCC-" => Some(CreditRating::CCCMinus),
        "CC" => Some(CreditRating::CC),
        "C" => Some(CreditRating::C),
        "D" => Some(CreditRating::D),
        "NR" | "NOT RATED" => Some(CreditRating::NotRated),
        _ => None,
    }
}

/// Convert HoldingContribution to API type.
fn convert_holding_contribution(c: &HoldingContribution) -> ApiHoldingContribution {
    ApiHoldingContribution {
        id: c.id.clone(),
        weight: c.weight,
        contribution: c.contribution,
        contribution_pct: c.contribution_pct,
    }
}

/// Convert sector bucket contributions to API type.
fn convert_sector_buckets(
    buckets: &std::collections::HashMap<Sector, BucketContribution>,
) -> Vec<ApiBucketContribution> {
    buckets
        .iter()
        .map(|(sector, b)| ApiBucketContribution {
            name: format!("{:?}", sector),
            count: b.count,
            weight: b.weight,
            contribution: b.contribution,
            contribution_pct: b.contribution_pct,
        })
        .collect()
}

/// Convert rating bucket contributions to API type.
fn convert_rating_buckets(
    buckets: &std::collections::HashMap<RatingBucket, BucketContribution>,
) -> Vec<ApiBucketContribution> {
    buckets
        .iter()
        .map(|(rating, b)| ApiBucketContribution {
            name: format!("{:?}", rating),
            count: b.count,
            weight: b.weight,
            contribution: b.contribution,
            contribution_pct: b.contribution_pct,
        })
        .collect()
}

/// Convert DurationContributions to API response type.
fn convert_duration_contributions(c: &DurationContributions) -> ApiDurationContributions {
    ApiDurationContributions {
        measure: c.measure,
        by_holding: c
            .by_holding
            .iter()
            .map(convert_holding_contribution)
            .collect(),
        by_sector: convert_sector_buckets(&c.by_sector),
        by_rating: convert_rating_buckets(&c.by_rating),
        portfolio_duration: c.portfolio_duration,
        total_market_value: c.total_market_value.to_string(),
    }
}

/// Convert Dv01Contributions to API response type.
fn convert_dv01_contributions(c: &Dv01Contributions) -> ApiDv01Contributions {
    ApiDv01Contributions {
        by_holding: c
            .by_holding
            .iter()
            .map(convert_holding_contribution)
            .collect(),
        by_sector: convert_sector_buckets(&c.by_sector),
        by_rating: convert_rating_buckets(&c.by_rating),
        total_dv01: c.total_dv01.to_string(),
        total_market_value: c.total_market_value.to_string(),
    }
}

/// Convert SpreadContributions to API response type.
fn convert_spread_contributions(c: &SpreadContributions) -> ApiSpreadContributions {
    ApiSpreadContributions {
        measure: c.measure,
        by_holding: c
            .by_holding
            .iter()
            .map(convert_holding_contribution)
            .collect(),
        by_sector: convert_sector_buckets(&c.by_sector),
        by_rating: convert_rating_buckets(&c.by_rating),
        portfolio_spread: c.portfolio_spread,
        total_market_value: c.total_market_value.to_string(),
    }
}

/// Convert Cs01Contributions to API response type.
fn convert_cs01_contributions(c: &Cs01Contributions) -> ApiCs01Contributions {
    ApiCs01Contributions {
        by_holding: c
            .by_holding
            .iter()
            .map(convert_holding_contribution)
            .collect(),
        by_sector: convert_sector_buckets(&c.by_sector),
        by_rating: convert_rating_buckets(&c.by_rating),
        total_cs01: c.total_cs01.to_string(),
        total_market_value: c.total_market_value.to_string(),
    }
}

// =============================================================================
// PORTFOLIO BUCKETING (convex-portfolio)
// =============================================================================

/// Request for portfolio bucketing analysis.
#[derive(Debug, Deserialize)]
pub struct BucketingRequest {
    /// Portfolio definition
    pub portfolio: PortfolioInput,
    /// Bond prices with analytics
    pub bond_prices: Vec<BondQuoteOutput>,
}

/// API bucket metrics.
#[derive(Debug, Serialize)]
pub struct ApiBucketMetrics {
    /// Number of holdings in this bucket
    pub count: usize,
    /// Total market value
    pub market_value: String,
    /// Weight as percentage of total (0-100)
    pub weight_pct: f64,
    /// Par value
    pub par_value: String,
    /// Weighted average YTM
    pub avg_ytm: Option<f64>,
    /// Weighted average duration
    pub avg_duration: Option<f64>,
    /// Total DV01 for this bucket
    pub total_dv01: Option<String>,
    /// Weighted average spread
    pub avg_spread: Option<f64>,
}

/// Sector bucketing response.
#[derive(Debug, Serialize)]
pub struct SectorBucketingResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Buckets by sector
    pub by_sector: Vec<SectorBucketEntry>,
    /// Holdings without sector classification
    pub unclassified: ApiBucketMetrics,
    /// Total portfolio market value
    pub total_market_value: String,
    /// Summary weights
    pub summary: SectorSummary,
    /// Number of holdings processed
    pub num_holdings: usize,
    /// Timestamp
    pub timestamp: i64,
}

/// Single sector bucket entry.
#[derive(Debug, Serialize)]
pub struct SectorBucketEntry {
    /// Sector name
    pub sector: String,
    /// Bucket metrics
    #[serde(flatten)]
    pub metrics: ApiBucketMetrics,
}

/// Sector summary weights.
#[derive(Debug, Serialize)]
pub struct SectorSummary {
    /// Weight of government-related sectors
    pub government_weight: f64,
    /// Weight of credit sectors
    pub credit_weight: f64,
    /// Weight of securitized sectors
    pub securitized_weight: f64,
}

/// Calculate sector bucketing for a portfolio.
pub async fn calculate_sector_bucketing(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<BucketingRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        // Convert to holdings
        let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
            Ok(h) => h,
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e })),
                ));
            }
        };

        let config = AnalyticsConfig::default();
        let dist = portfolio_result(bucket_by_sector(&holdings, &config))?;

        let by_sector: Vec<SectorBucketEntry> = dist
            .by_sector
            .iter()
            .map(|(sector, metrics)| SectorBucketEntry {
                sector: format!("{:?}", sector),
                metrics: convert_bucket_metrics(metrics),
            })
            .collect();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let response = SectorBucketingResponse {
            portfolio_id: request.portfolio.portfolio_id.clone(),
            by_sector,
            unclassified: convert_bucket_metrics(&dist.unclassified),
            total_market_value: dist.total_market_value.to_string(),
            summary: SectorSummary {
                government_weight: dist.government_weight(),
                credit_weight: dist.credit_weight(),
                securitized_weight: dist.securitized_weight(),
            },
            num_holdings: holdings.len(),
            timestamp: now,
        };

        (
            StatusCode::OK,
            Json(serde_json::to_value(response).unwrap()),
        )
    })
}

/// Rating bucketing response.
#[derive(Debug, Serialize)]
pub struct RatingBucketingResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Buckets by individual rating notch
    pub by_rating: Vec<RatingBucketEntry>,
    /// Buckets by rating bucket (AAA, AA, A, BBB, etc.)
    pub by_bucket: Vec<RatingBucketEntry>,
    /// Unrated holdings
    pub unrated: ApiBucketMetrics,
    /// Total portfolio market value
    pub total_market_value: String,
    /// Summary metrics
    pub summary: RatingSummary,
    /// Number of holdings processed
    pub num_holdings: usize,
    /// Timestamp
    pub timestamp: i64,
}

/// Single rating bucket entry.
#[derive(Debug, Serialize)]
pub struct RatingBucketEntry {
    /// Rating or bucket name
    pub rating: String,
    /// Bucket metrics
    #[serde(flatten)]
    pub metrics: ApiBucketMetrics,
}

/// Rating summary metrics.
#[derive(Debug, Serialize)]
pub struct RatingSummary {
    /// Weight of investment grade holdings
    pub investment_grade_weight: f64,
    /// Weight of high yield holdings
    pub high_yield_weight: f64,
    /// Weight of defaulted holdings
    pub default_weight: f64,
    /// Weight of unrated holdings
    pub unrated_weight: f64,
    /// Average rating score (1=AAA, 22=D)
    pub average_rating_score: Option<f64>,
    /// Implied average rating
    pub average_rating: Option<String>,
}

/// Calculate rating bucketing for a portfolio.
pub async fn calculate_rating_bucketing(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<BucketingRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        // Convert to holdings
        let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
            Ok(h) => h,
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e })),
                ));
            }
        };

        let config = AnalyticsConfig::default();
        let dist = portfolio_result(bucket_by_rating(&holdings, &config))?;

        let by_rating: Vec<RatingBucketEntry> = dist
            .by_rating
            .iter()
            .map(|(rating, metrics)| RatingBucketEntry {
                rating: format!("{:?}", rating),
                metrics: convert_bucket_metrics(metrics),
            })
            .collect();

        let by_bucket: Vec<RatingBucketEntry> = dist
            .by_bucket
            .iter()
            .map(|(bucket, metrics)| RatingBucketEntry {
                rating: format!("{:?}", bucket),
                metrics: convert_bucket_metrics(metrics),
            })
            .collect();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let response = RatingBucketingResponse {
            portfolio_id: request.portfolio.portfolio_id.clone(),
            by_rating,
            by_bucket,
            unrated: convert_bucket_metrics(&dist.unrated),
            total_market_value: dist.total_market_value.to_string(),
            summary: RatingSummary {
                investment_grade_weight: dist.investment_grade_weight(),
                high_yield_weight: dist.high_yield_weight(),
                default_weight: dist.default_weight(),
                unrated_weight: dist.unrated_weight(),
                average_rating_score: dist.average_rating_score(),
                average_rating: dist.average_rating().map(|r| format!("{:?}", r)),
            },
            num_holdings: holdings.len(),
            timestamp: now,
        };

        (
            StatusCode::OK,
            Json(serde_json::to_value(response).unwrap()),
        )
    })
}

/// Maturity bucketing response.
#[derive(Debug, Serialize)]
pub struct MaturityBucketingResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Buckets by maturity
    pub by_bucket: Vec<MaturityBucketEntry>,
    /// Holdings without maturity information
    pub unknown: ApiBucketMetrics,
    /// Total portfolio market value
    pub total_market_value: String,
    /// Summary metrics
    pub summary: MaturitySummary,
    /// Number of holdings processed
    pub num_holdings: usize,
    /// Timestamp
    pub timestamp: i64,
}

/// Single maturity bucket entry.
#[derive(Debug, Serialize)]
pub struct MaturityBucketEntry {
    /// Maturity bucket name
    pub bucket: String,
    /// Bucket metrics
    #[serde(flatten)]
    pub metrics: ApiBucketMetrics,
}

/// Maturity summary metrics.
#[derive(Debug, Serialize)]
pub struct MaturitySummary {
    /// Weight of short-term holdings (0-3 years)
    pub short_term_weight: f64,
    /// Weight of intermediate holdings (3-10 years)
    pub intermediate_weight: f64,
    /// Weight of long-term holdings (10+ years)
    pub long_term_weight: f64,
    /// Weighted average years to maturity
    pub weighted_avg_maturity: Option<f64>,
}

/// Calculate maturity bucketing for a portfolio.
pub async fn calculate_maturity_bucketing(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<BucketingRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        // Convert to holdings
        let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
            Ok(h) => h,
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e })),
                ));
            }
        };

        let config = AnalyticsConfig::default();
        let dist = portfolio_result(bucket_by_maturity(&holdings, &config))?;

        let by_bucket: Vec<MaturityBucketEntry> = dist
            .by_bucket
            .iter()
            .map(|(bucket, metrics)| MaturityBucketEntry {
                bucket: format!("{:?}", bucket),
                metrics: convert_bucket_metrics(metrics),
            })
            .collect();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let response = MaturityBucketingResponse {
            portfolio_id: request.portfolio.portfolio_id.clone(),
            by_bucket,
            unknown: convert_bucket_metrics(&dist.unknown),
            total_market_value: dist.total_market_value.to_string(),
            summary: MaturitySummary {
                short_term_weight: dist.short_term_weight(),
                intermediate_weight: dist.intermediate_weight(),
                long_term_weight: dist.long_term_weight(),
                weighted_avg_maturity: dist.weighted_avg_maturity,
            },
            num_holdings: holdings.len(),
            timestamp: now,
        };

        (
            StatusCode::OK,
            Json(serde_json::to_value(response).unwrap()),
        )
    })
}

/// Custom bucketing response.
#[derive(Debug, Serialize)]
pub struct CustomBucketingResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Bucketing type (country, issuer, currency, etc.)
    pub bucket_type: String,
    /// Buckets
    pub by_bucket: Vec<CustomBucketEntry>,
    /// Unclassified holdings
    pub unclassified: ApiBucketMetrics,
    /// Total portfolio market value
    pub total_market_value: String,
    /// Number of distinct buckets
    pub bucket_count: usize,
    /// Number of holdings processed
    pub num_holdings: usize,
    /// Timestamp
    pub timestamp: i64,
}

/// Single custom bucket entry.
#[derive(Debug, Serialize)]
pub struct CustomBucketEntry {
    /// Bucket key
    pub key: String,
    /// Bucket metrics
    #[serde(flatten)]
    pub metrics: ApiBucketMetrics,
}

/// Request for custom bucketing.
#[derive(Debug, Deserialize)]
pub struct CustomBucketingRequest {
    /// Portfolio definition
    pub portfolio: PortfolioInput,
    /// Bond prices with analytics
    pub bond_prices: Vec<BondQuoteOutput>,
    /// Bucketing type: "country", "issuer", "currency"
    pub bucket_type: String,
}

/// Calculate custom bucketing (country, issuer, currency) for a portfolio.
pub async fn calculate_custom_bucketing(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<CustomBucketingRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        // Convert to holdings
        let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
            Ok(h) => h,
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e })),
                ));
            }
        };

        let config = AnalyticsConfig::default();

        let dist = match request.bucket_type.to_lowercase().as_str() {
            "country" => portfolio_result(bucket_by_country(&holdings, &config))?,
            "issuer" => portfolio_result(bucket_by_issuer(&holdings, &config))?,
            "currency" => portfolio_result(bucket_by_currency(&holdings, &config))?,
            _ => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("Invalid bucket_type: {}. Valid options: country, issuer, currency", request.bucket_type)
                    })),
                ));
            }
        };

        let by_bucket: Vec<CustomBucketEntry> = dist
            .by_bucket
            .iter()
            .map(|(key, metrics)| CustomBucketEntry {
                key: key.clone(),
                metrics: convert_bucket_metrics(metrics),
            })
            .collect();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let response = CustomBucketingResponse {
            portfolio_id: request.portfolio.portfolio_id.clone(),
            bucket_type: request.bucket_type.clone(),
            by_bucket,
            unclassified: convert_bucket_metrics(&dist.unclassified),
            total_market_value: dist.total_market_value.to_string(),
            bucket_count: dist.bucket_count(),
            num_holdings: holdings.len(),
            timestamp: now,
        };

        (
            StatusCode::OK,
            Json(serde_json::to_value(response).unwrap()),
        )
    })
}

/// Combined bucketing response.
#[derive(Debug, Serialize)]
pub struct CombinedBucketingResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Sector distribution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sector: Option<SectorBucketingResponse>,
    /// Rating distribution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<RatingBucketingResponse>,
    /// Maturity distribution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maturity: Option<MaturityBucketingResponse>,
    /// Number of holdings processed
    pub num_holdings: usize,
    /// Timestamp
    pub timestamp: i64,
}

/// Request for combined bucketing.
#[derive(Debug, Deserialize)]
pub struct CombinedBucketingRequest {
    /// Portfolio definition
    pub portfolio: PortfolioInput,
    /// Bond prices with analytics
    pub bond_prices: Vec<BondQuoteOutput>,
    /// Bucketing types to include (default: all)
    #[serde(default = "default_bucket_types")]
    pub bucket_types: Vec<String>,
}

fn default_bucket_types() -> Vec<String> {
    vec![
        "sector".to_string(),
        "rating".to_string(),
        "maturity".to_string(),
    ]
}

/// Calculate all bucketing types for a portfolio.
pub async fn calculate_all_bucketing(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<CombinedBucketingRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        // Convert to holdings
        let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
            Ok(h) => h,
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e })),
                ));
            }
        };

        let config = AnalyticsConfig::default();
        let types: std::collections::HashSet<_> = request
            .bucket_types
            .iter()
            .map(|s| s.to_lowercase())
            .collect();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // Calculate sector distribution
        let sector = if types.contains("sector") || types.is_empty() {
            let dist = portfolio_result(bucket_by_sector(&holdings, &config))?;
            let by_sector: Vec<SectorBucketEntry> = dist
                .by_sector
                .iter()
                .map(|(sector, metrics)| SectorBucketEntry {
                    sector: format!("{:?}", sector),
                    metrics: convert_bucket_metrics(metrics),
                })
                .collect();

            Some(SectorBucketingResponse {
                portfolio_id: request.portfolio.portfolio_id.clone(),
                by_sector,
                unclassified: convert_bucket_metrics(&dist.unclassified),
                total_market_value: dist.total_market_value.to_string(),
                summary: SectorSummary {
                    government_weight: dist.government_weight(),
                    credit_weight: dist.credit_weight(),
                    securitized_weight: dist.securitized_weight(),
                },
                num_holdings: holdings.len(),
                timestamp: now,
            })
        } else {
            None
        };

        // Calculate rating distribution
        let rating = if types.contains("rating") || types.is_empty() {
            let dist = portfolio_result(bucket_by_rating(&holdings, &config))?;
            let by_rating: Vec<RatingBucketEntry> = dist
                .by_rating
                .iter()
                .map(|(rating, metrics)| RatingBucketEntry {
                    rating: format!("{:?}", rating),
                    metrics: convert_bucket_metrics(metrics),
                })
                .collect();
            let by_bucket: Vec<RatingBucketEntry> = dist
                .by_bucket
                .iter()
                .map(|(bucket, metrics)| RatingBucketEntry {
                    rating: format!("{:?}", bucket),
                    metrics: convert_bucket_metrics(metrics),
                })
                .collect();

            Some(RatingBucketingResponse {
                portfolio_id: request.portfolio.portfolio_id.clone(),
                by_rating,
                by_bucket,
                unrated: convert_bucket_metrics(&dist.unrated),
                total_market_value: dist.total_market_value.to_string(),
                summary: RatingSummary {
                    investment_grade_weight: dist.investment_grade_weight(),
                    high_yield_weight: dist.high_yield_weight(),
                    default_weight: dist.default_weight(),
                    unrated_weight: dist.unrated_weight(),
                    average_rating_score: dist.average_rating_score(),
                    average_rating: dist.average_rating().map(|r| format!("{:?}", r)),
                },
                num_holdings: holdings.len(),
                timestamp: now,
            })
        } else {
            None
        };

        // Calculate maturity distribution
        let maturity = if types.contains("maturity") || types.is_empty() {
            let dist = portfolio_result(bucket_by_maturity(&holdings, &config))?;
            let by_bucket: Vec<MaturityBucketEntry> = dist
                .by_bucket
                .iter()
                .map(|(bucket, metrics)| MaturityBucketEntry {
                    bucket: format!("{:?}", bucket),
                    metrics: convert_bucket_metrics(metrics),
                })
                .collect();

            Some(MaturityBucketingResponse {
                portfolio_id: request.portfolio.portfolio_id.clone(),
                by_bucket,
                unknown: convert_bucket_metrics(&dist.unknown),
                total_market_value: dist.total_market_value.to_string(),
                summary: MaturitySummary {
                    short_term_weight: dist.short_term_weight(),
                    intermediate_weight: dist.intermediate_weight(),
                    long_term_weight: dist.long_term_weight(),
                    weighted_avg_maturity: dist.weighted_avg_maturity,
                },
                num_holdings: holdings.len(),
                timestamp: now,
            })
        } else {
            None
        };

        let response = CombinedBucketingResponse {
            portfolio_id: request.portfolio.portfolio_id.clone(),
            sector,
            rating,
            maturity,
            num_holdings: holdings.len(),
            timestamp: now,
        };

        (
            StatusCode::OK,
            Json(serde_json::to_value(response).unwrap()),
        )
    })
}

/// Convert BucketMetrics to API type.
fn convert_bucket_metrics(m: &BucketMetrics) -> ApiBucketMetrics {
    ApiBucketMetrics {
        count: m.count,
        market_value: m.market_value.to_string(),
        weight_pct: m.weight_pct,
        par_value: m.par_value.to_string(),
        avg_ytm: m.avg_ytm,
        avg_duration: m.avg_duration,
        total_dv01: m.total_dv01.map(|d| d.to_string()),
        avg_spread: m.avg_spread,
    }
}

/// Convert API portfolio input to internal Portfolio type.
fn convert_portfolio_input(input: &PortfolioInput) -> Result<Portfolio, String> {
    use std::collections::HashSet;
    let currency = Currency::from_code(&input.currency)
        .ok_or_else(|| format!("unknown reporting currency {}", input.currency))?;
    if input.portfolio_id.trim().is_empty()
        || input.portfolio_id.len() > 8192
        || input.positions.len() > 65_536
    {
        return Err("bounded nonempty portfolio identity and position manifest required".into());
    }
    let mut ids = HashSet::new();
    for p in &input.positions {
        if p.instrument_id.trim().is_empty()
            || p.instrument_id.len() > 8192
            || !ids.insert(p.instrument_id.as_str())
        {
            return Err("unique nonempty bounded instrument positions required".into());
        }
        if let Some(value) = &p.currency {
            let position_currency = Currency::from_code(value)
                .ok_or_else(|| format!("{}: invalid position currency", p.instrument_id))?;
            if position_currency != currency {
                return Err(format!(
                    "{}: foreign position requires explicit dated FX",
                    p.instrument_id
                ));
            }
        }
        for text in [&p.sector, &p.rating, &p.country, &p.issuer]
            .into_iter()
            .flatten()
        {
            if text.trim().is_empty() || text.len() > 8192 {
                return Err(format!(
                    "{}: bounded nonempty supplied classifications required",
                    p.instrument_id
                ));
            }
        }
    }
    Ok(Portfolio {
        portfolio_id: PortfolioId::new(&input.portfolio_id),
        name: input.name.clone(),
        currency,
        positions: input
            .positions
            .iter()
            .map(|p| Position {
                instrument_id: InstrumentId::new(&p.instrument_id),
                notional: p.notional,
                sector: p.sector.clone(),
                rating: p.rating.clone(),
            })
            .collect(),
    })
}

fn default_limit() -> usize {
    100
}

// =============================================================================
// PORTFOLIO CRUD
// =============================================================================

/// Query parameters for portfolio listing.
#[derive(Debug, Deserialize)]
pub struct PortfolioListQuery {
    /// Maximum number of portfolios to return
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination
    #[serde(default)]
    pub offset: usize,
    /// Currency filter
    pub currency: Option<String>,
    /// Text search query (searches name, portfolio_id, description)
    pub q: Option<String>,
}

/// Response for portfolio listing.
#[derive(Debug, Serialize)]
pub struct PortfolioListResponse {
    /// Portfolios
    pub portfolios: Vec<StoredPortfolio>,
    /// Total count (before pagination)
    pub total: usize,
    /// Limit used
    pub limit: usize,
    /// Offset used
    pub offset: usize,
}

/// List portfolios with optional filtering and pagination.
pub async fn list_portfolios(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<PortfolioListQuery>,
) -> impl IntoResponse {
    // Build filter from query params
    let filter = PortfolioFilter {
        currency: query.currency,
        text_search: query.q,
    };

    // Get total count
    let total = state.portfolio_store.count(&filter);

    // Get portfolios with pagination
    let portfolios = state
        .portfolio_store
        .list(&filter, query.limit, query.offset);

    let response = PortfolioListResponse {
        portfolios,
        total,
        limit: query.limit,
        offset: query.offset,
    };

    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap()),
    )
}

/// Get a single portfolio by ID.
pub async fn get_portfolio(
    State(state): State<Arc<AppState>>,
    Path(portfolio_id): Path<String>,
) -> impl IntoResponse {
    match state.portfolio_store.get(&portfolio_id) {
        Some(portfolio) => (
            StatusCode::OK,
            Json(serde_json::to_value(portfolio).unwrap()),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Portfolio not found: {}", portfolio_id) })),
        ),
    }
}

/// Request for creating a portfolio.
#[derive(Debug, Deserialize)]
pub struct CreatePortfolioRequest {
    /// Portfolio identifier
    pub portfolio_id: String,
    /// Portfolio name
    pub name: String,
    /// Reporting currency (USD, EUR, GBP, etc.)
    #[serde(default = "default_currency")]
    pub currency: String,
    /// Description
    pub description: Option<String>,
    /// Initial positions
    #[serde(default)]
    pub positions: Vec<StoredPosition>,
}

/// Create a new portfolio.
pub async fn create_portfolio(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreatePortfolioRequest>,
) -> impl IntoResponse {
    // Check if portfolio already exists
    if state.portfolio_store.get(&request.portfolio_id).is_some() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("Portfolio already exists: {}", request.portfolio_id)
            })),
        );
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let portfolio = StoredPortfolio {
        portfolio_id: request.portfolio_id,
        name: request.name,
        currency: request.currency,
        description: request.description,
        positions: request.positions,
        created_at: now,
        updated_at: now,
    };

    let created = state.portfolio_store.upsert(portfolio);

    (
        StatusCode::CREATED,
        Json(serde_json::to_value(created).unwrap()),
    )
}

/// Request for updating a portfolio.
#[derive(Debug, Deserialize)]
pub struct UpdatePortfolioRequest {
    /// Portfolio name
    pub name: Option<String>,
    /// Reporting currency
    pub currency: Option<String>,
    /// Description
    pub description: Option<String>,
    /// Positions (if provided, replaces all positions)
    pub positions: Option<Vec<StoredPosition>>,
}

/// Update an existing portfolio.
pub async fn update_portfolio(
    State(state): State<Arc<AppState>>,
    Path(portfolio_id): Path<String>,
    Json(request): Json<UpdatePortfolioRequest>,
) -> impl IntoResponse {
    // Get existing portfolio
    let existing = match state.portfolio_store.get(&portfolio_id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(
                    serde_json::json!({ "error": format!("Portfolio not found: {}", portfolio_id) }),
                ),
            );
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    // Update fields
    let portfolio = StoredPortfolio {
        portfolio_id: existing.portfolio_id,
        name: request.name.unwrap_or(existing.name),
        currency: request.currency.unwrap_or(existing.currency),
        description: request.description.or(existing.description),
        positions: request.positions.unwrap_or(existing.positions),
        created_at: existing.created_at,
        updated_at: now,
    };

    let updated = state.portfolio_store.upsert(portfolio);

    (StatusCode::OK, Json(serde_json::to_value(updated).unwrap()))
}

/// Delete a portfolio.
pub async fn delete_portfolio(
    State(state): State<Arc<AppState>>,
    Path(portfolio_id): Path<String>,
) -> impl IntoResponse {
    match state.portfolio_store.delete(&portfolio_id) {
        Some(_) => (StatusCode::NO_CONTENT, Json(serde_json::json!({}))),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Portfolio not found: {}", portfolio_id) })),
        ),
    }
}

/// Add a position to a portfolio.
pub async fn add_portfolio_position(
    State(state): State<Arc<AppState>>,
    Path(portfolio_id): Path<String>,
    Json(position): Json<StoredPosition>,
) -> impl IntoResponse {
    match state.portfolio_store.add_position(&portfolio_id, position) {
        Some(portfolio) => (
            StatusCode::OK,
            Json(serde_json::to_value(portfolio).unwrap()),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Portfolio not found: {}", portfolio_id) })),
        ),
    }
}

/// Remove a position from a portfolio.
pub async fn remove_portfolio_position(
    State(state): State<Arc<AppState>>,
    Path((portfolio_id, instrument_id)): Path<(String, String)>,
) -> impl IntoResponse {
    match state
        .portfolio_store
        .remove_position(&portfolio_id, &instrument_id)
    {
        Some(portfolio) => (
            StatusCode::OK,
            Json(serde_json::to_value(portfolio).unwrap()),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Portfolio not found: {}", portfolio_id) })),
        ),
    }
}

/// Update a position in a portfolio.
pub async fn update_portfolio_position(
    State(state): State<Arc<AppState>>,
    Path((portfolio_id, instrument_id)): Path<(String, String)>,
    Json(mut position): Json<StoredPosition>,
) -> impl IntoResponse {
    // Ensure instrument_id matches path
    position.instrument_id = instrument_id.clone();

    match state
        .portfolio_store
        .update_position(&portfolio_id, position)
    {
        Some(portfolio) => (
            StatusCode::OK,
            Json(serde_json::to_value(portfolio).unwrap()),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!("Portfolio or position not found: {}/{}", portfolio_id, instrument_id)
            })),
        ),
    }
}

/// Batch create portfolios.
#[derive(Debug, Deserialize)]
pub struct BatchPortfolioCreateRequest {
    /// Portfolios to create
    pub portfolios: Vec<CreatePortfolioRequest>,
}

/// Batch create response.
#[derive(Debug, Serialize)]
pub struct BatchPortfolioCreateResponse {
    /// Number of portfolios created
    pub created: usize,
    /// Number of portfolios that already existed (skipped)
    pub skipped: usize,
}

/// Batch create portfolios.
pub async fn batch_create_portfolios(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BatchPortfolioCreateRequest>,
) -> impl IntoResponse {
    let mut created = 0;
    let mut skipped = 0;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    for req in request.portfolios {
        // Check if portfolio already exists
        if state.portfolio_store.get(&req.portfolio_id).is_some() {
            skipped += 1;
            continue;
        }

        let portfolio = StoredPortfolio {
            portfolio_id: req.portfolio_id,
            name: req.name,
            currency: req.currency,
            description: req.description,
            positions: req.positions,
            created_at: now,
            updated_at: now,
        };

        state.portfolio_store.upsert(portfolio);
        created += 1;
    }

    let response = BatchPortfolioCreateResponse { created, skipped };

    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap()),
    )
}

// =============================================================================
// BENCHMARK COMPARISON
// =============================================================================

/// Request for benchmark comparison.
#[derive(Debug, Deserialize)]
pub struct BenchmarkComparisonRequest {
    /// Portfolio definition
    pub portfolio: PortfolioInput,
    /// Portfolio bond prices
    pub portfolio_prices: Vec<BondQuoteOutput>,
    /// Benchmark definition
    pub benchmark: PortfolioInput,
    /// Benchmark bond prices
    pub benchmark_prices: Vec<BondQuoteOutput>,
}

/// Response for benchmark comparison.
#[derive(Debug, Serialize)]
pub struct BenchmarkComparisonResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Benchmark ID
    pub benchmark_id: String,
    /// Portfolio holdings count
    pub portfolio_holdings: usize,
    /// Benchmark holdings count
    pub benchmark_holdings: usize,
    /// Duration comparison
    pub duration: DurationComparisonOutput,
    /// Spread comparison
    pub spread: SpreadComparisonOutput,
    /// Yield comparison
    pub yield_comparison: YieldComparisonOutput,
    /// Risk comparison
    pub risk: RiskComparisonOutput,
    /// Active weights summary
    pub active_weights: ActiveWeightsOutput,
    /// Sector-level comparison
    pub by_sector: Vec<SectorComparisonOutput>,
    /// Rating-level comparison
    pub by_rating: Vec<RatingComparisonOutput>,
    /// Timestamp
    pub timestamp: i64,
}

/// Duration comparison output.
#[derive(Debug, Serialize)]
pub struct DurationComparisonOutput {
    /// Portfolio duration
    pub portfolio: Option<f64>,
    /// Benchmark duration
    pub benchmark: Option<f64>,
    /// Difference (portfolio - benchmark)
    pub difference: Option<f64>,
    /// Ratio (portfolio / benchmark)
    pub ratio: Option<f64>,
    /// Is portfolio longer duration?
    pub is_longer: bool,
}

/// Spread comparison output.
#[derive(Debug, Serialize)]
pub struct SpreadComparisonOutput {
    /// Portfolio spread (bps)
    pub portfolio: Option<f64>,
    /// Benchmark spread (bps)
    pub benchmark: Option<f64>,
    /// Difference (bps)
    pub difference: Option<f64>,
    /// Ratio
    pub ratio: Option<f64>,
    /// Is portfolio wider spread?
    pub is_wider: bool,
}

/// Yield comparison output.
#[derive(Debug, Serialize)]
pub struct YieldComparisonOutput {
    /// Portfolio YTM (%)
    pub portfolio_ytm: Option<f64>,
    /// Benchmark YTM (%)
    pub benchmark_ytm: Option<f64>,
    /// YTM difference (%)
    pub ytm_difference: Option<f64>,
    /// Is portfolio higher yield?
    pub is_higher_yield: bool,
}

/// Risk comparison output.
#[derive(Debug, Serialize)]
pub struct RiskComparisonOutput {
    /// Portfolio DV01
    pub portfolio_dv01: Option<String>,
    /// Benchmark DV01
    pub benchmark_dv01: Option<String>,
    /// DV01 difference
    pub dv01_difference: Option<String>,
    /// DV01 ratio
    pub dv01_ratio: Option<f64>,
    /// Portfolio convexity
    pub portfolio_convexity: Option<f64>,
    /// Benchmark convexity
    pub benchmark_convexity: Option<f64>,
}

/// Active weights output.
#[derive(Debug, Serialize)]
pub struct ActiveWeightsOutput {
    /// Total active weight (sum of absolute)
    pub total_active_weight: f64,
    /// Number of overweight positions
    pub overweight_count: usize,
    /// Number of underweight positions
    pub underweight_count: usize,
    /// Overweight sectors
    pub overweight_sectors: Vec<SectorActiveWeight>,
    /// Underweight sectors
    pub underweight_sectors: Vec<SectorActiveWeight>,
    /// Largest active positions
    pub largest_positions: Vec<PositionActiveWeight>,
}

/// Sector active weight.
#[derive(Debug, Serialize)]
pub struct SectorActiveWeight {
    /// Sector name
    pub sector: String,
    /// Active weight (%)
    pub active_weight: f64,
}

/// Position active weight.
#[derive(Debug, Serialize)]
pub struct PositionActiveWeight {
    /// Holding ID
    pub id: String,
    /// Active weight (%)
    pub active_weight: f64,
}

/// Sector comparison output.
#[derive(Debug, Serialize)]
pub struct SectorComparisonOutput {
    /// Sector name
    pub sector: String,
    /// Portfolio weight (%)
    pub portfolio_weight: f64,
    /// Benchmark weight (%)
    pub benchmark_weight: f64,
    /// Active weight (%)
    pub active_weight: f64,
    /// Portfolio duration
    pub portfolio_duration: Option<f64>,
    /// Benchmark duration
    pub benchmark_duration: Option<f64>,
    /// Portfolio spread (bps)
    pub portfolio_spread: Option<f64>,
    /// Benchmark spread (bps)
    pub benchmark_spread: Option<f64>,
}

/// Rating comparison output.
#[derive(Debug, Serialize)]
pub struct RatingComparisonOutput {
    /// Rating bucket
    pub rating: String,
    /// Portfolio weight (%)
    pub portfolio_weight: f64,
    /// Benchmark weight (%)
    pub benchmark_weight: f64,
    /// Active weight (%)
    pub active_weight: f64,
    /// Portfolio duration
    pub portfolio_duration: Option<f64>,
    /// Benchmark duration
    pub benchmark_duration: Option<f64>,
}

/// Perform comprehensive benchmark comparison.
pub async fn compare_to_benchmark(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<BenchmarkComparisonRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        // Convert portfolio holdings
        let port_holdings = match convert_to_holdings(&request.portfolio, &request.portfolio_prices)
        {
            Ok(h) => h,
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Portfolio error: {}", e) })),
                ));
            }
        };

        // Convert benchmark holdings
        let bench_holdings =
            match convert_to_holdings(&request.benchmark, &request.benchmark_prices) {
                Ok(h) => h,
                Err(e) => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": format!("Benchmark error: {}", e) })),
                    ));
                }
            };

        let config = AnalyticsConfig::default();

        // Perform comparison
        let comparison = portfolio_result(benchmark_comparison(
            &port_holdings,
            &bench_holdings,
            &config,
        ))?;
        let weights = &comparison.active_weights;

        // Convert duration comparison
        let duration = DurationComparisonOutput {
            portfolio: comparison.duration.portfolio_duration,
            benchmark: comparison.duration.benchmark_duration,
            difference: comparison.duration.difference,
            ratio: comparison.duration.ratio,
            is_longer: comparison.duration.is_longer(),
        };

        // Convert spread comparison
        let spread = SpreadComparisonOutput {
            portfolio: comparison.spread.portfolio_spread,
            benchmark: comparison.spread.benchmark_spread,
            difference: comparison.spread.difference,
            ratio: comparison.spread.ratio,
            is_wider: comparison.spread.is_wider(),
        };

        // Convert yield comparison
        let yield_comparison = YieldComparisonOutput {
            portfolio_ytm: comparison.yield_comparison.portfolio_ytm.map(|y| y * 100.0),
            benchmark_ytm: comparison.yield_comparison.benchmark_ytm.map(|y| y * 100.0),
            ytm_difference: comparison
                .yield_comparison
                .ytm_difference
                .map(|y| y * 100.0),
            is_higher_yield: comparison.yield_comparison.is_higher_yield(),
        };

        // Convert risk comparison
        let risk = RiskComparisonOutput {
            portfolio_dv01: comparison.risk.portfolio_dv01.map(|v| v.to_string()),
            benchmark_dv01: comparison.risk.benchmark_dv01.map(|v| v.to_string()),
            dv01_difference: comparison.risk.dv01_difference.map(|v| v.to_string()),
            dv01_ratio: comparison.risk.dv01_ratio,
            portfolio_convexity: comparison.risk.portfolio_convexity,
            benchmark_convexity: comparison.risk.benchmark_convexity,
        };

        // Convert active weights
        let overweight_sectors: Vec<SectorActiveWeight> = weights
            .overweight_sectors()
            .into_iter()
            .map(|(s, w)| SectorActiveWeight {
                sector: format!("{:?}", s),
                active_weight: w,
            })
            .collect();

        let underweight_sectors: Vec<SectorActiveWeight> = weights
            .underweight_sectors()
            .into_iter()
            .map(|(s, w)| SectorActiveWeight {
                sector: format!("{:?}", s),
                active_weight: w,
            })
            .collect();

        let largest_positions: Vec<PositionActiveWeight> = weights
            .largest_active_positions(10)
            .into_iter()
            .map(|(id, w)| PositionActiveWeight {
                id: id.to_string(),
                active_weight: w,
            })
            .collect();

        let active_weights_output = ActiveWeightsOutput {
            total_active_weight: weights.total_active_weight,
            overweight_count: weights.overweight_count,
            underweight_count: weights.underweight_count,
            overweight_sectors,
            underweight_sectors,
            largest_positions,
        };

        // Convert sector comparisons
        let by_sector: Vec<SectorComparisonOutput> = comparison
            .by_sector
            .into_iter()
            .map(|(sector, comp)| SectorComparisonOutput {
                sector: format!("{:?}", sector),
                portfolio_weight: comp.portfolio_weight,
                benchmark_weight: comp.benchmark_weight,
                active_weight: comp.active_weight,
                portfolio_duration: comp.portfolio_duration,
                benchmark_duration: comp.benchmark_duration,
                portfolio_spread: comp.portfolio_spread,
                benchmark_spread: comp.benchmark_spread,
            })
            .collect();

        // Convert rating comparisons
        let by_rating: Vec<RatingComparisonOutput> = comparison
            .by_rating
            .into_iter()
            .map(|(rating, comp)| RatingComparisonOutput {
                rating: format!("{:?}", rating),
                portfolio_weight: comp.portfolio_weight,
                benchmark_weight: comp.benchmark_weight,
                active_weight: comp.active_weight,
                portfolio_duration: comp.portfolio_duration,
                benchmark_duration: comp.benchmark_duration,
            })
            .collect();

        let response = BenchmarkComparisonResponse {
            portfolio_id: request.portfolio.portfolio_id.clone(),
            benchmark_id: request.benchmark.portfolio_id.clone(),
            portfolio_holdings: port_holdings.len(),
            benchmark_holdings: bench_holdings.len(),
            duration,
            spread,
            yield_comparison,
            risk,
            active_weights: active_weights_output,
            by_sector,
            by_rating,
            timestamp: chrono::Utc::now().timestamp(),
        };

        (
            StatusCode::OK,
            Json(serde_json::to_value(response).unwrap()),
        )
    })
}

/// Request for active weights calculation.
#[derive(Debug, Deserialize)]
pub struct ActiveWeightsRequest {
    /// Portfolio definition
    pub portfolio: PortfolioInput,
    /// Portfolio bond prices
    pub portfolio_prices: Vec<BondQuoteOutput>,
    /// Benchmark definition
    pub benchmark: PortfolioInput,
    /// Benchmark bond prices
    pub benchmark_prices: Vec<BondQuoteOutput>,
}

/// Calculate active weights vs benchmark.
pub async fn calculate_active_weights(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<ActiveWeightsRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        // Convert portfolio holdings
        let port_holdings = match convert_to_holdings(&request.portfolio, &request.portfolio_prices)
        {
            Ok(h) => h,
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Portfolio error: {}", e) })),
                ));
            }
        };

        // Convert benchmark holdings
        let bench_holdings =
            match convert_to_holdings(&request.benchmark, &request.benchmark_prices) {
                Ok(h) => h,
                Err(e) => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": format!("Benchmark error: {}", e) })),
                    ));
                }
            };

        let config = AnalyticsConfig::default();
        let weights = portfolio_result(active_weights(&port_holdings, &bench_holdings, &config))?;

        // Convert by_sector
        let by_sector: Vec<serde_json::Value> = weights
            .by_sector
            .iter()
            .map(|(sector, w)| {
                serde_json::json!({
                    "sector": format!("{:?}", sector),
                    "portfolio_weight": w.portfolio_weight,
                    "benchmark_weight": w.benchmark_weight,
                    "active_weight": w.active_weight,
                    "relative_weight": w.relative_weight,
                    "is_overweight": w.is_overweight()
                })
            })
            .collect();

        // Convert by_rating
        let by_rating: Vec<serde_json::Value> = weights
            .by_rating
            .iter()
            .map(|(rating, w)| {
                serde_json::json!({
                    "rating": format!("{:?}", rating),
                    "portfolio_weight": w.portfolio_weight,
                    "benchmark_weight": w.benchmark_weight,
                    "active_weight": w.active_weight,
                    "relative_weight": w.relative_weight,
                    "is_overweight": w.is_overweight()
                })
            })
            .collect();

        // Convert by_holding
        let by_holding: Vec<serde_json::Value> = weights
            .by_holding
            .iter()
            .map(|(id, w)| {
                serde_json::json!({
                    "id": id,
                    "portfolio_weight": w.portfolio_weight,
                    "benchmark_weight": w.benchmark_weight,
                    "active_weight": w.active_weight,
                    "relative_weight": w.relative_weight,
                    "is_overweight": w.is_overweight()
                })
            })
            .collect();

        let response = serde_json::json!({
            "portfolio_id": request.portfolio.portfolio_id,
            "benchmark_id": request.benchmark.portfolio_id,
            "total_active_weight": weights.total_active_weight,
            "overweight_count": weights.overweight_count,
            "underweight_count": weights.underweight_count,
            "by_sector": by_sector,
            "by_rating": by_rating,
            "by_holding": by_holding,
            "overweight_sectors": weights.overweight_sectors().into_iter()
                .map(|(s, w)| serde_json::json!({"sector": format!("{:?}", s), "weight": w}))
                .collect::<Vec<_>>(),
            "underweight_sectors": weights.underweight_sectors().into_iter()
                .map(|(s, w)| serde_json::json!({"sector": format!("{:?}", s), "weight": w}))
                .collect::<Vec<_>>(),
            "largest_positions": weights.largest_active_positions(20).into_iter()
                .map(|(id, w)| serde_json::json!({"id": id, "weight": w}))
                .collect::<Vec<_>>(),
            "timestamp": chrono::Utc::now().timestamp()
        });

        (StatusCode::OK, Json(response))
    })
}

/// Request for tracking error estimation.
#[derive(Debug, Deserialize)]
pub struct TrackingErrorRequest {
    /// Portfolio definition
    pub portfolio: PortfolioInput,
    /// Portfolio bond prices
    pub portfolio_prices: Vec<BondQuoteOutput>,
    /// Benchmark definition
    pub benchmark: PortfolioInput,
    /// Benchmark bond prices
    pub benchmark_prices: Vec<BondQuoteOutput>,
    /// Rate volatility assumption (annualized, e.g., 0.01 for 100bp)
    pub rate_vol: Option<f64>,
    /// Spread volatility assumption (annualized, e.g., 0.002 for 20bp)
    pub spread_vol: Option<f64>,
}

/// Estimate tracking error vs benchmark.
pub async fn calculate_tracking_error(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<TrackingErrorRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        // Convert portfolio holdings
        let port_holdings = match convert_to_holdings(&request.portfolio, &request.portfolio_prices)
        {
            Ok(h) => h,
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Portfolio error: {}", e) })),
                ));
            }
        };

        // Convert benchmark holdings
        let bench_holdings =
            match convert_to_holdings(&request.benchmark, &request.benchmark_prices) {
                Ok(h) => h,
                Err(e) => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": format!("Benchmark error: {}", e) })),
                    ));
                }
            };

        let config = AnalyticsConfig::default();

        // Default volatility assumptions
        let rate_vol = request.rate_vol.unwrap_or(0.01); // 100bp
        let spread_vol = request.spread_vol.unwrap_or(0.002); // 20bp

        let te = portfolio_result(estimate_tracking_error(
            &port_holdings,
            &bench_holdings,
            &config,
            rate_vol,
            spread_vol,
        ))?;

        let response = serde_json::json!({
            "portfolio_id": request.portfolio.portfolio_id,
            "benchmark_id": request.benchmark.portfolio_id,
            "tracking_error": te.tracking_error,
            "tracking_error_bps": te.tracking_error * 100.0,
            "contributions": {
                "duration": te.duration_contribution,
                "spread": te.spread_contribution,
                "sector": te.sector_contribution,
                "selection": te.selection_contribution
            },
            "active_exposures": {
                "duration": te.active_duration,
                "spread_bps": te.active_spread
            },
            "assumptions": {
                "rate_vol": rate_vol,
                "spread_vol": spread_vol
            },
            "timestamp": chrono::Utc::now().timestamp()
        });

        (StatusCode::OK, Json(response))
    })
}

/// Request for duration/spread attribution.
#[derive(Debug, Deserialize)]
pub struct AttributionRequest {
    /// Portfolio definition
    pub portfolio: PortfolioInput,
    /// Portfolio bond prices
    pub portfolio_prices: Vec<BondQuoteOutput>,
    /// Benchmark definition
    pub benchmark: PortfolioInput,
    /// Benchmark bond prices
    pub benchmark_prices: Vec<BondQuoteOutput>,
}

/// Calculate duration and spread attribution by sector.
pub async fn calculate_attribution(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<AttributionRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        // Convert portfolio holdings
        let port_holdings = match convert_to_holdings(&request.portfolio, &request.portfolio_prices)
        {
            Ok(h) => h,
            Err(e) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("Portfolio error: {}", e) })),
                ));
            }
        };

        // Convert benchmark holdings
        let bench_holdings =
            match convert_to_holdings(&request.benchmark, &request.benchmark_prices) {
                Ok(h) => h,
                Err(e) => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({ "error": format!("Benchmark error: {}", e) })),
                    ));
                }
            };

        let config = AnalyticsConfig::default();

        // Calculate duration difference by sector
        let dur_diff = portfolio_result(duration_difference_by_sector(
            &port_holdings,
            &bench_holdings,
            &config,
        ))?;

        // Calculate spread difference by sector
        let spread_diff = portfolio_result(spread_difference_by_sector(
            &port_holdings,
            &bench_holdings,
            &config,
        ))?;

        // Convert to output
        let duration_by_sector: Vec<serde_json::Value> = dur_diff
            .into_iter()
            .map(|(sector, diff)| {
                serde_json::json!({
                    "sector": format!("{:?}", sector),
                    "contribution": diff
                })
            })
            .collect();

        let spread_by_sector: Vec<serde_json::Value> = spread_diff
            .into_iter()
            .map(|(sector, diff)| {
                serde_json::json!({
                    "sector": format!("{:?}", sector),
                    "contribution": diff
                })
            })
            .collect();

        let response = serde_json::json!({
            "portfolio_id": request.portfolio.portfolio_id,
            "benchmark_id": request.benchmark.portfolio_id,
            "duration_attribution": duration_by_sector,
            "spread_attribution": spread_by_sector,
            "timestamp": chrono::Utc::now().timestamp()
        });

        (StatusCode::OK, Json(response))
    })
}

// =============================================================================
// LIQUIDITY ANALYTICS
// =============================================================================

/// Position with liquidity data for liquidity analytics.
#[derive(Debug, Deserialize)]
pub struct LiquidityPosition {
    /// Instrument ID
    pub instrument_id: String,
    /// Notional/par amount
    pub notional: Decimal,
    /// Market price (percentage of par)
    pub market_price: Option<Decimal>,
    /// Liquidity score (0-100)
    pub liquidity_score: Option<f64>,
    /// Bid-ask spread in basis points
    pub bid_ask_spread: Option<f64>,
    /// Sector (optional)
    pub sector: Option<String>,
}

/// Request for liquidity metrics calculation.
#[derive(Debug, Deserialize)]
pub struct LiquidityMetricsRequest {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Portfolio name
    pub name: String,
    /// Positions with liquidity data
    pub positions: Vec<LiquidityPosition>,
}

/// Response for liquidity metrics.
#[derive(Debug, Serialize)]
pub struct LiquidityMetricsResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Weighted average bid-ask spread (basis points)
    pub avg_bid_ask_spread: Option<f64>,
    /// Weighted average liquidity score (0-100)
    pub avg_liquidity_score: Option<f64>,
    /// Percentage of holdings classified as highly liquid
    pub highly_liquid_pct: f64,
    /// Percentage of holdings classified as moderately liquid
    pub moderately_liquid_pct: f64,
    /// Percentage of holdings classified as illiquid
    pub illiquid_pct: f64,
    /// Holdings with bid-ask spread data
    pub bid_ask_coverage: usize,
    /// Holdings with liquidity score data
    pub score_coverage: usize,
    /// Total holdings count
    pub total_holdings: usize,
    /// Bid-ask coverage percentage
    pub bid_ask_coverage_pct: f64,
    /// Score coverage percentage
    pub score_coverage_pct: f64,
    /// Whether portfolio has liquidity concerns
    pub has_liquidity_concerns: bool,
    /// Calculation timestamp
    pub timestamp: i64,
}

/// Calculate liquidity metrics for a portfolio.
pub async fn calculate_liquidity_metrics_handler(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<LiquidityMetricsRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        let holdings = portfolio_result(convert_liquidity_positions(&request.positions))?;
        let config = AnalyticsConfig::default();

        let metrics = portfolio_result(calculate_liquidity_metrics(&holdings, &config))?;

        let response = LiquidityMetricsResponse {
            portfolio_id: request.portfolio_id,
            avg_bid_ask_spread: metrics.avg_bid_ask_spread,
            avg_liquidity_score: metrics.avg_liquidity_score,
            highly_liquid_pct: metrics.highly_liquid_pct,
            moderately_liquid_pct: metrics.moderately_liquid_pct,
            illiquid_pct: metrics.illiquid_pct,
            bid_ask_coverage: metrics.bid_ask_coverage,
            score_coverage: metrics.score_coverage,
            total_holdings: metrics.total_holdings,
            bid_ask_coverage_pct: metrics.bid_ask_coverage_pct(),
            score_coverage_pct: metrics.score_coverage_pct(),
            has_liquidity_concerns: metrics.has_liquidity_concerns(),
            timestamp: chrono::Utc::now().timestamp(),
        };

        (StatusCode::OK, Json(response))
    })
}

/// Liquidity bucket output.
#[derive(Debug, Serialize)]
pub struct LiquidityBucketOutput {
    /// Bucket name
    pub bucket: String,
    /// Market value in this bucket
    pub market_value: String,
    /// Weight as percentage
    pub weight_pct: f64,
    /// Number of holdings
    pub count: usize,
    /// Average liquidity score in this bucket
    pub avg_score: Option<f64>,
    /// Average bid-ask spread in this bucket
    pub avg_spread: Option<f64>,
}

/// Response for liquidity distribution.
#[derive(Debug, Serialize)]
pub struct LiquidityDistributionResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Distribution by bucket
    pub buckets: Vec<LiquidityBucketOutput>,
    /// Total market value
    pub total_market_value: String,
    /// Holdings without liquidity data
    pub unknown_count: usize,
    /// Calculation timestamp
    pub timestamp: i64,
}

/// Calculate liquidity distribution by bucket.
pub async fn calculate_liquidity_distribution(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<LiquidityMetricsRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        let holdings = portfolio_result(convert_liquidity_positions(&request.positions))?;
        let config = AnalyticsConfig::default();

        let dist = portfolio_result(liquidity_distribution(&holdings, &config))?;

        let buckets: Vec<LiquidityBucketOutput> = dist
            .by_bucket
            .into_iter()
            .map(|(bucket, info)| LiquidityBucketOutput {
                bucket: format!("{:?}", bucket),
                market_value: info.market_value.to_string(),
                weight_pct: info.weight_pct,
                count: info.count,
                avg_score: info.avg_score,
                avg_spread: info.avg_spread,
            })
            .collect();

        let response = LiquidityDistributionResponse {
            portfolio_id: request.portfolio_id,
            buckets,
            total_market_value: dist.total_market_value.to_string(),
            unknown_count: dist.unknown_count,
            timestamp: chrono::Utc::now().timestamp(),
        };

        (StatusCode::OK, Json(response))
    })
}

/// Request for comprehensive liquidity analysis.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiquidityAnalysisRequest {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Portfolio name
    pub name: String,
    /// Positions with liquidity data
    pub positions: Vec<LiquidityPosition>,
}

/// Response for comprehensive liquidity analysis.
#[derive(Debug, Serialize)]
pub struct LiquidityAnalysisResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Liquidity metrics summary
    pub metrics: LiquidityMetricsResponse,
    /// Distribution by bucket
    pub distribution: Vec<LiquidityBucketOutput>,
    /// Calculation timestamp
    pub timestamp: i64,
}

/// Calculate comprehensive liquidity analysis.
pub async fn calculate_liquidity_analysis(
    State(_state): State<Arc<AppState>>,
    request: Result<Json<LiquidityAnalysisRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    let Json(request) = portfolio_result(request.map_err(|error| error.body_text()))?;
    Ok({
        let holdings = portfolio_result(convert_liquidity_positions(&request.positions))?;
        let config = AnalyticsConfig::default();
        let timestamp = chrono::Utc::now().timestamp();

        // Calculate metrics
        let metrics_result = portfolio_result(calculate_liquidity_metrics(&holdings, &config))?;
        let metrics = LiquidityMetricsResponse {
            portfolio_id: request.portfolio_id.clone(),
            avg_bid_ask_spread: metrics_result.avg_bid_ask_spread,
            avg_liquidity_score: metrics_result.avg_liquidity_score,
            highly_liquid_pct: metrics_result.highly_liquid_pct,
            moderately_liquid_pct: metrics_result.moderately_liquid_pct,
            illiquid_pct: metrics_result.illiquid_pct,
            bid_ask_coverage: metrics_result.bid_ask_coverage,
            score_coverage: metrics_result.score_coverage,
            total_holdings: metrics_result.total_holdings,
            bid_ask_coverage_pct: metrics_result.bid_ask_coverage_pct(),
            score_coverage_pct: metrics_result.score_coverage_pct(),
            has_liquidity_concerns: metrics_result.has_liquidity_concerns(),
            timestamp,
        };

        // Calculate distribution
        let dist = portfolio_result(liquidity_distribution(&holdings, &config))?;
        let distribution: Vec<LiquidityBucketOutput> = dist
            .by_bucket
            .into_iter()
            .map(|(bucket, info)| LiquidityBucketOutput {
                bucket: format!("{:?}", bucket),
                market_value: info.market_value.to_string(),
                weight_pct: info.weight_pct,
                count: info.count,
                avg_score: info.avg_score,
                avg_spread: info.avg_spread,
            })
            .collect();

        let response = LiquidityAnalysisResponse {
            portfolio_id: request.portfolio_id,
            metrics,
            distribution,
            timestamp,
        };

        (StatusCode::OK, Json(response))
    })
}

/// Convert liquidity positions to holdings.
fn convert_liquidity_positions(positions: &[LiquidityPosition]) -> Result<Vec<Holding>, String> {
    if positions.len() > 65_536 {
        return Err("position manifest bound exceeded".into());
    }
    let mut ids = std::collections::HashSet::new();
    positions
        .iter()
        .map(|pos| {
            if pos.instrument_id.trim().is_empty()
                || pos.instrument_id.len() > 8192
                || !ids.insert(pos.instrument_id.as_str())
            {
                return Err("unique nonempty position identities required".into());
            }
            let market_price = pos
                .market_price
                .ok_or_else(|| format!("{}: actual market price is required", pos.instrument_id))?;
            let mut analytics = HoldingAnalytics::new();
            let classification = Classification::new();
            analytics.liquidity_score = pos.liquidity_score;
            analytics.bid_ask_spread = pos.bid_ask_spread;
            HoldingBuilder::new()
                .id(&pos.instrument_id)
                .identifiers(BondIdentifiers::new())
                .par_amount(pos.notional)
                .market_price(market_price)
                .classification(classification)
                .analytics(analytics)
                .build()
                .map_err(|e| e.to_string())
        })
        .collect()
}

// =============================================================================
// CREDIT QUALITY ANALYTICS
// =============================================================================

/// Position with rating data for credit quality analytics.
#[derive(Debug, Deserialize)]
pub struct CreditPosition {
    /// Instrument ID
    pub instrument_id: String,
    /// Notional/par amount
    pub notional: Decimal,
    /// Market price (percentage of par)
    pub market_price: Option<Decimal>,
    /// Credit rating (e.g., "AAA", "BBB+", "BB-")
    pub rating: Option<String>,
}

/// Request for credit quality metrics.
#[derive(Debug, Deserialize)]
pub struct CreditQualityRequest {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Portfolio name
    pub name: String,
    /// Positions with rating data
    pub positions: Vec<CreditPosition>,
}

/// Quality tier output.
#[derive(Debug, Serialize)]
pub struct QualityTiersOutput {
    /// High quality: AAA, AA (%)
    pub high_quality: f64,
    /// Upper medium: A (%)
    pub upper_medium: f64,
    /// Lower medium: BBB (%)
    pub lower_medium: f64,
    /// Non-investment grade: BB and below (%)
    pub speculative: f64,
    /// Highly speculative: CCC and below (%)
    pub highly_speculative: f64,
    /// Default: D (%)
    pub default: f64,
    /// Not rated (%)
    pub not_rated: f64,
}

/// Response for credit quality metrics.
#[derive(Debug, Serialize)]
pub struct CreditQualityResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Weighted average rating score (1=AAA, 22=D)
    pub average_rating_score: Option<f64>,
    /// Implied average rating
    pub average_rating: Option<String>,
    /// Investment grade weight (%)
    pub ig_weight: f64,
    /// High yield weight (%)
    pub hy_weight: f64,
    /// Default weight (%)
    pub default_weight: f64,
    /// Unrated weight (%)
    pub unrated_weight: f64,
    /// Crossover risk: BBB weight (%)
    pub bbb_weight: f64,
    /// Crossover risk: BB weight (%)
    pub bb_weight: f64,
    /// Combined crossover risk (BBB + BB)
    pub crossover_risk: f64,
    /// Is majority investment grade
    pub is_investment_grade: bool,
    /// Has significant HY exposure (>10%)
    pub has_significant_hy: bool,
    /// Quality tier distribution
    pub quality_tiers: QualityTiersOutput,
    /// Total holdings count
    pub total_holdings: usize,
    /// Holdings with rating data
    pub rated_holdings: usize,
    /// Calculation timestamp
    pub timestamp: i64,
}

/// Calculate credit quality metrics for a portfolio.
pub async fn calculate_credit_quality_handler(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<CreditQualityRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        let holdings = portfolio_result(convert_credit_positions(&request.positions))?;
        let config = AnalyticsConfig::default();

        let metrics = portfolio_result(calculate_credit_quality(&holdings, &config))?;

        let rated_holdings = request
            .positions
            .iter()
            .filter(|p| p.rating.is_some())
            .count();

        let response = CreditQualityResponse {
            portfolio_id: request.portfolio_id,
            average_rating_score: metrics.average_rating_score,
            average_rating: metrics.average_rating.map(|r| format!("{:?}", r)),
            ig_weight: metrics.ig_weight,
            hy_weight: metrics.hy_weight,
            default_weight: metrics.default_weight,
            unrated_weight: metrics.unrated_weight,
            bbb_weight: metrics.bbb_weight,
            bb_weight: metrics.bb_weight,
            crossover_risk: metrics.crossover_risk(),
            is_investment_grade: metrics.is_investment_grade(),
            has_significant_hy: metrics.has_significant_hy(),
            quality_tiers: QualityTiersOutput {
                high_quality: metrics.quality_tiers.high_quality,
                upper_medium: metrics.quality_tiers.upper_medium,
                lower_medium: metrics.quality_tiers.lower_medium,
                speculative: metrics.quality_tiers.speculative,
                highly_speculative: metrics.quality_tiers.highly_speculative,
                default: metrics.quality_tiers.default,
                not_rated: metrics.quality_tiers.not_rated,
            },
            total_holdings: request.positions.len(),
            rated_holdings,
            timestamp: chrono::Utc::now().timestamp(),
        };

        (StatusCode::OK, Json(response))
    })
}

/// Fallen angel risk output.
#[derive(Debug, Serialize)]
pub struct FallenAngelRiskOutput {
    /// Weight of BBB holdings (%)
    pub bbb_weight: f64,
    /// Weight of BBB- holdings (most at risk) (%)
    pub bbb_minus_weight: f64,
    /// Total market value at risk
    pub market_value_at_risk: String,
    /// Number of holdings at risk
    pub holdings_count: usize,
}

/// Rising star risk output.
#[derive(Debug, Serialize)]
pub struct RisingStarRiskOutput {
    /// Weight of BB holdings (%)
    pub bb_weight: f64,
    /// Weight of BB+ holdings (most likely to upgrade) (%)
    pub bb_plus_weight: f64,
    /// Total market value with upgrade potential
    pub market_value_potential: String,
    /// Number of holdings with upgrade potential
    pub holdings_count: usize,
}

/// Response for migration risk.
#[derive(Debug, Serialize)]
pub struct MigrationRiskResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Fallen angel risk (IG -> HY downgrade risk)
    pub fallen_angel_risk: FallenAngelRiskOutput,
    /// Rising star potential (HY -> IG upgrade potential)
    pub rising_star_risk: RisingStarRiskOutput,
    /// Total crossover exposure (BBB + BB weight)
    pub total_crossover_exposure: f64,
    /// Calculation timestamp
    pub timestamp: i64,
}

/// Calculate migration risk (fallen angels / rising stars).
pub async fn calculate_migration_risk_handler(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<CreditQualityRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        let holdings = portfolio_result(convert_credit_positions(&request.positions))?;
        let config = AnalyticsConfig::default();

        let risk = portfolio_result(calculate_migration_risk(&holdings, &config))?;

        let response = MigrationRiskResponse {
            portfolio_id: request.portfolio_id,
            fallen_angel_risk: FallenAngelRiskOutput {
                bbb_weight: risk.fallen_angel_risk.bbb_weight,
                bbb_minus_weight: risk.fallen_angel_risk.bbb_minus_weight,
                market_value_at_risk: risk.fallen_angel_risk.market_value_at_risk.to_string(),
                holdings_count: risk.fallen_angel_risk.holdings_count,
            },
            rising_star_risk: RisingStarRiskOutput {
                bb_weight: risk.rising_star_risk.bb_weight,
                bb_plus_weight: risk.rising_star_risk.bb_plus_weight,
                market_value_potential: risk.rising_star_risk.market_value_potential.to_string(),
                holdings_count: risk.rising_star_risk.holdings_count,
            },
            total_crossover_exposure: risk.fallen_angel_risk.bbb_weight
                + risk.rising_star_risk.bb_weight,
            timestamp: chrono::Utc::now().timestamp(),
        };

        (StatusCode::OK, Json(response))
    })
}

/// Response for comprehensive credit analysis.
#[derive(Debug, Serialize)]
pub struct CreditAnalysisResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Credit quality metrics
    pub quality: CreditQualityResponse,
    /// Migration risk
    pub migration_risk: MigrationRiskResponse,
    /// Calculation timestamp
    pub timestamp: i64,
}

/// Calculate comprehensive credit analysis.
pub async fn calculate_credit_analysis(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<CreditQualityRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        let holdings = portfolio_result(convert_credit_positions(&request.positions))?;
        let config = AnalyticsConfig::default();
        let timestamp = chrono::Utc::now().timestamp();

        // Calculate quality metrics
        let metrics = portfolio_result(calculate_credit_quality(&holdings, &config))?;
        let rated_holdings = request
            .positions
            .iter()
            .filter(|p| p.rating.is_some())
            .count();

        let quality = CreditQualityResponse {
            portfolio_id: request.portfolio_id.clone(),
            average_rating_score: metrics.average_rating_score,
            average_rating: metrics.average_rating.map(|r| format!("{:?}", r)),
            ig_weight: metrics.ig_weight,
            hy_weight: metrics.hy_weight,
            default_weight: metrics.default_weight,
            unrated_weight: metrics.unrated_weight,
            bbb_weight: metrics.bbb_weight,
            bb_weight: metrics.bb_weight,
            crossover_risk: metrics.crossover_risk(),
            is_investment_grade: metrics.is_investment_grade(),
            has_significant_hy: metrics.has_significant_hy(),
            quality_tiers: QualityTiersOutput {
                high_quality: metrics.quality_tiers.high_quality,
                upper_medium: metrics.quality_tiers.upper_medium,
                lower_medium: metrics.quality_tiers.lower_medium,
                speculative: metrics.quality_tiers.speculative,
                highly_speculative: metrics.quality_tiers.highly_speculative,
                default: metrics.quality_tiers.default,
                not_rated: metrics.quality_tiers.not_rated,
            },
            total_holdings: request.positions.len(),
            rated_holdings,
            timestamp,
        };

        // Calculate migration risk
        let risk = portfolio_result(calculate_migration_risk(&holdings, &config))?;
        let migration_risk = MigrationRiskResponse {
            portfolio_id: request.portfolio_id.clone(),
            fallen_angel_risk: FallenAngelRiskOutput {
                bbb_weight: risk.fallen_angel_risk.bbb_weight,
                bbb_minus_weight: risk.fallen_angel_risk.bbb_minus_weight,
                market_value_at_risk: risk.fallen_angel_risk.market_value_at_risk.to_string(),
                holdings_count: risk.fallen_angel_risk.holdings_count,
            },
            rising_star_risk: RisingStarRiskOutput {
                bb_weight: risk.rising_star_risk.bb_weight,
                bb_plus_weight: risk.rising_star_risk.bb_plus_weight,
                market_value_potential: risk.rising_star_risk.market_value_potential.to_string(),
                holdings_count: risk.rising_star_risk.holdings_count,
            },
            total_crossover_exposure: risk.fallen_angel_risk.bbb_weight
                + risk.rising_star_risk.bb_weight,
            timestamp,
        };

        let response = CreditAnalysisResponse {
            portfolio_id: request.portfolio_id,
            quality,
            migration_risk,
            timestamp,
        };

        (StatusCode::OK, Json(response))
    })
}

/// Convert credit positions to holdings.
fn convert_credit_positions(positions: &[CreditPosition]) -> Result<Vec<Holding>, String> {
    if positions.len() > 65_536 {
        return Err("position manifest bound exceeded".into());
    }
    let mut ids = std::collections::HashSet::new();
    positions
        .iter()
        .map(|pos| {
            if pos.instrument_id.trim().is_empty()
                || pos.instrument_id.len() > 8192
                || !ids.insert(pos.instrument_id.as_str())
            {
                return Err("unique nonempty position identities required".into());
            }
            let market_price = pos
                .market_price
                .ok_or_else(|| format!("{}: actual market price is required", pos.instrument_id))?;
            let analytics = HoldingAnalytics::new();
            let mut classification = Classification::new();
            if let Some(value) = &pos.rating {
                let rating = CreditRating::parse(value)
                    .ok_or_else(|| format!("{}: invalid rating {value}", pos.instrument_id))?;
                classification =
                    Classification::new().with_rating(RatingInfo::from_composite(rating));
            }
            HoldingBuilder::new()
                .id(&pos.instrument_id)
                .identifiers(BondIdentifiers::new())
                .par_amount(pos.notional)
                .market_price(market_price)
                .classification(classification)
                .analytics(analytics)
                .build()
                .map_err(|e| e.to_string())
        })
        .collect()
}

// =============================================================================
// ETF SEC YIELD ANALYTICS
// =============================================================================

/// Request for SEC 30-day yield calculation.
#[derive(Debug, Deserialize)]
pub struct SecYieldRequest {
    /// ETF ID
    pub etf_id: String,
    /// Net investment income over 30 days
    pub net_investment_income: Decimal,
    /// Average shares outstanding during the 30-day period
    pub avg_shares_outstanding: Decimal,
    /// Maximum offering price per share at period end
    pub max_offering_price: Decimal,
    /// Gross expenses before waivers (optional)
    pub gross_expenses: Option<Decimal>,
    /// Fee waivers during the period (optional)
    pub fee_waivers: Option<Decimal>,
    /// As-of date (YYYY-MM-DD)
    pub as_of_date: String,
}

/// Response for SEC 30-day yield.
#[derive(Debug, Serialize)]
pub struct SecYieldResponse {
    /// ETF ID
    pub etf_id: String,
    /// SEC 30-day yield (annualized)
    pub sec_30_day_yield: f64,
    /// Unsubsidized SEC yield (before fee waivers)
    pub unsubsidized_yield: Option<f64>,
    /// Fee waiver impact (difference between subsidized and unsubsidized)
    pub fee_waiver_impact: Option<f64>,
    /// Dividend income component
    pub dividend_income: String,
    /// Interest income component
    pub interest_income: String,
    /// Total income
    pub total_income: String,
    /// Average shares outstanding
    pub avg_shares: String,
    /// Maximum offering price
    pub max_offering_price: String,
    /// As-of date
    pub as_of_date: String,
    /// Calculation timestamp
    pub timestamp: i64,
}

/// Calculate SEC 30-day yield.
pub async fn calculate_sec_yield_handler(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<SecYieldRequest>,
) -> impl IntoResponse {
    let as_of_date = match parse_date(&request.as_of_date) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };

    let input = SecYieldInput {
        net_investment_income: request.net_investment_income,
        avg_shares_outstanding: request.avg_shares_outstanding,
        max_offering_price: request.max_offering_price,
        gross_expenses: request.gross_expenses,
        fee_waivers: request.fee_waivers,
        as_of_date,
    };

    let result = calculate_sec_yield(&input);

    let response = SecYieldResponse {
        etf_id: request.etf_id,
        sec_30_day_yield: result.sec_30_day_yield,
        unsubsidized_yield: result.unsubsidized_yield,
        fee_waiver_impact: result.fee_waiver_impact(),
        dividend_income: result.dividend_income.to_string(),
        interest_income: result.interest_income.to_string(),
        total_income: result.total_income.to_string(),
        avg_shares: result.avg_shares.to_string(),
        max_offering_price: result.max_offering_price.to_string(),
        as_of_date: request.as_of_date,
        timestamp: chrono::Utc::now().timestamp(),
    };

    (StatusCode::OK, Json(response)).into_response()
}

// =============================================================================
// ETF BASKET ANALYTICS
// =============================================================================

/// Position for basket creation.
#[derive(Debug, Deserialize)]
pub struct BasketPosition {
    /// Instrument ID
    pub instrument_id: String,
    /// Notional/par amount
    pub notional: Decimal,
    /// Market price
    pub market_price: Decimal,
    /// ISIN (optional)
    pub isin: Option<String>,
}

/// Request for creation basket.
#[derive(Debug, Deserialize)]
pub struct CreationBasketRequest {
    /// ETF ID
    pub etf_id: String,
    /// Creation unit size (number of ETF shares per creation unit)
    pub creation_unit_size: Decimal,
    /// Total ETF shares outstanding
    pub total_shares: Decimal,
    /// Cash balance in the portfolio
    #[serde(default)]
    pub cash_balance: Decimal,
    /// Holdings in the portfolio
    pub holdings: Vec<BasketPosition>,
}

/// Basket component output.
#[derive(Debug, Serialize)]
pub struct BasketComponentOutput {
    /// Holding identifier
    pub holding_id: String,
    /// Security identifier (ISIN)
    pub security_id: String,
    /// Quantity
    pub quantity: String,
    /// Price
    pub price: String,
    /// Market value
    pub market_value: String,
    /// Weight (%)
    pub weight_pct: f64,
}

/// Response for creation basket.
#[derive(Debug, Serialize)]
pub struct CreationBasketResponse {
    /// ETF ID
    pub etf_id: String,
    /// Creation unit size
    pub creation_unit_size: String,
    /// Components in the basket
    pub components: Vec<BasketComponentOutput>,
    /// Number of securities
    pub security_count: usize,
    /// Securities value
    pub securities_value: String,
    /// Cash component
    pub cash_component: String,
    /// Total creation unit value
    pub total_value: String,
    /// NAV per creation unit
    pub nav_per_cu: f64,
    /// NAV per share
    pub nav_per_share: f64,
    /// Cash percentage
    pub cash_pct: f64,
    /// Calculation timestamp
    pub timestamp: i64,
}

/// Build creation basket for an ETF.
pub async fn build_creation_basket_handler(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<CreationBasketRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        let holdings = portfolio_result(convert_basket_positions(&request.holdings))?;

        let basket = portfolio_result(build_creation_basket(
            &holdings,
            request.creation_unit_size,
            request.total_shares,
            request.cash_balance,
        ))?;

        let components: Vec<BasketComponentOutput> = basket
            .components
            .iter()
            .map(|c| BasketComponentOutput {
                holding_id: c.holding_id.clone(),
                security_id: c.security_id.clone(),
                quantity: c.quantity.to_string(),
                price: c.price.to_string(),
                market_value: c.market_value.to_string(),
                weight_pct: c.weight_pct,
            })
            .collect();

        let response = CreationBasketResponse {
            etf_id: request.etf_id,
            creation_unit_size: basket.creation_unit_size.to_string(),
            components,
            security_count: basket.security_count,
            securities_value: basket.securities_value.to_string(),
            cash_component: basket.cash_component.to_string(),
            total_value: basket.total_value.to_string(),
            nav_per_cu: basket.nav_per_cu,
            nav_per_share: basket.nav_per_share(),
            cash_pct: basket.cash_pct(),
            timestamp: chrono::Utc::now().timestamp(),
        };

        (StatusCode::OK, Json(response))
    })
}

/// Request for basket analysis.
#[derive(Debug, Deserialize)]
pub struct BasketAnalysisRequest {
    /// ETF ID
    pub etf_id: String,
    /// Creation unit size
    pub creation_unit_size: Decimal,
    /// Total shares outstanding
    pub total_shares: Decimal,
    /// Cash balance
    #[serde(default)]
    pub cash_balance: Decimal,
    /// Current basket holdings
    pub basket_holdings: Vec<BasketPosition>,
    /// Target/benchmark holdings to compare against
    pub target_holdings: Vec<BasketPosition>,
}

/// Weight difference output.
#[derive(Debug, Serialize)]
pub struct WeightDifferenceOutput {
    /// Holding ID
    pub holding_id: String,
    /// Weight difference (%)
    pub diff_pct: f64,
}

/// Response for basket analysis.
#[derive(Debug, Serialize)]
pub struct BasketAnalysisResponse {
    /// ETF ID
    pub etf_id: String,
    /// Holdings excluded from basket
    pub excluded_holdings: Vec<String>,
    /// Holdings added to basket
    pub added_holdings: Vec<String>,
    /// Largest weight differences
    pub weight_differences: Vec<WeightDifferenceOutput>,
    /// Duration difference
    pub duration_diff: Option<f64>,
    /// Yield difference
    pub yield_diff: Option<f64>,
    /// Tracking error estimate (bps)
    pub tracking_error_bps: Option<f64>,
    /// Calculation timestamp
    pub timestamp: i64,
}

/// Analyze basket versus target holdings.
pub async fn analyze_basket_handler(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<BasketAnalysisRequest>,
) -> Result<impl IntoResponse, PortfolioHandlerError> {
    Ok({
        let basket_holdings = portfolio_result(convert_basket_positions(&request.basket_holdings))?;
        let target_holdings = portfolio_result(convert_basket_positions(&request.target_holdings))?;
        let config = AnalyticsConfig::default();

        // Build the basket first
        let basket = portfolio_result(build_creation_basket(
            &basket_holdings,
            request.creation_unit_size,
            request.total_shares,
            request.cash_balance,
        ))?;

        // Analyze against target
        let analysis = portfolio_result(analyze_basket(&basket, &target_holdings, &config))?;

        let weight_differences: Vec<WeightDifferenceOutput> = analysis
            .weight_differences
            .iter()
            .map(|(id, diff)| WeightDifferenceOutput {
                holding_id: id.clone(),
                diff_pct: *diff,
            })
            .collect();

        let response = BasketAnalysisResponse {
            etf_id: request.etf_id,
            excluded_holdings: analysis.excluded_holdings,
            added_holdings: analysis.added_holdings,
            weight_differences,
            duration_diff: analysis.duration_diff,
            yield_diff: analysis.yield_diff,
            tracking_error_bps: analysis.tracking_error_bps,
            timestamp: chrono::Utc::now().timestamp(),
        };

        (StatusCode::OK, Json(response))
    })
}

/// Convert basket positions to holdings.
fn convert_basket_positions(positions: &[BasketPosition]) -> Result<Vec<Holding>, String> {
    if positions.len() > 65_536 {
        return Err("position manifest bound exceeded".into());
    }
    let mut ids = std::collections::HashSet::new();
    positions
        .iter()
        .map(|pos| {
            if pos.instrument_id.trim().is_empty()
                || pos.instrument_id.len() > 8192
                || !ids.insert(pos.instrument_id.as_str())
            {
                return Err("unique nonempty position identities required".into());
            }
            let market_price = pos.market_price;
            let analytics = HoldingAnalytics::new();
            let classification = Classification::new();

            HoldingBuilder::new()
                .id(&pos.instrument_id)
                .identifiers(BondIdentifiers::new())
                .par_amount(pos.notional)
                .market_price(market_price)
                .classification(classification)
                .analytics(analytics)
                .build()
                .map_err(|e| e.to_string())
        })
        .collect()
}

// =============================================================================
// KEY RATE DURATION ANALYTICS
// =============================================================================

/// One supplied position with explicitly calculated key-rate sensitivities.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyRatePosition {
    /// Exact caller instrument identity.
    pub instrument_id: String,
    /// Positive supplied par amount, in the request's common base currency.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    pub notional: Decimal,
    /// Full dirty price per100 on the same declared basis as supplied duration.
    #[serde(default, with = "convex_core::contracts::decimal_serde::optional")]
    pub market_price: Option<Decimal>,
    /// Exact tenor/duration pairs. Missing factors remain unavailable.
    pub key_rate_durations: Option<Vec<(f64, f64)>>,
}

/// Checked supplied key-rate aggregation request; no pricing inputs are inferred.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyRateDurationRequest {
    /// Caller portfolio identity, echoed in the result.
    pub portfolio_id: String,
    /// Caller portfolio name, echoed in the result.
    pub name: String,
    /// Supplied same-base positions with optional calculated factor profiles.
    pub positions: Vec<KeyRatePosition>,
    /// Exact requested tenor points; absent uses native standard tenors.
    pub tenors: Option<Vec<f64>>,
    /// Duration presentation weighting; absent uses market value.
    pub weighting: Option<convex_portfolio::WeightingMethod>,
}

/// Exact requested factor with separate known currency risk and coverage.
#[derive(Debug, Serialize)]
pub struct KeyRatePointOutput {
    /// Supplied tenor in years.
    pub tenor: f64,
    /// Known weighted duration; null when no covered weight exists.
    pub duration: Option<f64>,
    /// Signed percentage of the known duration sum; null for undefined ratio.
    pub contribution_pct: Option<f64>,
    /// Currency DV01 of actually covered holdings only, independent of weights.
    pub known_partial_dv01: Option<f64>,
    /// Full supplied-book factor DV01, available only with complete coverage.
    pub partial_dv01: Option<f64>,
    /// Holdings explicitly declaring this factor, including supplied zeros.
    pub covered_holdings: usize,
    /// All supplied holdings.
    pub total_holdings: usize,
    /// Whether every supplied holding explicitly declares this factor.
    pub complete: bool,
}

/// Aggregation of supplied factors with unavailable metrics represented as null.
#[derive(Debug, Serialize)]
pub struct KeyRateDurationResponse {
    /// Caller portfolio identity.
    pub portfolio_id: String,
    /// Caller portfolio name.
    pub name: String,
    /// Every requested factor in original order, including unavailable points.
    pub profile: Vec<KeyRatePointOutput>,
    /// Sum of known weighted factors; partial coverage is explicit below.
    pub total_duration: Option<f64>,
    /// Known short-end (<2years) duration; null if no defined profile exists.
    pub short_duration: Option<f64>,
    /// Known intermediate (2..10years) duration; null without a profile.
    pub intermediate_duration: Option<f64>,
    /// Known long-end (>10years) duration; null without a profile.
    pub long_duration: Option<f64>,
    /// Holdings declaring every requested factor.
    pub coverage: usize,
    /// All supplied holdings.
    pub total_holdings: usize,
    /// Percentage of holdings declaring every requested factor.
    pub coverage_pct: f64,
    /// Complete factor coverage across every supplied holding.
    pub coverage_complete: bool,
    /// Whether any weighted-duration presentation is defined.
    pub duration_available: bool,
    /// Server calculation timestamp; not a market-data or risk revision.
    pub timestamp: i64,
}

/// Aggregate supplied key-rate profiles without inventing missing factor risk.
pub async fn calculate_key_rate_duration_handler(
    State(state): State<Arc<AppState>>,
    incoming: axum::extract::Request,
) -> impl IntoResponse {
    let (parts, body) = incoming.into_parts();
    let bytes =
        match axum::body::to_bytes(body, convex_core::contracts::json::MAX_INPUT_BYTES).await {
            Ok(bytes) => bytes,
            Err(error) => {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(serde_json::json!({"error":error.to_string()})),
                )
                    .into_response()
            }
        };
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error":error.to_string()})),
            )
                .into_response()
        }
    };
    if let Err(error) = convex_core::contracts::json::parse(text) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":error})),
        )
            .into_response();
    }
    // The framework retains its content-type and typed-extractor diagnostics.
    // Admission above preserves raw numeric intent before typed deserialization.
    let incoming = axum::http::Request::from_parts(parts, axum::body::Body::from(bytes));
    let Json(request) = match <Json<KeyRateDurationRequest> as axum::extract::FromRequest<
        Arc<AppState>,
    >>::from_request(incoming, &state)
    .await
    {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };
    let holdings = match convert_key_rate_positions(&request.positions) {
        Ok(holdings) => holdings,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error":error})),
            )
                .into_response()
        }
    };
    let config = AnalyticsConfig {
        weighting: request.weighting.unwrap_or_default(),
        ..AnalyticsConfig::default()
    };
    let exposure = match aggregate_key_rate_exposure(&holdings, &config, request.tenors.as_deref())
    {
        Ok(exposure) => exposure,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error":error.to_string()})),
            )
                .into_response()
        }
    };
    let total = exposure
        .profile
        .as_ref()
        .map(|profile| profile.total_duration);
    let profile = exposure
        .factors
        .iter()
        .map(|point| {
            let duration = point.duration.map(|duration| duration.as_f64());
            let known = point.known_partial_dv01.map(|risk| risk.as_f64());
            let contribution_pct = total
                .filter(|value| *value != 0.0)
                .and_then(|total| duration.map(|duration| duration / total * 100.0));
            KeyRatePointOutput {
                tenor: point.tenor,
                duration,
                contribution_pct,
                known_partial_dv01: known,
                partial_dv01: known.filter(|_| point.is_complete()),
                covered_holdings: point.covered_holdings,
                total_holdings: point.total_holdings,
                complete: point.is_complete(),
            }
        })
        .collect();
    let coverage = exposure.complete_holdings;
    let bucket = |predicate: fn(f64) -> bool| {
        let points: Vec<_> = exposure
            .factors
            .iter()
            .filter(|point| predicate(point.tenor))
            .collect();
        if points.is_empty() {
            return None;
        }
        points.iter().try_fold(0.0, |sum, point| {
            point.duration.map(|duration| sum + duration.as_f64())
        })
    };
    let response = KeyRateDurationResponse {
        portfolio_id: request.portfolio_id,
        name: request.name,
        profile,
        total_duration: total,
        short_duration: bucket(|tenor| tenor < 2.0),
        intermediate_duration: bucket(|tenor| (2.0..=10.0).contains(&tenor)),
        long_duration: bucket(|tenor| tenor > 10.0),
        coverage,
        total_holdings: holdings.len(),
        coverage_pct: if holdings.is_empty() {
            0.0
        } else {
            coverage as f64 / holdings.len() as f64 * 100.0
        },
        coverage_complete: !holdings.is_empty()
            && exposure.factors.iter().all(|point| point.is_complete()),
        duration_available: exposure.profile.is_some(),
        timestamp: chrono::Utc::now().timestamp(),
    };
    (StatusCode::OK, Json(response)).into_response()
}

/// Convert key rate positions to holdings with KRD data.
fn convert_key_rate_positions(positions: &[KeyRatePosition]) -> Result<Vec<Holding>, String> {
    if positions.len() > 65_536 {
        return Err("at most65536key-rate positions required".into());
    }
    let mut ids = std::collections::HashSet::new();
    positions
        .iter()
        .map(|pos| {
            if pos.instrument_id.trim().is_empty()
                || pos.instrument_id.len() > 8192
                || !ids.insert(pos.instrument_id.as_str())
            {
                return Err("unique nonempty instrument IDs required".into());
            }
            let market_price = pos
                .market_price
                .ok_or_else(|| "market_price is required for key-rate weighting".to_owned())?;
            let mut analytics = HoldingAnalytics::new();
            if let Some(krd_data) = &pos.key_rate_durations {
                if krd_data.len() > 4096 {
                    return Err("at most4096key-rate rows required".into());
                }
                let rows = krd_data
                    .iter()
                    .map(|(tenor, value)| {
                        AnalyticsDuration::try_from(*value)
                            .map(|duration| KeyRateDuration {
                                tenor: *tenor,
                                duration,
                            })
                            .map_err(|error| error.to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let rates = KeyRateDurations::new(rows);
                rates.total_duration().map_err(|error| error.to_string())?;
                analytics.key_rate_durations = Some(rates);
            }
            HoldingBuilder::new()
                .id(&pos.instrument_id)
                .identifiers(BondIdentifiers::new())
                .par_amount(pos.notional)
                .market_price(market_price)
                .analytics(analytics)
                .build()
                .map_err(|error| error.to_string())
        })
        .collect()
}

// =============================================================================
// HELPERS
// =============================================================================

/// Parse a date string (YYYY-MM-DD) into a Date.
fn parse_date(s: &str) -> Result<Date, String> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return Err(format!("Invalid date format: {} (expected YYYY-MM-DD)", s));
    }

    let year = parts[0]
        .parse::<i32>()
        .map_err(|_| format!("Invalid year: {}", parts[0]))?;
    let month = parts[1]
        .parse::<u32>()
        .map_err(|_| format!("Invalid month: {}", parts[1]))?;
    let day = parts[2]
        .parse::<u32>()
        .map_err(|_| format!("Invalid day: {}", parts[2]))?;

    Date::from_ymd(year, month, day).map_err(|e| format!("Invalid date: {}", e))
}

fn parse_currency(s: &str) -> Currency {
    match s.to_uppercase().as_str() {
        "USD" => Currency::USD,
        "EUR" => Currency::EUR,
        "GBP" => Currency::GBP,
        "JPY" => Currency::JPY,
        "CHF" => Currency::CHF,
        "CAD" => Currency::CAD,
        "AUD" => Currency::AUD,
        "NZD" => Currency::NZD,
        "SEK" => Currency::SEK,
        "NOK" => Currency::NOK,
        "DKK" => Currency::DKK,
        "HKD" => Currency::HKD,
        "SGD" => Currency::SGD,
        "CNY" => Currency::CNY,
        "INR" => Currency::INR,
        "BRL" => Currency::BRL,
        "MXN" => Currency::MXN,
        "ZAR" => Currency::ZAR,
        _ => Currency::USD, // Default to USD for unknown currencies
    }
}
