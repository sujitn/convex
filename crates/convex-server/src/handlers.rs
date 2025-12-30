//! Request handlers.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Datelike;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_core::Date;
use convex_core::Currency;
use convex_engine::{Portfolio, Position, PricingEngine};
use convex_ext_file::{InMemoryBondStore, InMemoryPortfolioStore, PortfolioFilter, StoredPortfolio, StoredPosition};

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

fn default_currency() -> String {
    "USD".to_string()
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
