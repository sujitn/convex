//! Request handlers.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Datelike;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use convex_core::Date;
use convex_core::Currency;
use convex_analytics::risk::{Duration as AnalyticsDuration, KeyRateDuration, KeyRateDurations};
use convex_bonds::types::BondIdentifiers;
use convex_engine::{Portfolio, Position, PricingEngine};
use convex_ext_file::{InMemoryBondStore, InMemoryPortfolioStore, PortfolioFilter, StoredPortfolio, StoredPosition};
use convex_portfolio::{
    AnalyticsConfig, Holding, HoldingAnalytics, HoldingBuilder,
    duration_contributions, dv01_contributions, spread_contributions, cs01_contributions,
    DurationContributions, Dv01Contributions, SpreadContributions, Cs01Contributions,
    HoldingContribution, BucketContribution, Sector, RatingBucket,
    bucket_by_sector, bucket_by_rating, bucket_by_maturity,
    bucket_by_country, bucket_by_issuer, bucket_by_currency,
    BucketMetrics,
    // Stress testing
    run_stress_scenario, run_stress_scenarios, summarize_results,
    stress_scenarios, RateScenario, SpreadScenario, StressScenario,
    StressResult,
    // Benchmark comparison
    benchmark_comparison, active_weights, estimate_tracking_error,
    duration_difference_by_sector, spread_difference_by_sector,
    // Liquidity analytics
    calculate_liquidity_metrics, liquidity_distribution, estimate_days_to_liquidate,
    // Credit quality analytics
    calculate_credit_quality, calculate_migration_risk,
    CreditRating, Classification, RatingInfo,
    // Key rate duration
    aggregate_key_rate_profile,
    // ETF analytics
    calculate_sec_yield, SecYieldInput, build_creation_basket, analyze_basket,
    // Portfolio
    Portfolio as ConvexPortfolio, PortfolioBuilder,
};

use crate::websocket::WebSocketState;
use convex_traits::ids::{CurveId, EtfId, InstrumentId, PortfolioId};
use convex_traits::output::{BondQuoteOutput, EtfQuoteOutput, PortfolioAnalyticsOutput};
use convex_traits::reference_data::{BondFilter, BondReferenceData, BondReferenceSource, EtfHoldingEntry, EtfHoldings};

/// Application state.
pub struct AppState {
    /// The pricing engine
    pub engine: Arc<PricingEngine>,
    /// WebSocket state for real-time streaming
    pub ws_state: WebSocketState,
    /// Bond reference data store (for CRUD operations)
    pub bond_store: Arc<InMemoryBondStore>,
    /// Portfolio store (for CRUD operations)
    pub portfolio_store: Arc<InMemoryPortfolioStore>,
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

/// Error response.
#[derive(Serialize)]
pub struct ErrorResponse {
    error: String,
}

impl ErrorResponse {
    fn new(message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
        }
    }
}

/// Query parameters for single bond quote.
#[derive(Debug, Deserialize)]
pub struct BondQuoteQuery {
    /// Settlement date (YYYY-MM-DD). Defaults to today.
    pub settlement_date: Option<String>,
    /// Market price for spread calculations.
    pub market_price: Option<Decimal>,
}

/// Get bond quote by instrument ID.
///
/// Looks up bond reference data and prices the bond.
pub async fn get_bond_quote(
    State(state): State<Arc<AppState>>,
    Path(instrument_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<BondQuoteQuery>,
) -> impl IntoResponse {
    use convex_engine::pricing_router::PricingInput;

    let id = InstrumentId::new(&instrument_id);

    // Look up bond reference data
    let bond = match state.engine.reference_data().bonds.get_by_id(&id).await {
        Ok(Some(bond)) => bond,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": format!("Bond not found: {}", instrument_id)
                })),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to look up bond: {}", e)
                })),
            );
        }
    };

    // Parse or default settlement date
    let settlement_date = match query.settlement_date {
        Some(ref date_str) => match parse_date(date_str) {
            Ok(d) => d,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e })),
                );
            }
        },
        None => {
            // Default to today
            let now = chrono::Utc::now();
            Date::from_ymd(now.year(), now.month(), now.day())
                .unwrap_or_else(|_| Date::from_ymd(2024, 1, 15).unwrap())
        }
    };

    // Build pricing input
    let input = PricingInput {
        bond,
        settlement_date,
        market_price: query.market_price,
        discount_curve: None,
        benchmark_curve: None,
        government_curve: None,
        volatility: None,
    };

    // Price the bond
    let router = state.engine.pricing_router();
    match router.price(&input) {
        Ok(quote) => {
            // Publish to WebSocket subscribers
            state.ws_state.publish_bond_quote(quote.clone());
            (StatusCode::OK, Json(serde_json::to_value(quote).unwrap()))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Pricing failed: {}", e)
            })),
        ),
    }
}

// =============================================================================
// SINGLE BOND PRICING (POST)
// =============================================================================

/// Request for single bond pricing.
#[derive(Debug, Deserialize)]
pub struct SingleBondPricingRequest {
    /// Bond reference data
    pub bond: BondReferenceData,
    /// Settlement date (YYYY-MM-DD)
    pub settlement_date: String,
    /// Market price (optional, for spread calculations)
    pub market_price: Option<Decimal>,
}

/// Price a single bond (POST endpoint).
///
/// Use this when you have bond reference data and want to price on-demand.
pub async fn price_single_bond(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SingleBondPricingRequest>,
) -> impl IntoResponse {
    use convex_engine::pricing_router::PricingInput;

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

    // Build pricing input
    let input = PricingInput {
        bond: request.bond,
        settlement_date,
        market_price: request.market_price,
        discount_curve: None,
        benchmark_curve: None,
        government_curve: None,
        volatility: None,
    };

    // Price the bond
    let router = state.engine.pricing_router();
    match router.price(&input) {
        Ok(quote) => {
            // Publish to WebSocket subscribers
            state.ws_state.publish_bond_quote(quote.clone());
            (StatusCode::OK, Json(serde_json::to_value(quote).unwrap()))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Pricing failed: {}", e)
            })),
        ),
    }
}

/// Get curve.
pub async fn get_curve(
    State(state): State<Arc<AppState>>,
    Path(curve_id): Path<String>,
) -> impl IntoResponse {
    let id = CurveId::new(curve_id);

    if let Some(curve) = state.engine.curve_builder().get(&id) {
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "curve_id": curve.curve_id.as_str(),
                "built_at": curve.built_at,
                "points": curve.points,
            })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Curve not found"
            })),
        )
    }
}

/// List curves.
pub async fn list_curves(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let curves: Vec<String> = state
        .engine
        .curve_builder()
        .list()
        .iter()
        .map(|c| c.as_str().to_string())
        .collect();

    Json(serde_json::json!({
        "curves": curves
    }))
}

// =============================================================================
// CURVE MANAGEMENT
// =============================================================================

/// Request for creating a curve.
#[derive(Debug, Deserialize)]
pub struct CreateCurveRequest {
    /// Curve identifier
    pub curve_id: String,
    /// Reference date (YYYY-MM-DD)
    pub reference_date: String,
    /// Curve points: [(tenor_years, zero_rate_decimal), ...]
    pub points: Vec<CurvePointInput>,
}

/// Curve point input.
#[derive(Debug, Deserialize)]
pub struct CurvePointInput {
    /// Tenor in years
    pub tenor: f64,
    /// Zero rate as decimal (e.g., 0.04 for 4%)
    pub rate: f64,
}

/// Response for curve creation.
#[derive(Debug, Serialize)]
pub struct CurveResponse {
    /// Curve identifier
    pub curve_id: String,
    /// Reference date (YYYY-MM-DD)
    pub reference_date: String,
    /// Curve points
    pub points: Vec<(f64, f64)>,
    /// Build timestamp
    pub built_at: i64,
}

/// Create a new curve from points.
pub async fn create_curve(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateCurveRequest>,
) -> impl IntoResponse {
    // Parse reference date
    let reference_date = match parse_date(&request.reference_date) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    // Validate points
    if request.points.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "At least one curve point is required" })),
        );
    }

    // Convert points
    let points: Vec<(f64, f64)> = request
        .points
        .iter()
        .map(|p| (p.tenor, p.rate))
        .collect();

    let curve_id = CurveId::new(&request.curve_id);

    // Create the curve
    match state
        .engine
        .curve_builder()
        .create_from_points(curve_id.clone(), reference_date, points)
    {
        Ok(curve) => {
            let response = CurveResponse {
                curve_id: curve.curve_id.as_str().to_string(),
                reference_date: format!(
                    "{:04}-{:02}-{:02}",
                    curve.reference_date.year(),
                    curve.reference_date.month(),
                    curve.reference_date.day()
                ),
                points: curve.points.clone(),
                built_at: curve.built_at,
            };
            (StatusCode::CREATED, Json(serde_json::to_value(response).unwrap()))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// Delete a curve.
pub async fn delete_curve(
    State(state): State<Arc<AppState>>,
    Path(curve_id): Path<String>,
) -> impl IntoResponse {
    let id = CurveId::new(&curve_id);

    if state.engine.curve_builder().delete(&id) {
        (StatusCode::NO_CONTENT, Json(serde_json::json!({})))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Curve not found" })),
        )
    }
}

/// Query parameters for curve rate queries.
#[derive(Debug, Deserialize)]
pub struct CurveRateQuery {
    /// Compounding convention (continuous, annual, semiannual, quarterly, monthly)
    #[serde(default = "default_compounding")]
    pub compounding: String,
}

fn default_compounding() -> String {
    "continuous".to_string()
}

/// Get zero rate for a tenor.
pub async fn get_curve_zero_rate(
    State(state): State<Arc<AppState>>,
    Path((curve_id, tenor)): Path<(String, f64)>,
    axum::extract::Query(query): axum::extract::Query<CurveRateQuery>,
) -> impl IntoResponse {
    use convex_curves::{Compounding, RateCurveDyn};

    let id = CurveId::new(&curve_id);

    let curve = match state.engine.curve_builder().get(&id) {
        Some(c) => c,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Curve not found" })),
            );
        }
    };

    let compounding = match query.compounding.to_lowercase().as_str() {
        "simple" => Compounding::Simple,
        "annual" => Compounding::Annual,
        "semiannual" | "semi_annual" => Compounding::SemiAnnual,
        "quarterly" => Compounding::Quarterly,
        "monthly" => Compounding::Monthly,
        "daily" => Compounding::Daily,
        _ => Compounding::Continuous,
    };

    match curve.zero_rate(tenor, compounding) {
        Ok(rate) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "curve_id": curve_id,
                "tenor": tenor,
                "zero_rate": rate,
                "compounding": query.compounding
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// Get discount factor for a tenor.
pub async fn get_curve_discount_factor(
    State(state): State<Arc<AppState>>,
    Path((curve_id, tenor)): Path<(String, f64)>,
) -> impl IntoResponse {
    use convex_curves::RateCurveDyn;

    let id = CurveId::new(&curve_id);

    let curve = match state.engine.curve_builder().get(&id) {
        Some(c) => c,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Curve not found" })),
            );
        }
    };

    match curve.discount_factor(tenor) {
        Ok(df) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "curve_id": curve_id,
                "tenor": tenor,
                "discount_factor": df
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// Get forward rate between two tenors.
pub async fn get_curve_forward_rate(
    State(state): State<Arc<AppState>>,
    Path((curve_id, t1, t2)): Path<(String, f64, f64)>,
) -> impl IntoResponse {
    use convex_curves::RateCurveDyn;

    let id = CurveId::new(&curve_id);

    let curve = match state.engine.curve_builder().get(&id) {
        Some(c) => c,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Curve not found" })),
            );
        }
    };

    match curve.forward_rate(t1, t2) {
        Ok(rate) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "curve_id": curve_id,
                "t1": t1,
                "t2": t2,
                "forward_rate": rate
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
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
    let inputs: Vec<PricingInput> = request
        .bonds
        .into_iter()
        .map(|item| PricingInput {
            bond: item.bond,
            settlement_date,
            market_price: item.market_price,
            discount_curve: None,
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        })
        .collect();

    let router = state.engine.pricing_router();
    let start = Instant::now();

    // Execute pricing (parallel or sequential)
    let results = if request.parallel {
        router.price_batch_parallel(&inputs)
    } else {
        router.price_batch(&inputs)
    };

    let elapsed = start.elapsed();
    let elapsed_ms = elapsed.as_millis() as u64;

    // Separate successes and failures
    let mut quotes = Vec::new();
    let mut errors = Vec::new();

    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(quote) => {
                // Publish to WebSocket subscribers
                state.ws_state.publish_bond_quote(quote.clone());
                quotes.push(quote);
            }
            Err(e) => errors.push(PricingError {
                instrument_id: inputs
                    .get(i)
                    .map(|inp| inp.bond.instrument_id.as_str().to_string())
                    .unwrap_or_else(|| format!("index_{}", i)),
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

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
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
    let results = etf_pricer.calculate_inav_batch(&holdings_list, &request.bond_prices, settlement_date);

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

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
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
) -> impl IntoResponse {
    // Convert to internal types
    let portfolio = convert_portfolio_input(&request.portfolio);

    let analyzer = state.engine.portfolio_analyzer();

    match analyzer.calculate(&portfolio, &request.bond_prices) {
        Ok(output) => {
            // Publish to WebSocket subscribers
            state.ws_state.publish_portfolio_analytics(output.clone());
            (StatusCode::OK, Json(serde_json::to_value(output).unwrap()))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
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
    /// Successful calculations
    pub results: Vec<PortfolioAnalyticsOutput>,
    /// Errors
    pub errors: Vec<PortfolioAnalyticsError>,
}

/// Portfolio analytics error.
#[derive(Debug, Serialize)]
pub struct PortfolioAnalyticsError {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Error message
    pub error: String,
}

/// Batch calculate analytics for multiple portfolios.
pub async fn batch_calculate_portfolio_analytics(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BatchPortfolioAnalyticsRequest>,
) -> impl IntoResponse {
    // Convert all portfolios
    let portfolios: Vec<Portfolio> = request
        .portfolios
        .iter()
        .map(convert_portfolio_input)
        .collect();

    let analyzer = state.engine.portfolio_analyzer();
    let results = analyzer.calculate_batch(&portfolios, &request.bond_prices);

    let mut outputs = Vec::new();
    let mut errors = Vec::new();

    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(output) => {
                // Publish to WebSocket subscribers
                state.ws_state.publish_portfolio_analytics(output.clone());
                outputs.push(output);
            }
            Err(e) => errors.push(PortfolioAnalyticsError {
                portfolio_id: portfolios
                    .get(i)
                    .map(|p| p.portfolio_id.as_str().to_string())
                    .unwrap_or_else(|| format!("index_{}", i)),
                error: e.to_string(),
            }),
        }
    }

    let response = BatchPortfolioAnalyticsResponse {
        results: outputs,
        errors,
    };

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
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
) -> impl IntoResponse {
    let portfolio = convert_portfolio_input(&request.portfolio);

    let analyzer = state.engine.portfolio_analyzer();
    let contributions = analyzer.duration_contribution(&portfolio, &request.bond_prices);

    let mut total_duration = Decimal::ZERO;
    let entries: Vec<DurationContributionEntry> = contributions
        .into_iter()
        .map(|(id, weight, contrib)| {
            total_duration += contrib;
            DurationContributionEntry {
                instrument_id: id.as_str().to_string(),
                weight,
                contribution: contrib,
            }
        })
        .collect();

    let response = DurationContributionResponse {
        contributions: entries,
        total_duration,
    };

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
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
}

fn default_contribution_types() -> Vec<String> {
    vec!["duration".to_string(), "dv01".to_string(), "spread".to_string(), "cs01".to_string()]
}

/// Serializable holding contribution for API response.
#[derive(Debug, Serialize)]
pub struct ApiHoldingContribution {
    /// Holding identifier
    pub id: String,
    /// Market value weight (0-1)
    pub weight: f64,
    /// Absolute contribution value
    pub contribution: f64,
    /// Contribution as percentage of total (0-100)
    pub contribution_pct: f64,
}

/// Serializable bucket contribution for API response.
#[derive(Debug, Serialize)]
pub struct ApiBucketContribution {
    /// Bucket name
    pub name: String,
    /// Number of holdings in bucket
    pub count: usize,
    /// Total weight of holdings (0-1)
    pub weight: f64,
    /// Absolute contribution
    pub contribution: f64,
    /// Contribution percentage (0-100)
    pub contribution_pct: f64,
}

/// Duration contributions response.
#[derive(Debug, Serialize)]
pub struct ApiDurationContributions {
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
) -> impl IntoResponse {
    // Convert to holdings
    let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    if holdings.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "No valid holdings found (positions must have matching bond prices)" })),
        );
    }

    let config = AnalyticsConfig::default();
    let types: std::collections::HashSet<_> = request.contribution_types.iter()
        .map(|s| s.to_lowercase())
        .collect();

    // Calculate requested contribution types
    let duration = if types.contains("duration") || types.is_empty() {
        Some(convert_duration_contributions(&duration_contributions(&holdings, &config)))
    } else {
        None
    };

    let dv01 = if types.contains("dv01") || types.is_empty() {
        Some(convert_dv01_contributions(&dv01_contributions(&holdings, &config)))
    } else {
        None
    };

    let spread = if types.contains("spread") || types.is_empty() {
        Some(convert_spread_contributions(&spread_contributions(&holdings, &config)))
    } else {
        None
    };

    let cs01 = if types.contains("cs01") || types.is_empty() {
        Some(convert_cs01_contributions(&cs01_contributions(&holdings, &config)))
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

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
}

/// Convert portfolio positions and bond prices to convex-portfolio Holdings.
fn convert_to_holdings(
    portfolio: &PortfolioInput,
    bond_prices: &[BondQuoteOutput],
) -> Result<Vec<Holding>, String> {
    use std::collections::HashMap;

    // Build price lookup by instrument_id
    let price_map: HashMap<_, _> = bond_prices
        .iter()
        .map(|q| (q.instrument_id.as_str(), q))
        .collect();

    let mut holdings = Vec::new();

    for position in &portfolio.positions {
        if let Some(quote) = price_map.get(position.instrument_id.as_str()) {
            // Need dirty price for market value calculation
            let market_price = quote.dirty_price
                .or(quote.clean_price)
                .unwrap_or(Decimal::from(100));

            // Build analytics from quote
            let mut analytics = HoldingAnalytics::new();

            if let Some(dur) = quote.modified_duration {
                analytics.modified_duration = Some(dur.to_f64().unwrap_or(0.0));
            }
            if let Some(dur) = quote.effective_duration {
                analytics.effective_duration = Some(dur.to_f64().unwrap_or(0.0));
            }
            if let Some(dur) = quote.macaulay_duration {
                analytics.macaulay_duration = Some(dur.to_f64().unwrap_or(0.0));
            }
            if let Some(dur) = quote.spread_duration {
                analytics.spread_duration = Some(dur.to_f64().unwrap_or(0.0));
            }
            if let Some(conv) = quote.convexity {
                analytics.convexity = Some(conv.to_f64().unwrap_or(0.0));
            }
            if let Some(conv) = quote.effective_convexity {
                analytics.effective_convexity = Some(conv.to_f64().unwrap_or(0.0));
            }
            if let Some(dv01) = quote.dv01 {
                analytics.dv01 = Some(dv01.to_f64().unwrap_or(0.0));
            }
            if let Some(ytm) = quote.ytm {
                analytics.ytm = Some(ytm.to_f64().unwrap_or(0.0));
            }
            if let Some(ytw) = quote.ytw {
                analytics.ytw = Some(ytw.to_f64().unwrap_or(0.0));
            }
            if let Some(z) = quote.z_spread {
                analytics.z_spread = Some(z.to_f64().unwrap_or(0.0));
            }
            if let Some(o) = quote.oas {
                analytics.oas = Some(o.to_f64().unwrap_or(0.0));
            }
            if let Some(g) = quote.g_spread {
                analytics.g_spread = Some(g.to_f64().unwrap_or(0.0));
            }
            if let Some(i) = quote.i_spread {
                analytics.i_spread = Some(i.to_f64().unwrap_or(0.0));
            }
            if let Some(cs01) = quote.cs01 {
                analytics.cs01 = Some(cs01.to_f64().unwrap_or(0.0));
            }

            // Build classification from position metadata
            let mut classification = convex_portfolio::Classification::new();
            if let Some(ref sector_str) = position.sector {
                if let Some(sector) = parse_sector(sector_str) {
                    classification = classification.with_sector(
                        convex_portfolio::SectorInfo::from_composite(sector)
                    );
                }
            }
            if let Some(ref rating_str) = position.rating {
                if let Some(rating) = parse_rating(rating_str) {
                    classification = classification.with_rating(
                        convex_portfolio::RatingInfo::from_composite(rating)
                    );
                }
            }
            if let Some(ref country) = position.country {
                classification = classification.with_country(country);
            }
            if let Some(ref issuer) = position.issuer {
                classification = classification.with_issuer(issuer);
            }

            // Create identifiers - use ISIN if available, otherwise create from instrument_id
            let identifiers = quote.isin.as_ref()
                .and_then(|isin| BondIdentifiers::from_isin_str(isin).ok())
                .unwrap_or_else(|| BondIdentifiers::from_isin_str(&format!("XX{:010}", position.instrument_id.len())).unwrap_or_default());

            // Build holding
            let holding = HoldingBuilder::new()
                .id(&position.instrument_id)
                .identifiers(identifiers)
                .par_amount(position.notional)
                .market_price(market_price)
                .accrued_interest(quote.accrued_interest.unwrap_or(Decimal::ZERO))
                .currency(quote.currency)
                .analytics(analytics)
                .classification(classification)
                .build();

            match holding {
                Ok(h) => holdings.push(h),
                Err(e) => {
                    // Log but continue with other holdings
                    tracing::warn!("Failed to build holding for {}: {}", position.instrument_id, e);
                }
            }
        }
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
        _ => Some(Sector::Other),
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
fn convert_sector_buckets(buckets: &std::collections::HashMap<Sector, BucketContribution>) -> Vec<ApiBucketContribution> {
    buckets.iter()
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
fn convert_rating_buckets(buckets: &std::collections::HashMap<RatingBucket, BucketContribution>) -> Vec<ApiBucketContribution> {
    buckets.iter()
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
        by_holding: c.by_holding.iter().map(convert_holding_contribution).collect(),
        by_sector: convert_sector_buckets(&c.by_sector),
        by_rating: convert_rating_buckets(&c.by_rating),
        portfolio_duration: c.portfolio_duration,
        total_market_value: c.total_market_value.to_string(),
    }
}

/// Convert Dv01Contributions to API response type.
fn convert_dv01_contributions(c: &Dv01Contributions) -> ApiDv01Contributions {
    ApiDv01Contributions {
        by_holding: c.by_holding.iter().map(convert_holding_contribution).collect(),
        by_sector: convert_sector_buckets(&c.by_sector),
        by_rating: convert_rating_buckets(&c.by_rating),
        total_dv01: c.total_dv01.to_string(),
        total_market_value: c.total_market_value.to_string(),
    }
}

/// Convert SpreadContributions to API response type.
fn convert_spread_contributions(c: &SpreadContributions) -> ApiSpreadContributions {
    ApiSpreadContributions {
        by_holding: c.by_holding.iter().map(convert_holding_contribution).collect(),
        by_sector: convert_sector_buckets(&c.by_sector),
        by_rating: convert_rating_buckets(&c.by_rating),
        portfolio_spread: c.portfolio_spread,
        total_market_value: c.total_market_value.to_string(),
    }
}

/// Convert Cs01Contributions to API response type.
fn convert_cs01_contributions(c: &Cs01Contributions) -> ApiCs01Contributions {
    ApiCs01Contributions {
        by_holding: c.by_holding.iter().map(convert_holding_contribution).collect(),
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
) -> impl IntoResponse {
    // Convert to holdings
    let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    let config = AnalyticsConfig::default();
    let dist = bucket_by_sector(&holdings, &config);

    let by_sector: Vec<SectorBucketEntry> = dist.by_sector
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

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
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
) -> impl IntoResponse {
    // Convert to holdings
    let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    let config = AnalyticsConfig::default();
    let dist = bucket_by_rating(&holdings, &config);

    let by_rating: Vec<RatingBucketEntry> = dist.by_rating
        .iter()
        .map(|(rating, metrics)| RatingBucketEntry {
            rating: format!("{:?}", rating),
            metrics: convert_bucket_metrics(metrics),
        })
        .collect();

    let by_bucket: Vec<RatingBucketEntry> = dist.by_bucket
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

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
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
) -> impl IntoResponse {
    // Convert to holdings
    let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    let config = AnalyticsConfig::default();
    let dist = bucket_by_maturity(&holdings, &config);

    let by_bucket: Vec<MaturityBucketEntry> = dist.by_bucket
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

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
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
) -> impl IntoResponse {
    // Convert to holdings
    let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    let config = AnalyticsConfig::default();

    let dist = match request.bucket_type.to_lowercase().as_str() {
        "country" => bucket_by_country(&holdings, &config),
        "issuer" => bucket_by_issuer(&holdings, &config),
        "currency" => bucket_by_currency(&holdings, &config),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("Invalid bucket_type: {}. Valid options: country, issuer, currency", request.bucket_type)
                })),
            );
        }
    };

    let by_bucket: Vec<CustomBucketEntry> = dist.by_bucket
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

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
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
    vec!["sector".to_string(), "rating".to_string(), "maturity".to_string()]
}

/// Calculate all bucketing types for a portfolio.
pub async fn calculate_all_bucketing(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<CombinedBucketingRequest>,
) -> impl IntoResponse {
    // Convert to holdings
    let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    let config = AnalyticsConfig::default();
    let types: std::collections::HashSet<_> = request.bucket_types.iter()
        .map(|s| s.to_lowercase())
        .collect();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    // Calculate sector distribution
    let sector = if types.contains("sector") || types.is_empty() {
        let dist = bucket_by_sector(&holdings, &config);
        let by_sector: Vec<SectorBucketEntry> = dist.by_sector
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
        let dist = bucket_by_rating(&holdings, &config);
        let by_rating: Vec<RatingBucketEntry> = dist.by_rating
            .iter()
            .map(|(rating, metrics)| RatingBucketEntry {
                rating: format!("{:?}", rating),
                metrics: convert_bucket_metrics(metrics),
            })
            .collect();
        let by_bucket: Vec<RatingBucketEntry> = dist.by_bucket
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
        let dist = bucket_by_maturity(&holdings, &config);
        let by_bucket: Vec<MaturityBucketEntry> = dist.by_bucket
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

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
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
fn convert_portfolio_input(input: &PortfolioInput) -> Portfolio {
    let currency = match input.currency.to_uppercase().as_str() {
        "EUR" => Currency::EUR,
        "GBP" => Currency::GBP,
        "JPY" => Currency::JPY,
        "CHF" => Currency::CHF,
        "CAD" => Currency::CAD,
        "AUD" => Currency::AUD,
        _ => Currency::USD,
    };

    Portfolio {
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
    }
}

// =============================================================================
// BOND REFERENCE DATA CRUD
// =============================================================================

/// Query parameters for bond listing.
#[derive(Debug, Deserialize)]
pub struct BondListQuery {
    /// Maximum number of bonds to return
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination
    #[serde(default)]
    pub offset: usize,
    /// Currency filter
    pub currency: Option<String>,
    /// Issuer type filter
    pub issuer_type: Option<String>,
    /// Bond type filter
    pub bond_type: Option<String>,
    /// Country filter
    pub country: Option<String>,
    /// Sector filter
    pub sector: Option<String>,
    /// Issuer ID filter
    pub issuer_id: Option<String>,
    /// Text search query (searches description, issuer name, ISIN, CUSIP)
    pub q: Option<String>,
    /// Filter callable bonds
    pub is_callable: Option<bool>,
    /// Filter floating rate notes
    pub is_floating: Option<bool>,
    /// Filter inflation-linked bonds
    pub is_inflation_linked: Option<bool>,
}

fn default_limit() -> usize {
    100
}

/// Response for bond listing.
#[derive(Debug, Serialize)]
pub struct BondListResponse {
    /// Bonds
    pub bonds: Vec<BondReferenceData>,
    /// Total count (before pagination)
    pub total: u64,
    /// Limit used
    pub limit: usize,
    /// Offset used
    pub offset: usize,
}

/// List bonds with optional filtering and pagination.
pub async fn list_bonds(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<BondListQuery>,
) -> impl IntoResponse {
    use convex_traits::reference_data::{BondType, IssuerType};

    // Build filter from query params
    let filter = BondFilter {
        currency: query.currency.as_ref().and_then(|c| Currency::from_code(c)),
        issuer_type: query.issuer_type.as_ref().and_then(|t| match t.to_lowercase().as_str() {
            "sovereign" => Some(IssuerType::Sovereign),
            "agency" => Some(IssuerType::Agency),
            "supranational" => Some(IssuerType::Supranational),
            "corporateig" | "corporate_ig" => Some(IssuerType::CorporateIG),
            "corporatehy" | "corporate_hy" => Some(IssuerType::CorporateHY),
            "financial" => Some(IssuerType::Financial),
            "municipal" => Some(IssuerType::Municipal),
            _ => None,
        }),
        bond_type: query.bond_type.as_ref().and_then(|t| match t.to_lowercase().as_str() {
            "fixedbullet" | "fixed_bullet" => Some(BondType::FixedBullet),
            "fixedcallable" | "fixed_callable" => Some(BondType::FixedCallable),
            "fixedputable" | "fixed_putable" => Some(BondType::FixedPutable),
            "floatingrate" | "floating_rate" | "frn" => Some(BondType::FloatingRate),
            "zerocoupon" | "zero_coupon" => Some(BondType::ZeroCoupon),
            "inflationlinked" | "inflation_linked" | "linker" => Some(BondType::InflationLinked),
            "amortizing" => Some(BondType::Amortizing),
            "convertible" => Some(BondType::Convertible),
            _ => None,
        }),
        country: query.country.clone(),
        sector: query.sector.clone(),
        issuer_id: query.issuer_id.clone(),
        text_search: query.q.clone(),
        is_callable: query.is_callable,
        is_floating: query.is_floating,
        is_inflation_linked: query.is_inflation_linked,
        maturity_from: None,
        maturity_to: None,
    };

    // Get total count first
    let total = match state.bond_store.count(&filter).await {
        Ok(count) => count,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            );
        }
    };

    // Get bonds with pagination
    let bonds = match state.bond_store.search(&filter, query.limit, query.offset).await {
        Ok(bonds) => bonds,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            );
        }
    };

    let response = BondListResponse {
        bonds,
        total,
        limit: query.limit,
        offset: query.offset,
    };

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
}

/// Get a single bond by instrument ID.
pub async fn get_bond(
    State(state): State<Arc<AppState>>,
    Path(instrument_id): Path<String>,
) -> impl IntoResponse {
    let id = InstrumentId::new(&instrument_id);

    match state.bond_store.get_by_id(&id).await {
        Ok(Some(bond)) => (StatusCode::OK, Json(serde_json::to_value(bond).unwrap())),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Bond not found: {}", instrument_id) })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// Create a new bond.
pub async fn create_bond(
    State(state): State<Arc<AppState>>,
    Json(bond): Json<BondReferenceData>,
) -> impl IntoResponse {
    // Check if bond already exists
    if let Ok(Some(_)) = state.bond_store.get_by_id(&bond.instrument_id).await {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("Bond already exists: {}", bond.instrument_id.as_str())
            })),
        );
    }

    // Set timestamp if not provided
    let mut bond = bond;
    if bond.last_updated == 0 {
        bond.last_updated = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
    }

    let created = state.bond_store.upsert(bond);

    (StatusCode::CREATED, Json(serde_json::to_value(created).unwrap()))
}

/// Update an existing bond.
pub async fn update_bond(
    State(state): State<Arc<AppState>>,
    Path(instrument_id): Path<String>,
    Json(mut bond): Json<BondReferenceData>,
) -> impl IntoResponse {
    let id = InstrumentId::new(&instrument_id);

    // Verify bond exists
    match state.bond_store.get_by_id(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("Bond not found: {}", instrument_id) })),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            );
        }
    }

    // Ensure instrument ID matches path
    bond.instrument_id = id;

    // Update timestamp
    bond.last_updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let updated = state.bond_store.upsert(bond);

    (StatusCode::OK, Json(serde_json::to_value(updated).unwrap()))
}

/// Delete a bond.
pub async fn delete_bond(
    State(state): State<Arc<AppState>>,
    Path(instrument_id): Path<String>,
) -> impl IntoResponse {
    let id = InstrumentId::new(&instrument_id);

    match state.bond_store.delete(&id) {
        Some(_) => (StatusCode::NO_CONTENT, Json(serde_json::json!({}))),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Bond not found: {}", instrument_id) })),
        ),
    }
}

/// Batch create bonds.
#[derive(Debug, Deserialize)]
pub struct BatchBondCreateRequest {
    /// Bonds to create
    pub bonds: Vec<BondReferenceData>,
}

/// Batch create response.
#[derive(Debug, Serialize)]
pub struct BatchBondCreateResponse {
    /// Number of bonds created
    pub created: usize,
    /// Number of bonds that already existed (skipped)
    pub skipped: usize,
    /// Errors encountered
    pub errors: Vec<String>,
}

/// Batch create bonds.
pub async fn batch_create_bonds(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BatchBondCreateRequest>,
) -> impl IntoResponse {
    let mut created = 0;
    let mut skipped = 0;
    let errors: Vec<String> = Vec::new();

    for mut bond in request.bonds {
        // Check if bond already exists
        if let Ok(Some(_)) = state.bond_store.get_by_id(&bond.instrument_id).await {
            skipped += 1;
            continue;
        }

        // Set timestamp if not provided
        if bond.last_updated == 0 {
            bond.last_updated = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
        }

        state.bond_store.upsert(bond);
        created += 1;
    }

    let response = BatchBondCreateResponse {
        created,
        skipped,
        errors,
    };

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
}

/// Get bond by ISIN.
pub async fn get_bond_by_isin(
    State(state): State<Arc<AppState>>,
    Path(isin): Path<String>,
) -> impl IntoResponse {
    match state.bond_store.get_by_isin(&isin).await {
        Ok(Some(bond)) => (StatusCode::OK, Json(serde_json::to_value(bond).unwrap())),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Bond not found with ISIN: {}", isin) })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// Get bond by CUSIP.
pub async fn get_bond_by_cusip(
    State(state): State<Arc<AppState>>,
    Path(cusip): Path<String>,
) -> impl IntoResponse {
    match state.bond_store.get_by_cusip(&cusip).await {
        Ok(Some(bond)) => (StatusCode::OK, Json(serde_json::to_value(bond).unwrap())),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Bond not found with CUSIP: {}", cusip) })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
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
    let portfolios = state.portfolio_store.list(&filter, query.limit, query.offset);

    let response = PortfolioListResponse {
        portfolios,
        total,
        limit: query.limit,
        offset: query.offset,
    };

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
}

/// Get a single portfolio by ID.
pub async fn get_portfolio(
    State(state): State<Arc<AppState>>,
    Path(portfolio_id): Path<String>,
) -> impl IntoResponse {
    match state.portfolio_store.get(&portfolio_id) {
        Some(portfolio) => (StatusCode::OK, Json(serde_json::to_value(portfolio).unwrap())),
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

    (StatusCode::CREATED, Json(serde_json::to_value(created).unwrap()))
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
                Json(serde_json::json!({ "error": format!("Portfolio not found: {}", portfolio_id) })),
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
        Some(portfolio) => (StatusCode::OK, Json(serde_json::to_value(portfolio).unwrap())),
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
    match state.portfolio_store.remove_position(&portfolio_id, &instrument_id) {
        Some(portfolio) => (StatusCode::OK, Json(serde_json::to_value(portfolio).unwrap())),
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

    match state.portfolio_store.update_position(&portfolio_id, position) {
        Some(portfolio) => (StatusCode::OK, Json(serde_json::to_value(portfolio).unwrap())),
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

    let response = BatchPortfolioCreateResponse {
        created,
        skipped,
    };

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
}

// =============================================================================
// STRESS TESTING
// =============================================================================

/// Request for stress testing with custom scenarios.
#[derive(Debug, Deserialize)]
pub struct StressTestRequest {
    /// Portfolio definition
    pub portfolio: PortfolioInput,
    /// Bond prices with analytics
    pub bond_prices: Vec<BondQuoteOutput>,
    /// Custom scenarios to run (optional - if not provided, uses standard scenarios)
    pub scenarios: Option<Vec<ScenarioInput>>,
}

/// Input for a custom stress scenario.
#[derive(Debug, Deserialize)]
pub struct ScenarioInput {
    /// Scenario name
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// Rate scenario (optional)
    pub rate_scenario: Option<RateScenarioInput>,
    /// Spread scenario (optional)
    pub spread_scenario: Option<SpreadScenarioInput>,
}

/// Input for rate scenarios.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum RateScenarioInput {
    /// Parallel shift across all tenors
    #[serde(rename = "parallel")]
    Parallel {
        /// Shift in basis points
        shift_bps: f64,
    },
    /// Key rate shifts at specific tenors
    #[serde(rename = "key_rates")]
    KeyRates {
        /// Shifts at various tenors [(tenor, shift_bps), ...]
        shifts: Vec<TenorShiftInput>,
    },
    /// Curve steepening
    #[serde(rename = "steepening")]
    Steepening {
        /// Short end shift (bps)
        short_shift: f64,
        /// Long end shift (bps)
        long_shift: f64,
        /// Pivot tenor in years (default 5)
        pivot_tenor: Option<f64>,
    },
    /// Curve flattening
    #[serde(rename = "flattening")]
    Flattening {
        /// Short end shift (bps)
        short_shift: f64,
        /// Long end shift (bps)
        long_shift: f64,
        /// Pivot tenor in years (default 5)
        pivot_tenor: Option<f64>,
    },
    /// Butterfly shift
    #[serde(rename = "butterfly")]
    Butterfly {
        /// Wing shift (short and long end, bps)
        wing_shift: f64,
        /// Belly shift (intermediate, bps)
        belly_shift: f64,
    },
}

/// Input for tenor shift.
#[derive(Debug, Deserialize)]
pub struct TenorShiftInput {
    /// Tenor in years
    pub tenor: f64,
    /// Shift in basis points
    pub shift_bps: f64,
}

/// Input for spread scenarios.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum SpreadScenarioInput {
    /// Uniform spread shock
    #[serde(rename = "uniform")]
    Uniform {
        /// Shift in basis points
        shift_bps: f64,
    },
    /// Spread shock by rating
    #[serde(rename = "by_rating")]
    ByRating {
        /// Map of rating -> shift_bps
        shifts: std::collections::HashMap<String, f64>,
    },
    /// Spread shock by sector
    #[serde(rename = "by_sector")]
    BySector {
        /// Map of sector -> shift_bps
        shifts: std::collections::HashMap<String, f64>,
    },
}

/// Response for stress test results.
#[derive(Debug, Serialize)]
pub struct StressTestResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Number of holdings
    pub num_holdings: usize,
    /// Individual scenario results
    pub results: Vec<StressResultOutput>,
    /// Summary statistics
    pub summary: Option<StressSummaryOutput>,
    /// Timestamp
    pub timestamp: i64,
}

/// Output for a single stress result.
#[derive(Debug, Serialize)]
pub struct StressResultOutput {
    /// Scenario name
    pub scenario_name: String,
    /// Initial portfolio value
    pub initial_value: String,
    /// Stressed portfolio value
    pub stressed_value: String,
    /// Profit/Loss
    pub pnl: String,
    /// P&L as percentage
    pub pnl_pct: f64,
    /// Rate impact component (percentage)
    pub rate_impact: Option<f64>,
    /// Spread impact component (percentage)
    pub spread_impact: Option<f64>,
    /// Is this a gain?
    pub is_gain: bool,
}

/// Output for stress summary.
#[derive(Debug, Serialize)]
pub struct StressSummaryOutput {
    /// Number of scenarios
    pub scenario_count: usize,
    /// Worst P&L
    pub worst_pnl: String,
    /// Worst P&L percentage
    pub worst_pnl_pct: f64,
    /// Worst scenario name
    pub worst_scenario: String,
    /// Best P&L
    pub best_pnl: String,
    /// Best P&L percentage
    pub best_pnl_pct: f64,
    /// Best scenario name
    pub best_scenario: String,
    /// Average P&L
    pub avg_pnl: String,
    /// Average P&L percentage
    pub avg_pnl_pct: f64,
}

/// Run stress tests on a portfolio.
///
/// If no scenarios are provided, runs all standard scenarios.
pub async fn run_stress_test(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<StressTestRequest>,
) -> impl IntoResponse {
    // Convert to holdings
    let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    // Build portfolio
    let portfolio = match build_convex_portfolio(&request.portfolio, holdings) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    let config = AnalyticsConfig::default();

    // Convert or use standard scenarios
    let scenarios: Vec<StressScenario> = match &request.scenarios {
        Some(inputs) => inputs.iter().map(convert_scenario_input).collect(),
        None => stress_scenarios::all(),
    };

    // Run stress tests
    let results = run_stress_scenarios(&portfolio, &scenarios, &config);
    let summary = summarize_results(&results);

    // Convert to output
    let result_outputs: Vec<StressResultOutput> = results.iter().map(convert_stress_result).collect();
    let summary_output = summary.map(|s| StressSummaryOutput {
        scenario_count: s.scenario_count,
        worst_pnl: format!("{:.2}", s.worst_pnl),
        worst_pnl_pct: s.worst_pnl_pct,
        worst_scenario: s.worst_scenario,
        best_pnl: format!("{:.2}", s.best_pnl),
        best_pnl_pct: s.best_pnl_pct,
        best_scenario: s.best_scenario,
        avg_pnl: format!("{:.2}", s.avg_pnl),
        avg_pnl_pct: s.avg_pnl_pct,
    });

    let response = StressTestResponse {
        portfolio_id: request.portfolio.portfolio_id.clone(),
        num_holdings: portfolio.holding_count(),
        results: result_outputs,
        summary: summary_output,
        timestamp: chrono::Utc::now().timestamp(),
    };

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
}

/// Run standard stress scenarios only.
pub async fn run_standard_stress_test(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<BucketingRequest>, // Reuse the same request type
) -> impl IntoResponse {
    // Convert to holdings
    let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    // Build portfolio
    let portfolio = match build_convex_portfolio(&request.portfolio, holdings) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    let config = AnalyticsConfig::default();

    // Run all standard scenarios
    let scenarios = stress_scenarios::all();
    let results = run_stress_scenarios(&portfolio, &scenarios, &config);
    let summary = summarize_results(&results);

    // Convert to output
    let result_outputs: Vec<StressResultOutput> = results.iter().map(convert_stress_result).collect();
    let summary_output = summary.map(|s| StressSummaryOutput {
        scenario_count: s.scenario_count,
        worst_pnl: format!("{:.2}", s.worst_pnl),
        worst_pnl_pct: s.worst_pnl_pct,
        worst_scenario: s.worst_scenario,
        best_pnl: format!("{:.2}", s.best_pnl),
        best_pnl_pct: s.best_pnl_pct,
        best_scenario: s.best_scenario,
        avg_pnl: format!("{:.2}", s.avg_pnl),
        avg_pnl_pct: s.avg_pnl_pct,
    });

    let response = StressTestResponse {
        portfolio_id: request.portfolio.portfolio_id.clone(),
        num_holdings: portfolio.holding_count(),
        results: result_outputs,
        summary: summary_output,
        timestamp: chrono::Utc::now().timestamp(),
    };

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
}

/// Request for a single scenario stress test.
#[derive(Debug, Deserialize)]
pub struct SingleStressTestRequest {
    /// Portfolio definition
    pub portfolio: PortfolioInput,
    /// Bond prices with analytics
    pub bond_prices: Vec<BondQuoteOutput>,
    /// Scenario to run
    pub scenario: ScenarioInput,
}

/// Run a single custom stress scenario.
pub async fn run_single_stress_test(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<SingleStressTestRequest>,
) -> impl IntoResponse {
    // Convert to holdings
    let holdings = match convert_to_holdings(&request.portfolio, &request.bond_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    // Build portfolio
    let portfolio = match build_convex_portfolio(&request.portfolio, holdings) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e })),
            );
        }
    };

    let config = AnalyticsConfig::default();

    // Convert scenario
    let scenario = convert_scenario_input(&request.scenario);

    // Run stress test
    let result = run_stress_scenario(&portfolio, &scenario, &config);
    let output = convert_stress_result(&result);

    let response = serde_json::json!({
        "portfolio_id": request.portfolio.portfolio_id,
        "num_holdings": portfolio.holding_count(),
        "result": output,
        "timestamp": chrono::Utc::now().timestamp()
    });

    (StatusCode::OK, Json(response))
}

/// List available standard scenarios.
pub async fn list_standard_scenarios() -> impl IntoResponse {
    let scenarios = stress_scenarios::all();

    let output: Vec<serde_json::Value> = scenarios
        .iter()
        .map(|s| {
            let mut obj = serde_json::json!({
                "name": s.name,
                "description": s.description,
                "has_rate_scenario": s.has_rate_scenario(),
                "has_spread_scenario": s.has_spread_scenario()
            });

            // Add rate scenario details
            if let Some(ref rs) = s.rate_scenario {
                obj["rate_scenario_type"] = serde_json::json!(rs.name());
            }

            // Add spread scenario details
            if let Some(ref ss) = s.spread_scenario {
                obj["spread_scenario_type"] = serde_json::json!(ss.name());
            }

            obj
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!({
        "scenarios": output,
        "count": output.len()
    })))
}

/// Build a ConvexPortfolio from holdings and input.
fn build_convex_portfolio(
    input: &PortfolioInput,
    holdings: Vec<Holding>,
) -> Result<ConvexPortfolio, String> {
    let as_of_date = Date::from_ymd(2025, 1, 15)
        .map_err(|e| format!("Failed to create date: {}", e))?;

    let mut builder = PortfolioBuilder::new()
        .id(&input.portfolio_id)
        .name(&input.name)
        .as_of_date(as_of_date);

    for holding in holdings {
        builder = builder.add_holding(holding);
    }

    builder.build().map_err(|e| e.to_string())
}

/// Convert ScenarioInput to StressScenario.
fn convert_scenario_input(input: &ScenarioInput) -> StressScenario {
    let mut scenario = StressScenario::new(&input.name);

    if let Some(ref desc) = input.description {
        scenario = scenario.with_description(desc);
    }

    if let Some(ref rs) = input.rate_scenario {
        scenario = scenario.with_rate_scenario(convert_rate_scenario(rs));
    }

    if let Some(ref ss) = input.spread_scenario {
        scenario = scenario.with_spread_scenario(convert_spread_scenario(ss));
    }

    scenario
}

/// Convert RateScenarioInput to RateScenario.
fn convert_rate_scenario(input: &RateScenarioInput) -> RateScenario {
    match input {
        RateScenarioInput::Parallel { shift_bps } => RateScenario::ParallelShift(*shift_bps),
        RateScenarioInput::KeyRates { shifts } => {
            let tenor_shifts: Vec<(f64, f64)> = shifts
                .iter()
                .map(|ts| (ts.tenor, ts.shift_bps))
                .collect();
            RateScenario::key_rates(&tenor_shifts)
        }
        RateScenarioInput::Steepening {
            short_shift,
            long_shift,
            pivot_tenor,
        } => RateScenario::Steepening {
            short_shift: *short_shift,
            long_shift: *long_shift,
            pivot_tenor: pivot_tenor.unwrap_or(5.0),
        },
        RateScenarioInput::Flattening {
            short_shift,
            long_shift,
            pivot_tenor,
        } => RateScenario::Flattening {
            short_shift: *short_shift,
            long_shift: *long_shift,
            pivot_tenor: pivot_tenor.unwrap_or(5.0),
        },
        RateScenarioInput::Butterfly {
            wing_shift,
            belly_shift,
        } => RateScenario::butterfly(*wing_shift, *belly_shift),
    }
}

/// Convert SpreadScenarioInput to SpreadScenario.
fn convert_spread_scenario(input: &SpreadScenarioInput) -> SpreadScenario {
    match input {
        SpreadScenarioInput::Uniform { shift_bps } => SpreadScenario::Uniform(*shift_bps),
        SpreadScenarioInput::ByRating { shifts } => SpreadScenario::ByRating(shifts.clone()),
        SpreadScenarioInput::BySector { shifts } => SpreadScenario::BySector(shifts.clone()),
    }
}

/// Convert StressResult to output format.
fn convert_stress_result(result: &StressResult) -> StressResultOutput {
    StressResultOutput {
        scenario_name: result.scenario_name.clone(),
        initial_value: format!("{:.2}", result.initial_value),
        stressed_value: format!("{:.2}", result.stressed_value),
        pnl: format!("{:.2}", result.pnl),
        pnl_pct: result.pnl_pct,
        rate_impact: result.rate_impact,
        spread_impact: result.spread_impact,
        is_gain: result.is_gain(),
    }
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
    pub portfolio_dv01: f64,
    /// Benchmark DV01
    pub benchmark_dv01: f64,
    /// DV01 difference
    pub dv01_difference: f64,
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
) -> impl IntoResponse {
    // Convert portfolio holdings
    let port_holdings = match convert_to_holdings(&request.portfolio, &request.portfolio_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Portfolio error: {}", e) })),
            );
        }
    };

    // Convert benchmark holdings
    let bench_holdings = match convert_to_holdings(&request.benchmark, &request.benchmark_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Benchmark error: {}", e) })),
            );
        }
    };

    let config = AnalyticsConfig::default();

    // Perform comparison
    let comparison = benchmark_comparison(&port_holdings, &bench_holdings, &config);
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
        ytm_difference: comparison.yield_comparison.ytm_difference.map(|y| y * 100.0),
        is_higher_yield: comparison.yield_comparison.is_higher_yield(),
    };

    // Convert risk comparison
    let risk = RiskComparisonOutput {
        portfolio_dv01: comparison.risk.portfolio_dv01,
        benchmark_dv01: comparison.risk.benchmark_dv01,
        dv01_difference: comparison.risk.dv01_difference,
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

    (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
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
) -> impl IntoResponse {
    // Convert portfolio holdings
    let port_holdings = match convert_to_holdings(&request.portfolio, &request.portfolio_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Portfolio error: {}", e) })),
            );
        }
    };

    // Convert benchmark holdings
    let bench_holdings = match convert_to_holdings(&request.benchmark, &request.benchmark_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Benchmark error: {}", e) })),
            );
        }
    };

    let config = AnalyticsConfig::default();
    let weights = active_weights(&port_holdings, &bench_holdings, &config);

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
) -> impl IntoResponse {
    // Convert portfolio holdings
    let port_holdings = match convert_to_holdings(&request.portfolio, &request.portfolio_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Portfolio error: {}", e) })),
            );
        }
    };

    // Convert benchmark holdings
    let bench_holdings = match convert_to_holdings(&request.benchmark, &request.benchmark_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Benchmark error: {}", e) })),
            );
        }
    };

    let config = AnalyticsConfig::default();

    // Default volatility assumptions
    let rate_vol = request.rate_vol.unwrap_or(0.01);   // 100bp
    let spread_vol = request.spread_vol.unwrap_or(0.002); // 20bp

    let te = estimate_tracking_error(&port_holdings, &bench_holdings, &config, rate_vol, spread_vol);

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
) -> impl IntoResponse {
    // Convert portfolio holdings
    let port_holdings = match convert_to_holdings(&request.portfolio, &request.portfolio_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Portfolio error: {}", e) })),
            );
        }
    };

    // Convert benchmark holdings
    let bench_holdings = match convert_to_holdings(&request.benchmark, &request.benchmark_prices) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Benchmark error: {}", e) })),
            );
        }
    };

    let config = AnalyticsConfig::default();

    // Calculate duration difference by sector
    let dur_diff = duration_difference_by_sector(&port_holdings, &bench_holdings, &config);

    // Calculate spread difference by sector
    let spread_diff = spread_difference_by_sector(&port_holdings, &bench_holdings, &config);

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
) -> impl IntoResponse {
    let holdings = convert_liquidity_positions(&request.positions);
    let config = AnalyticsConfig::default();

    let metrics = calculate_liquidity_metrics(&holdings, &config);

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
) -> impl IntoResponse {
    let holdings = convert_liquidity_positions(&request.positions);
    let config = AnalyticsConfig::default();

    let dist = liquidity_distribution(&holdings, &config);

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
}

/// Request for days to liquidate estimation.
#[derive(Debug, Deserialize)]
pub struct DaysToLiquidateRequest {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Portfolio name
    pub name: String,
    /// Positions with liquidity data
    pub positions: Vec<LiquidityPosition>,
    /// Maximum participation rate (percentage of ADV, e.g., 20.0 for 20%)
    #[serde(default = "default_participation_rate")]
    pub max_participation_rate: f64,
}

fn default_participation_rate() -> f64 {
    20.0
}

/// Response for days to liquidate.
#[derive(Debug, Serialize)]
pub struct DaysToLiquidateResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Total estimated days to liquidate entire portfolio
    pub total_days: f64,
    /// Days attributed to highly liquid holdings
    pub highly_liquid_days: f64,
    /// Days attributed to illiquid holdings
    pub illiquid_days: f64,
    /// Number of holdings without ADV data
    pub holdings_without_adv: usize,
    /// Percentage of time spent on illiquid holdings
    pub illiquid_pct_of_time: f64,
    /// Maximum participation rate used
    pub max_participation_rate: f64,
    /// Calculation timestamp
    pub timestamp: i64,
}

/// Estimate days to liquidate a portfolio.
pub async fn calculate_days_to_liquidate(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<DaysToLiquidateRequest>,
) -> impl IntoResponse {
    let holdings = convert_liquidity_positions(&request.positions);
    let config = AnalyticsConfig::default();

    let dtl = estimate_days_to_liquidate(&holdings, request.max_participation_rate, &config);

    let response = DaysToLiquidateResponse {
        portfolio_id: request.portfolio_id,
        total_days: dtl.total_days,
        highly_liquid_days: dtl.highly_liquid_days,
        illiquid_days: dtl.illiquid_days,
        holdings_without_adv: dtl.holdings_without_adv,
        illiquid_pct_of_time: dtl.illiquid_pct_of_time(),
        max_participation_rate: request.max_participation_rate,
        timestamp: chrono::Utc::now().timestamp(),
    };

    (StatusCode::OK, Json(response))
}

/// Request for comprehensive liquidity analysis.
#[derive(Debug, Deserialize)]
pub struct LiquidityAnalysisRequest {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Portfolio name
    pub name: String,
    /// Positions with liquidity data
    pub positions: Vec<LiquidityPosition>,
    /// Maximum participation rate for liquidation estimate
    #[serde(default = "default_participation_rate")]
    pub max_participation_rate: f64,
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
    /// Days to liquidate estimate
    pub days_to_liquidate: DaysToLiquidateResponse,
    /// Calculation timestamp
    pub timestamp: i64,
}

/// Calculate comprehensive liquidity analysis.
pub async fn calculate_liquidity_analysis(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<LiquidityAnalysisRequest>,
) -> impl IntoResponse {
    let holdings = convert_liquidity_positions(&request.positions);
    let config = AnalyticsConfig::default();
    let timestamp = chrono::Utc::now().timestamp();

    // Calculate metrics
    let metrics_result = calculate_liquidity_metrics(&holdings, &config);
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
    let dist = liquidity_distribution(&holdings, &config);
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

    // Calculate days to liquidate
    let dtl = estimate_days_to_liquidate(&holdings, request.max_participation_rate, &config);
    let days_to_liquidate = DaysToLiquidateResponse {
        portfolio_id: request.portfolio_id.clone(),
        total_days: dtl.total_days,
        highly_liquid_days: dtl.highly_liquid_days,
        illiquid_days: dtl.illiquid_days,
        holdings_without_adv: dtl.holdings_without_adv,
        illiquid_pct_of_time: dtl.illiquid_pct_of_time(),
        max_participation_rate: request.max_participation_rate,
        timestamp,
    };

    let response = LiquidityAnalysisResponse {
        portfolio_id: request.portfolio_id,
        metrics,
        distribution,
        days_to_liquidate,
        timestamp,
    };

    (StatusCode::OK, Json(response))
}

/// Convert liquidity positions to holdings.
fn convert_liquidity_positions(positions: &[LiquidityPosition]) -> Vec<Holding> {
    // Use a valid placeholder ISIN for liquidity analytics
    let placeholder_identifiers = BondIdentifiers::from_isin_str("US912828Z229")
        .expect("Valid placeholder ISIN");

    positions
        .iter()
        .filter_map(|pos| {
            let market_price = pos.market_price.unwrap_or(Decimal::from(100));

            let mut analytics = HoldingAnalytics::new();
            analytics.liquidity_score = pos.liquidity_score;
            analytics.bid_ask_spread = pos.bid_ask_spread;

            HoldingBuilder::new()
                .id(&pos.instrument_id)
                .identifiers(placeholder_identifiers.clone())
                .par_amount(pos.notional)
                .market_price(market_price)
                .analytics(analytics)
                .build()
                .ok()
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
) -> impl IntoResponse {
    let holdings = convert_credit_positions(&request.positions);
    let config = AnalyticsConfig::default();

    let metrics = calculate_credit_quality(&holdings, &config);

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
) -> impl IntoResponse {
    let holdings = convert_credit_positions(&request.positions);
    let config = AnalyticsConfig::default();

    let risk = calculate_migration_risk(&holdings, &config);

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
) -> impl IntoResponse {
    let holdings = convert_credit_positions(&request.positions);
    let config = AnalyticsConfig::default();
    let timestamp = chrono::Utc::now().timestamp();

    // Calculate quality metrics
    let metrics = calculate_credit_quality(&holdings, &config);
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
    let risk = calculate_migration_risk(&holdings, &config);
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
}

/// Convert credit positions to holdings.
fn convert_credit_positions(positions: &[CreditPosition]) -> Vec<Holding> {
    let placeholder_identifiers =
        BondIdentifiers::from_isin_str("US912828Z229").expect("Valid placeholder ISIN");

    positions
        .iter()
        .filter_map(|pos| {
            let market_price = pos.market_price.unwrap_or(Decimal::from(100));

            // Parse rating string to CreditRating
            let classification = if let Some(ref rating_str) = pos.rating {
                let credit_rating = CreditRating::parse(rating_str);
                if let Some(rating) = credit_rating {
                    Classification::new().with_rating(RatingInfo::from_composite(rating))
                } else {
                    Classification::new()
                }
            } else {
                Classification::new()
            };

            HoldingBuilder::new()
                .id(&pos.instrument_id)
                .identifiers(placeholder_identifiers.clone())
                .par_amount(pos.notional)
                .market_price(market_price)
                .classification(classification)
                .build()
                .ok()
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
) -> impl IntoResponse {
    let holdings = convert_basket_positions(&request.holdings);

    let basket = build_creation_basket(
        &holdings,
        request.creation_unit_size,
        request.total_shares,
        request.cash_balance,
    );

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
) -> impl IntoResponse {
    let basket_holdings = convert_basket_positions(&request.basket_holdings);
    let target_holdings = convert_basket_positions(&request.target_holdings);
    let config = AnalyticsConfig::default();

    // Build the basket first
    let basket = build_creation_basket(
        &basket_holdings,
        request.creation_unit_size,
        request.total_shares,
        request.cash_balance,
    );

    // Analyze against target
    let analysis = analyze_basket(&basket, &target_holdings, &config);

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
}

/// Convert basket positions to holdings.
fn convert_basket_positions(positions: &[BasketPosition]) -> Vec<Holding> {
    let placeholder_identifiers =
        BondIdentifiers::from_isin_str("US912828Z229").expect("Valid placeholder ISIN");

    positions
        .iter()
        .filter_map(|pos| {
            HoldingBuilder::new()
                .id(&pos.instrument_id)
                .identifiers(placeholder_identifiers.clone())
                .par_amount(pos.notional)
                .market_price(pos.market_price)
                .build()
                .ok()
        })
        .collect()
}

// =============================================================================
// KEY RATE DURATION ANALYTICS
// =============================================================================

/// Position with key rate duration data.
#[derive(Debug, Deserialize)]
pub struct KeyRatePosition {
    /// Instrument ID
    pub instrument_id: String,
    /// Notional/par amount
    pub notional: Decimal,
    /// Market price
    pub market_price: Option<Decimal>,
    /// Key rate durations: array of [tenor, duration] pairs
    pub key_rate_durations: Option<Vec<(f64, f64)>>,
}

/// Request for key rate duration profile.
#[derive(Debug, Deserialize)]
pub struct KeyRateDurationRequest {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Portfolio name
    pub name: String,
    /// Positions with KRD data
    pub positions: Vec<KeyRatePosition>,
    /// Custom tenor points (optional, defaults to standard tenors)
    pub tenors: Option<Vec<f64>>,
}

/// Key rate point output.
#[derive(Debug, Serialize)]
pub struct KeyRatePointOutput {
    /// Tenor in years
    pub tenor: f64,
    /// Portfolio duration at this tenor
    pub duration: f64,
    /// Contribution to total duration (%)
    pub contribution_pct: f64,
}

/// Response for key rate duration profile.
#[derive(Debug, Serialize)]
pub struct KeyRateDurationResponse {
    /// Portfolio ID
    pub portfolio_id: String,
    /// Key rate duration profile
    pub profile: Vec<KeyRatePointOutput>,
    /// Total duration (sum of key rate durations)
    pub total_duration: f64,
    /// Short-end duration (< 2 years)
    pub short_duration: f64,
    /// Intermediate duration (2-10 years)
    pub intermediate_duration: f64,
    /// Long-end duration (> 10 years)
    pub long_duration: f64,
    /// Number of holdings with KRD data
    pub coverage: usize,
    /// Total holdings
    pub total_holdings: usize,
    /// Coverage percentage
    pub coverage_pct: f64,
    /// Calculation timestamp
    pub timestamp: i64,
}

/// Calculate key rate duration profile.
pub async fn calculate_key_rate_duration_handler(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<KeyRateDurationRequest>,
) -> impl IntoResponse {
    let holdings = convert_key_rate_positions(&request.positions);
    let config = AnalyticsConfig::default();

    let tenors: Option<&[f64]> = request.tenors.as_deref();

    match aggregate_key_rate_profile(&holdings, &config, tenors) {
        Some(profile) => {
            let total_duration = profile.total_duration;

            let key_rate_points: Vec<KeyRatePointOutput> = profile
                .durations
                .iter()
                .map(|krd| {
                    let krd_duration = krd.duration.as_f64();
                    let contribution_pct = if total_duration > 0.0 {
                        krd_duration / total_duration * 100.0
                    } else {
                        0.0
                    };
                    KeyRatePointOutput {
                        tenor: krd.tenor,
                        duration: krd_duration,
                        contribution_pct,
                    }
                })
                .collect();

            let response = KeyRateDurationResponse {
                portfolio_id: request.portfolio_id,
                profile: key_rate_points,
                total_duration,
                short_duration: profile.short_duration(),
                intermediate_duration: profile.intermediate_duration(),
                long_duration: profile.long_duration(),
                coverage: profile.coverage,
                total_holdings: profile.total_holdings,
                coverage_pct: profile.coverage_pct(),
                timestamp: chrono::Utc::now().timestamp(),
            };

            (StatusCode::OK, Json(response)).into_response()
        }
        None => {
            // No KRD data available
            let response = KeyRateDurationResponse {
                portfolio_id: request.portfolio_id,
                profile: vec![],
                total_duration: 0.0,
                short_duration: 0.0,
                intermediate_duration: 0.0,
                long_duration: 0.0,
                coverage: 0,
                total_holdings: request.positions.len(),
                coverage_pct: 0.0,
                timestamp: chrono::Utc::now().timestamp(),
            };

            (StatusCode::OK, Json(response)).into_response()
        }
    }
}

/// Convert key rate positions to holdings with KRD data.
fn convert_key_rate_positions(positions: &[KeyRatePosition]) -> Vec<Holding> {
    let placeholder_identifiers =
        BondIdentifiers::from_isin_str("US912828Z229").expect("Valid placeholder ISIN");

    positions
        .iter()
        .filter_map(|pos| {
            let market_price = pos.market_price.unwrap_or(Decimal::from(100));

            let mut analytics = HoldingAnalytics::new();
            if let Some(ref krd_data) = pos.key_rate_durations {
                let krd = KeyRateDurations::new(
                    krd_data
                        .iter()
                        .map(|(tenor, dur)| KeyRateDuration {
                            tenor: *tenor,
                            duration: AnalyticsDuration::from(*dur),
                        })
                        .collect(),
                );
                analytics.key_rate_durations = Some(krd);
            }

            HoldingBuilder::new()
                .id(&pos.instrument_id)
                .identifiers(placeholder_identifiers.clone())
                .par_amount(pos.notional)
                .market_price(market_price)
                .analytics(analytics)
                .build()
                .ok()
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

    Date::from_ymd(year, month, day)
        .map_err(|e| format!("Invalid date: {}", e))
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
