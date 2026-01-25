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
use convex_traits::storage::{PortfolioFilter, StoredPortfolio, StoredPosition};
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
    BondReferenceData, BondReferenceSource, EtfHoldingEntry, EtfHoldings,
};
use convex_traits::storage::{BondFilter, Pagination};

use crate::handlers::AppState;

fn parse_date(s: &str) -> Result<Date, String> {
    use std::str::FromStr;
    // Try YYYY-MM-DD
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Date::from_ymd(date.year(), date.month(), date.day())
            .map_err(|e| e.to_string());
    }
    Err(format!("Invalid date format: {}", s))
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

    // Build pricing input (rely on engine for inputs if available, otherwise minimal)
    let config_resolver = state.engine.config_resolver();
    let config = config_resolver.resolve(&bond).await;

    // We can use engine's internal cache for market price if not provided
    // For now, respect the query param or default to None
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

// ... (Price Single Bond logic remains largely same, just remove manual store access if any)

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
        isin: None,
        cusip: None,
        instrument_id: None,
    };

    let pagination = Pagination::new(query.offset, query.limit);
    let store = &state.engine.storage().bonds;

    // Get total count first
    let total = match store.count(&filter).await {
        Ok(count) => count,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            );
        }
    };

    // Get bonds with pagination
    let page = match store
        .list(&filter, &pagination)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            );
        }
    };

    let response = BondListResponse {
        bonds: page.items,
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
    let store = &state.engine.storage().bonds;

    match store.get(&id).await {
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
    let store = &state.engine.storage().bonds;

    // Check if bond already exists
    if let Ok(Some(_)) = store.get(&bond.instrument_id).await {
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

    if let Err(e) = store.save(&bond).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        );
    }

    (
        StatusCode::CREATED,
        Json(serde_json::to_value(bond).unwrap()),
    )
}

/// Update an existing bond.
pub async fn update_bond(
    State(state): State<Arc<AppState>>,
    Path(instrument_id): Path<String>,
    Json(mut bond): Json<BondReferenceData>,
) -> impl IntoResponse {
    let id = InstrumentId::new(&instrument_id);
    let store = &state.engine.storage().bonds;

    // Verify bond exists
    match store.get(&id).await {
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

    if let Err(e) = store.save(&bond).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        );
    }

    (StatusCode::OK, Json(serde_json::to_value(bond).unwrap()))
}

/// Delete a bond.
pub async fn delete_bond(
    State(state): State<Arc<AppState>>,
    Path(instrument_id): Path<String>,
) -> impl IntoResponse {
    let id = InstrumentId::new(&instrument_id);
    let store = &state.engine.storage().bonds;

    match store.delete(&id).await {
        Ok(true) => (StatusCode::NO_CONTENT, Json(serde_json::json!({}))),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Bond not found: {}", instrument_id) })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
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
    let store = &state.engine.storage().bonds;

    // TODO: Use save_batch for better performance if backend supports it.
    // For now, iterate.
    for mut bond in request.bonds {
        // Check if bond already exists
        if let Ok(Some(_)) = store.get(&bond.instrument_id).await {
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

        if let Err(_) = store.save(&bond).await {
            // Log error?
        } else {
            created += 1;
        }
    }

    let response = BatchBondCreateResponse {
        created,
        skipped,
        errors,
    };

    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap()),
    )
}

/// Get bond by ISIN.
pub async fn get_bond_by_isin(
    State(state): State<Arc<AppState>>,
    Path(isin): Path<String>,
) -> impl IntoResponse {
    // Note: BondStore doesn't expose get_by_isin directly in the trait (yet).
    // The previous implementation used InMemoryBondStore which had it.
    // BondFilter supports filtering by ISIN.
    let store = &state.engine.storage().bonds;
    let filter = BondFilter::by_isin(isin.clone());
    let pagination = Pagination::new(0, 1);

    match store.list(&filter, &pagination).await {
        Ok(page) => {
            if let Some(bond) = page.items.first() {
                (StatusCode::OK, Json(serde_json::to_value(bond).unwrap()))
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": format!("Bond not found with ISIN: {}", isin) })),
                )
            }
        },
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
    let store = &state.engine.storage().bonds;
    let filter = BondFilter::by_cusip(cusip.clone());
    let pagination = Pagination::new(0, 1);

    match store.list(&filter, &pagination).await {
        Ok(page) => {
            if let Some(bond) = page.items.first() {
                (StatusCode::OK, Json(serde_json::to_value(bond).unwrap()))
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": format!("Bond not found with CUSIP: {}", cusip) })),
                )
            }
        },
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
    /// Maximum number of items to return
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination
    #[serde(default)]
    pub offset: usize,
    /// Currency filter
    pub currency: Option<String>,
    /// Text search query
    pub q: Option<String>,
}

/// Response for portfolio listing.
#[derive(Debug, Serialize)]
pub struct PortfolioListResponse {
    /// Portfolios
    pub portfolios: Vec<StoredPortfolio>,
    /// Total count
    pub total: usize,
    /// Limit used
    pub limit: usize,
    /// Offset used
    pub offset: usize,
}

/// Request to create a portfolio.
#[derive(Debug, Deserialize)]
pub struct CreatePortfolioRequest {
    pub portfolio_id: String,
    pub name: String,
    pub currency: String,
    pub description: Option<String>,
    pub positions: Vec<StoredPosition>,
}

/// Request to update a portfolio.
#[derive(Debug, Deserialize)]
pub struct UpdatePortfolioRequest {
    pub name: Option<String>,
    pub currency: Option<String>,
    pub description: Option<String>,
    pub positions: Option<Vec<StoredPosition>>,
}

/// Request to batch create portfolios.
#[derive(Debug, Deserialize)]
pub struct BatchPortfolioCreateRequest {
    pub portfolios: Vec<CreatePortfolioRequest>,
}

/// Response for batch creation.
#[derive(Debug, Serialize)]
pub struct BatchPortfolioCreateResponse {
    pub created: usize,
    pub skipped: usize,
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

    let pagination = Pagination::new(query.offset, query.limit);
    let store = &state.engine.storage().portfolios;

    let page = match store.list(&filter, &pagination).await {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            );
        }
    };

    let response = PortfolioListResponse {
        portfolios: page.items,
        total: page.total as usize,
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
    let store = &state.engine.storage().portfolios;

    match store.get(&portfolio_id).await {
        Ok(Some(portfolio)) => (
            StatusCode::OK,
            Json(serde_json::to_value(portfolio).unwrap()),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Portfolio not found: {}", portfolio_id) })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// Create a new portfolio.
pub async fn create_portfolio(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreatePortfolioRequest>,
) -> impl IntoResponse {
    let store = &state.engine.storage().portfolios;

    // Check if portfolio already exists
    if let Ok(Some(_)) = store.get(&request.portfolio_id).await {
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

    if let Err(e) = store.save(&portfolio).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        );
    }

    (
        StatusCode::CREATED,
        Json(serde_json::to_value(portfolio).unwrap()),
    )
}

/// Update an existing portfolio.
pub async fn update_portfolio(
    State(state): State<Arc<AppState>>,
    Path(portfolio_id): Path<String>,
    Json(request): Json<UpdatePortfolioRequest>,
) -> impl IntoResponse {
    let store = &state.engine.storage().portfolios;

    // Get existing portfolio
    let existing = match store.get(&portfolio_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(
                    serde_json::json!({ "error": format!("Portfolio not found: {}", portfolio_id) }),
                ),
            );
        },
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
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

    if let Err(e) = store.save(&portfolio).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        );
    }

    (StatusCode::OK, Json(serde_json::to_value(portfolio).unwrap()))
}

/// Delete a portfolio.
pub async fn delete_portfolio(
    State(state): State<Arc<AppState>>,
    Path(portfolio_id): Path<String>,
) -> impl IntoResponse {
    let store = &state.engine.storage().portfolios;

    match store.delete(&portfolio_id).await {
        Ok(true) => (StatusCode::NO_CONTENT, Json(serde_json::json!({}))),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Portfolio not found: {}", portfolio_id) })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// Add a position to a portfolio.
pub async fn add_portfolio_position(
    State(state): State<Arc<AppState>>,
    Path(portfolio_id): Path<String>,
    Json(position): Json<StoredPosition>,
) -> impl IntoResponse {
    let store = &state.engine.storage().portfolios;

    let mut portfolio = match store.get(&portfolio_id).await {
        Ok(Some(p)) => p,
        Ok(None) => return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Portfolio not found: {}", portfolio_id) })),
        ).into_response(),
        Err(e) => return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    };

    portfolio.positions.push(position);
    portfolio.updated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    if let Err(e) = store.save(&portfolio).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response();
    }

    (StatusCode::OK, Json(serde_json::to_value(portfolio).unwrap())).into_response()
}

/// Remove a position from a portfolio.
pub async fn remove_portfolio_position(
    State(state): State<Arc<AppState>>,
    Path((portfolio_id, instrument_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let store = &state.engine.storage().portfolios;

    let mut portfolio = match store.get(&portfolio_id).await {
        Ok(Some(p)) => p,
        Ok(None) => return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Portfolio not found: {}", portfolio_id) })),
        ).into_response(),
        Err(e) => return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    };

    portfolio.positions.retain(|p| p.instrument_id != instrument_id);
    portfolio.updated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    if let Err(e) = store.save(&portfolio).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response();
    }

    (StatusCode::OK, Json(serde_json::to_value(portfolio).unwrap())).into_response()
}

/// Update a position in a portfolio.
pub async fn update_portfolio_position(
    State(state): State<Arc<AppState>>,
    Path((portfolio_id, instrument_id)): Path<(String, String)>,
    Json(mut position): Json<StoredPosition>,
) -> impl IntoResponse {
    // Ensure instrument_id matches path
    position.instrument_id = instrument_id.clone();
    let store = &state.engine.storage().portfolios;

    let mut portfolio = match store.get(&portfolio_id).await {
        Ok(Some(p)) => p,
        Ok(None) => return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Portfolio not found: {}", portfolio_id) })),
        ).into_response(),
        Err(e) => return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    };

    if let Some(existing) = portfolio
        .positions
        .iter_mut()
        .find(|p| p.instrument_id == position.instrument_id)
    {
        *existing = position;
        portfolio.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
    } else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Position not found: {}", instrument_id) })),
        ).into_response();
    }

    if let Err(e) = store.save(&portfolio).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response();
    }

    (StatusCode::OK, Json(serde_json::to_value(portfolio).unwrap())).into_response()
}

/// Batch create portfolios.
pub async fn batch_create_portfolios(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BatchPortfolioCreateRequest>,
) -> impl IntoResponse {
    let mut created = 0;
    let mut skipped = 0;
    let store = &state.engine.storage().portfolios;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    for req in request.portfolios {
        // Check if portfolio already exists
        if let Ok(Some(_)) = store.get(&req.portfolio_id).await {
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

        if let Ok(_) = store.save(&portfolio).await {
            created += 1;
        }
    }

    let response = BatchPortfolioCreateResponse { created, skipped };

    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap()),
    )
}

// ... (Rest of logic remains mostly stateless and correct)
