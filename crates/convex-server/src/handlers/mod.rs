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
use convex_ext_file::{
    InMemoryBondStore, InMemoryPortfolioStore, PortfolioFilter, StoredPortfolio, StoredPosition,
};
use convex_portfolio::{
    active_weights,
    // Key rate duration
    aggregate_key_rate_profile,
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
    estimate_days_to_liquidate,
    estimate_tracking_error,
    liquidity_distribution,
    // Stress testing
    run_stress_scenario,
    run_stress_scenarios,
    spread_contributions,
    spread_difference_by_sector,
    stress_scenarios,
    summarize_results,
    AnalyticsConfig,
    BucketContribution,
    BucketMetrics,
    Classification,
    CreditRating,
    Cs01Contributions,
    DurationContributions,
    Dv01Contributions,
    Holding,
    HoldingAnalytics,
    HoldingBuilder,
    HoldingContribution,
    // Portfolio
    Portfolio as ConvexPortfolio,
    PortfolioBuilder,
    RateScenario,
    RatingBucket,
    RatingInfo,
    SecYieldInput,
    Sector,
    SpreadContributions,
    SpreadScenario,
    StressResult,
    StressScenario,
};

use crate::websocket::WebSocketState;
use convex_traits::ids::{CurveId, EtfId, InstrumentId, PortfolioId};
use convex_traits::output::{BondQuoteOutput, EtfQuoteOutput, PortfolioAnalyticsOutput};
use convex_traits::reference_data::{
    BondFilter, BondReferenceData, BondReferenceSource, EtfHoldingEntry, EtfHoldings,
};

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

pub mod analytics;
pub use analytics::*;

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
    #[allow(dead_code)]
    fn new(message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
        }
    }
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
    let bond = match state.bond_store.get_by_id(&id).await {
        Ok(Some(bond)) => bond,
        Ok(None) => match state.engine.reference_data().bonds.get_by_id(&id).await {
            Ok(Some(bond)) => bond,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": format!("Bond not found: {}", instrument_id)
                    })),
                ).into_response();
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": format!("Failed to look up bond in reference data: {}", e)
                    })),
                ).into_response();
            }
        },
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to look up bond in store: {}", e)
                })),
            ).into_response();
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
                ).into_response();
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
    let input = PricingInput::with_mid_price(
        bond,
        settlement_date,
        query.market_price,
        None, // discount_curve
        None, // benchmark_curve
        None, // government_curve
        None, // volatility
    );

    // Price the bond
    let router = state.engine.pricing_router();
    match router.price(&input) {
        Ok(quote) => {
            // Publish to WebSocket subscribers
            state.ws_state.publish_bond_quote(quote.clone());
            (StatusCode::OK, Json(serde_json::to_value(quote).unwrap())).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Pricing failed: {}", e)
            })),
        ).into_response(),
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
    let points: Vec<(f64, f64)> = request.points.iter().map(|p| (p.tenor, p.rate)).collect();

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
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(response).unwrap()),
            )
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
        StatusCode::NO_CONTENT.into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Curve not found" })),
        ).into_response()
    }
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
            ).into_response();
        }
    };

    let compounding = match query.compounding.to_lowercase().as_str() {
        "simple" => Compounding::Simple,
        "annual" => Compounding::Annual,
        "semiannual" | "semi_annual" => Compounding::SemiAnnual,
        "quarterly" => Compounding::Quarterly,
        "monthly" => Compounding::Monthly,
        "daily" => Compounding::Daily,
        "continuous" => Compounding::Continuous,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("Unsupported compounding value: '{}'", query.compounding)
                })),
            ).into_response();
        }
    };

    let compounding_str = match compounding {
        Compounding::Simple => "simple",
        Compounding::Annual => "annual",
        Compounding::SemiAnnual => "semiannual",
        Compounding::Quarterly => "quarterly",
        Compounding::Monthly => "monthly",
        Compounding::Daily => "daily",
        Compounding::Continuous => "continuous",
    };

    match curve.zero_rate(tenor, compounding) {
        Ok(rate) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "curve_id": curve_id,
                "tenor": tenor,
                "zero_rate": rate,
                "compounding": compounding_str
            })),
        ).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
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

/// List bonds with optional filtering and pagination.
pub async fn list_bonds(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<BondListQuery>,
) -> impl IntoResponse {
    use convex_traits::reference_data::{BondType, IssuerType};

    // Build filter from query params
    let filter = BondFilter {
        currency: query.currency.as_ref().and_then(|c| Currency::from_code(c)),
        issuer_type: query
            .issuer_type
            .as_ref()
            .and_then(|t| match t.to_lowercase().as_str() {
                "sovereign" => Some(IssuerType::Sovereign),
                "agency" => Some(IssuerType::Agency),
                "supranational" => Some(IssuerType::Supranational),
                "corporateig" | "corporate_ig" => Some(IssuerType::CorporateIG),
                "corporatehy" | "corporate_hy" => Some(IssuerType::CorporateHY),
                "financial" => Some(IssuerType::Financial),
                "municipal" => Some(IssuerType::Municipal),
                _ => None,
            }),
        bond_type: query
            .bond_type
            .as_ref()
            .and_then(|t| match t.to_lowercase().as_str() {
                "fixedbullet" | "fixed_bullet" => Some(BondType::FixedBullet),
                "fixedcallable" | "fixed_callable" => Some(BondType::FixedCallable),
                "fixedputable" | "fixed_putable" => Some(BondType::FixedPutable),
                "floatingrate" | "floating_rate" | "frn" => Some(BondType::FloatingRate),
                "zerocoupon" | "zero_coupon" => Some(BondType::ZeroCoupon),
                "inflationlinked" | "inflation_linked" | "linker" => {
                    Some(BondType::InflationLinked)
                }
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
    let bonds = match state
        .bond_store
        .search(&filter, query.limit, query.offset)
        .await
    {
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

    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap()),
    )
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
    match state.bond_store.get_by_id(&bond.instrument_id).await {
        Ok(Some(_)) => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": format!("Bond already exists: {}", bond.instrument_id.as_str())
                })),
            ).into_response();
        }
        Ok(None) => {}
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Database read error: {}", e)
                })),
            ).into_response();
        }
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

    (
        StatusCode::CREATED,
        Json(serde_json::to_value(created).unwrap()),
    ).into_response()
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
        Some(_) => StatusCode::NO_CONTENT.into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Bond not found: {}", instrument_id) })),
        ).into_response(),
    }
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
        match state.bond_store.get_by_id(&bond.instrument_id).await {
            Ok(Some(_)) => {
                skipped += 1;
                continue;
            }
            Ok(None) => {}
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": format!("Failed to verify bond existence: {}", e)
                    })),
                ).into_response();
            }
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

    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap()),
    ).into_response()
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
        Some(_) => StatusCode::NO_CONTENT.into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Portfolio not found: {}", portfolio_id) })),
        ).into_response(),
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

/// Parse a date string (YYYY-MM-DD) into a Date.
pub(crate) fn parse_date(s: &str) -> Result<Date, String> {
    Date::parse(s).map_err(|e| e.to_string())
}

/// Error returned when currency parsing fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseCurrencyError(pub String);

impl std::fmt::Display for ParseCurrencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Unsupported currency: {}", self.0)
    }
}

impl std::error::Error for ParseCurrencyError {}

pub(crate) fn parse_currency(s: &str) -> Result<Currency, ParseCurrencyError> {
    match s.to_uppercase().as_str() {
        "USD" => Ok(Currency::USD),
        "EUR" => Ok(Currency::EUR),
        "GBP" => Ok(Currency::GBP),
        "JPY" => Ok(Currency::JPY),
        "CHF" => Ok(Currency::CHF),
        "CAD" => Ok(Currency::CAD),
        "AUD" => Ok(Currency::AUD),
        "NZD" => Ok(Currency::NZD),
        "SEK" => Ok(Currency::SEK),
        "NOK" => Ok(Currency::NOK),
        "DKK" => Ok(Currency::DKK),
        "HKD" => Ok(Currency::HKD),
        "SGD" => Ok(Currency::SGD),
        "CNY" => Ok(Currency::CNY),
        "INR" => Ok(Currency::INR),
        "BRL" => Ok(Currency::BRL),
        "MXN" => Ok(Currency::MXN),
        "ZAR" => Ok(Currency::ZAR),
        _ => Err(ParseCurrencyError(s.to_string())),
    }
}
