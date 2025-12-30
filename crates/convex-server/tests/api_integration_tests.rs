//! Integration tests for the Convex Server API endpoints.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use tower::ServiceExt;

use convex_core::{Currency, Date};
use convex_engine::PricingEngineBuilder;
use convex_ext_file::{
    create_empty_output, EmptyBondReferenceSource, EmptyCurveInputSource, EmptyEtfHoldingsSource,
    EmptyEtfQuoteSource, EmptyFxRateSource, EmptyIndexFixingSource, EmptyInflationFixingSource,
    EmptyIssuerReferenceSource, EmptyQuoteSource, EmptyRatingSource, EmptyVolatilitySource,
    InMemoryBondStore, InMemoryPortfolioStore,
};
use convex_server::routes::{create_router, create_router_with_bond_store, create_router_with_stores};
use convex_traits::config::EngineConfig;
use convex_traits::ids::InstrumentId;
use convex_traits::market_data::MarketDataProvider;
use convex_traits::output::BondQuoteOutput;
use convex_traits::reference_data::{BondReferenceData, BondType, IssuerType, ReferenceDataProvider};

/// Create test resources (engine + bond store) for tests that need shared state.
fn create_test_resources() -> (Arc<convex_engine::PricingEngine>, Arc<InMemoryBondStore>) {
    let engine = create_test_engine();
    let bond_store = Arc::new(InMemoryBondStore::new());
    (engine, bond_store)
}

/// Create a test engine with empty providers.
fn create_test_engine() -> Arc<convex_engine::PricingEngine> {
    let market_data = MarketDataProvider {
        quotes: Arc::new(EmptyQuoteSource),
        curve_inputs: Arc::new(EmptyCurveInputSource),
        index_fixings: Arc::new(EmptyIndexFixingSource),
        volatility: Arc::new(EmptyVolatilitySource),
        fx_rates: Arc::new(EmptyFxRateSource),
        inflation_fixings: Arc::new(EmptyInflationFixingSource),
        etf_quotes: Arc::new(EmptyEtfQuoteSource),
    };

    let reference_data = ReferenceDataProvider {
        bonds: Arc::new(EmptyBondReferenceSource),
        issuers: Arc::new(EmptyIssuerReferenceSource),
        ratings: Arc::new(EmptyRatingSource),
        etf_holdings: Arc::new(EmptyEtfHoldingsSource),
    };

    let output = create_empty_output();

    // Use in-memory storage for tests
    let storage = convex_ext_redb::create_memory_storage().expect("Failed to create memory storage");

    let engine = PricingEngineBuilder::new()
        .with_config(EngineConfig::default())
        .with_market_data(Arc::new(market_data))
        .with_reference_data(Arc::new(reference_data))
        .with_storage(Arc::new(storage))
        .with_output(Arc::new(output))
        .build()
        .expect("Failed to build engine");

    Arc::new(engine)
}

/// Create test bond reference data.
fn create_test_bond(id: &str, coupon: Decimal, maturity_year: i32) -> BondReferenceData {
    BondReferenceData {
        instrument_id: InstrumentId::new(id),
        isin: Some(id.to_string()),
        cusip: Some(format!("{}CUSIP", &id[..6.min(id.len())])),
        sedol: None,
        bbgid: None,
        description: format!("Test Bond {}", id),
        currency: Currency::USD,
        issue_date: Date::from_ymd(2020, 1, 15).unwrap(),
        maturity_date: Date::from_ymd(maturity_year, 6, 15).unwrap(),
        coupon_rate: Some(coupon),
        frequency: 2,
        day_count: "30/360".to_string(),
        face_value: dec!(100),
        bond_type: BondType::FixedBullet,
        issuer_type: IssuerType::CorporateIG,
        issuer_id: "TEST_ISSUER".to_string(),
        issuer_name: "Test Issuer Corp".to_string(),
        seniority: "Senior".to_string(),
        is_callable: false,
        call_schedule: vec![],
        is_putable: false,
        is_sinkable: false,
        floating_terms: None,
        inflation_index: None,
        inflation_base_index: None,
        has_deflation_floor: false,
        country_of_risk: "US".to_string(),
        sector: "Financials".to_string(),
        amount_outstanding: None,
        first_coupon_date: None,
        last_updated: 0,
        source: "test".to_string(),
    }
}

/// Create a test bond quote output.
fn create_test_quote(id: &str, clean_price: Decimal) -> BondQuoteOutput {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    BondQuoteOutput {
        instrument_id: InstrumentId::new(id),
        isin: Some(id.to_string()),
        currency: Currency::USD,
        settlement_date: Date::from_ymd(2025, 6, 17).unwrap(),
        clean_price: Some(clean_price),
        dirty_price: Some(clean_price + dec!(0.50)),
        accrued_interest: Some(dec!(0.50)),
        ytm: Some(dec!(0.05)),
        ytw: None,
        ytc: None,
        z_spread: Some(dec!(50)),
        i_spread: Some(dec!(45)),
        g_spread: Some(dec!(55)),
        asw: Some(dec!(48)),
        oas: None,
        discount_margin: None,
        simple_margin: None,
        modified_duration: Some(dec!(5.5)),
        macaulay_duration: Some(dec!(5.7)),
        effective_duration: Some(dec!(5.5)),
        spread_duration: Some(dec!(5.4)),
        convexity: Some(dec!(35)),
        effective_convexity: Some(dec!(34)),
        dv01: Some(dec!(0.055)),
        pv01: Some(dec!(0.054)),
        key_rate_durations: Some(vec![
            ("2Y".to_string(), dec!(0.5)),
            ("5Y".to_string(), dec!(2.0)),
            ("10Y".to_string(), dec!(3.0)),
        ]),
        cs01: Some(dec!(0.054)),
        timestamp: now,
        pricing_model: "DiscountToMaturity".to_string(),
        source: "test".to_string(),
        is_stale: false,
        quality: 100,
    }
}

/// Helper to make a POST request and get JSON response.
async fn post_json(
    app: axum::Router,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap_or(json!({}));

    (status, json)
}

// =============================================================================
// HEALTH CHECK TESTS
// =============================================================================

#[tokio::test]
async fn test_health_endpoint() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "ok");
    assert!(json["version"].is_string());
}

// =============================================================================
// BATCH PRICING TESTS
// =============================================================================

#[tokio::test]
async fn test_batch_price_single_bond() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let bond = create_test_bond("US912810TD00", dec!(0.05), 2030);

    let request_body = json!({
        "settlement_date": "2025-06-17",
        "parallel": false,
        "bonds": [
            {
                "bond": bond,
                "market_price": 99.50
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/batch/price", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["stats"]["total"].as_u64().unwrap() >= 1);
    assert!(json["stats"]["elapsed_ms"].is_number());
    assert!(json["stats"]["bonds_per_second"].is_number());
}

#[tokio::test]
async fn test_batch_price_multiple_bonds_parallel() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let bonds: Vec<Value> = (0..10)
        .map(|i| {
            let bond = create_test_bond(
                &format!("US91281{}TD00", i),
                dec!(0.04) + Decimal::from(i) * dec!(0.005),
                2028 + (i as i32 % 5),
            );
            json!({
                "bond": bond,
                "market_price": 98.0 + (i as f64 * 0.5)
            })
        })
        .collect();

    let request_body = json!({
        "settlement_date": "2025-06-17",
        "parallel": true,
        "bonds": bonds
    });

    let (status, json) = post_json(app, "/api/v1/batch/price", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["stats"]["total"].as_u64().unwrap(), 10);

    // Check we got results (either quotes or errors)
    let succeeded = json["stats"]["succeeded"].as_u64().unwrap();
    let failed = json["stats"]["failed"].as_u64().unwrap();
    assert_eq!(succeeded + failed, 10);
}

#[tokio::test]
async fn test_batch_price_invalid_date() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "settlement_date": "invalid-date",
        "bonds": []
    });

    let (status, json) = post_json(app, "/api/v1/batch/price", request_body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json["error"].is_string());
}

// =============================================================================
// ETF iNAV TESTS
// =============================================================================

#[tokio::test]
async fn test_etf_inav_calculation() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let bond_prices = vec![
        create_test_quote("US912810TD00", dec!(99.50)),
        create_test_quote("US037833DV24", dec!(101.00)),
    ];

    let request_body = json!({
        "settlement_date": "2025-06-17",
        "holdings": {
            "etf_id": "LQD",
            "name": "iShares Investment Grade Corporate Bond ETF",
            "as_of_date": "2025-06-15",
            "holdings": [
                {
                    "instrument_id": "US912810TD00",
                    "weight": 0.60,
                    "shares": 1000,
                    "market_value": 100000,
                    "notional_value": 100000,
                    "accrued_interest": 500
                },
                {
                    "instrument_id": "US037833DV24",
                    "weight": 0.40,
                    "shares": 1500,
                    "market_value": 150000,
                    "notional_value": 150000,
                    "accrued_interest": 750
                }
            ],
            "total_market_value": 250000,
            "shares_outstanding": 2500,
            "nav_per_share": 100.00
        },
        "bond_prices": bond_prices
    });

    let (status, json) = post_json(app, "/api/v1/etf/inav", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["etf_id"], "LQD");
    // Check structure is correct - fields should exist (may be null if not calculated)
    assert!(json.get("coverage").is_some(), "coverage field should exist");
    assert!(json.get("inav").is_some(), "iNAV field should exist");
    assert!(json.get("duration").is_some(), "Duration field should exist");
    assert!(json.get("num_holdings").is_some(), "num_holdings field should exist");
}

#[tokio::test]
async fn test_etf_inav_batch() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let bond_prices = vec![
        create_test_quote("US912810TD00", dec!(99.50)),
        create_test_quote("US037833DV24", dec!(101.00)),
    ];

    let request_body = json!({
        "settlement_date": "2025-06-17",
        "etfs": [
            {
                "etf_id": "LQD",
                "name": "iShares IG Corporate",
                "as_of_date": "2025-06-15",
                "holdings": [
                    {
                        "instrument_id": "US912810TD00",
                        "weight": 1.0,
                        "shares": 1000,
                        "market_value": 100000,
                        "notional_value": 100000
                    }
                ],
                "total_market_value": 100000,
                "shares_outstanding": 1000,
                "nav_per_share": 100.00
            },
            {
                "etf_id": "HYG",
                "name": "iShares High Yield",
                "as_of_date": "2025-06-15",
                "holdings": [
                    {
                        "instrument_id": "US037833DV24",
                        "weight": 1.0,
                        "shares": 1500,
                        "market_value": 150000,
                        "notional_value": 150000
                    }
                ],
                "total_market_value": 150000,
                "shares_outstanding": 1500,
                "nav_per_share": 100.00
            }
        ],
        "bond_prices": bond_prices
    });

    let (status, json) = post_json(app, "/api/v1/etf/inav/batch", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["results"].as_array().unwrap().len(), 2);
    assert_eq!(json["results"][0]["etf_id"], "LQD");
    assert_eq!(json["results"][1]["etf_id"], "HYG");
}

#[tokio::test]
async fn test_etf_inav_invalid_date() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "settlement_date": "not-a-date",
        "holdings": {
            "etf_id": "TEST",
            "name": "Test",
            "as_of_date": "2025-06-15",
            "holdings": [],
            "total_market_value": 0,
            "shares_outstanding": 1000
        },
        "bond_prices": []
    });

    let (status, json) = post_json(app, "/api/v1/etf/inav", request_body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json["error"].is_string());
}

// =============================================================================
// PORTFOLIO ANALYTICS TESTS
// =============================================================================

#[tokio::test]
async fn test_portfolio_analytics() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let bond_prices = vec![
        create_test_quote("US912810TD00", dec!(99.50)),
        create_test_quote("US037833DV24", dec!(101.00)),
    ];

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "CORP_IG",
            "name": "Investment Grade Corporate",
            "currency": "USD",
            "positions": [
                {
                    "instrument_id": "US912810TD00",
                    "notional": 1000000,
                    "sector": "Financials",
                    "rating": "A"
                },
                {
                    "instrument_id": "US037833DV24",
                    "notional": 500000,
                    "sector": "Technology",
                    "rating": "AA"
                }
            ]
        },
        "bond_prices": bond_prices
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/analytics", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "CORP_IG");
    assert_eq!(json["name"], "Investment Grade Corporate");
    // Analytics values are computed from bond prices
    assert!(json.get("market_value").is_some(), "market_value field should exist");
    assert!(json.get("duration").is_some(), "duration field should exist");
    assert!(json.get("convexity").is_some(), "convexity field should exist");
    assert!(json.get("dv01").is_some(), "dv01 field should exist");
    assert!(json["sector_breakdown"].is_array(), "sector_breakdown should be array");
    assert!(json["rating_breakdown"].is_array(), "rating_breakdown should be array");
}

#[tokio::test]
async fn test_portfolio_analytics_batch() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let bond_prices = vec![
        create_test_quote("US912810TD00", dec!(99.50)),
        create_test_quote("US037833DV24", dec!(101.00)),
    ];

    let request_body = json!({
        "portfolios": [
            {
                "portfolio_id": "PORT_A",
                "name": "Portfolio A",
                "currency": "USD",
                "positions": [
                    { "instrument_id": "US912810TD00", "notional": 1000000 }
                ]
            },
            {
                "portfolio_id": "PORT_B",
                "name": "Portfolio B",
                "currency": "EUR",
                "positions": [
                    { "instrument_id": "US037833DV24", "notional": 500000 }
                ]
            }
        ],
        "bond_prices": bond_prices
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/analytics/batch", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["results"].as_array().unwrap().len(), 2);
    assert_eq!(json["results"][0]["portfolio_id"], "PORT_A");
    assert_eq!(json["results"][1]["portfolio_id"], "PORT_B");
}

#[tokio::test]
async fn test_duration_contribution() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let bond_prices = vec![
        create_test_quote("US912810TD00", dec!(99.50)),
        create_test_quote("US037833DV24", dec!(101.00)),
    ];

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "TEST_PORT",
            "name": "Test Portfolio",
            "positions": [
                { "instrument_id": "US912810TD00", "notional": 600000 },
                { "instrument_id": "US037833DV24", "notional": 400000 }
            ]
        },
        "bond_prices": bond_prices
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/duration-contribution", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["contributions"].is_array(), "contributions should be array");
    assert!(json.get("total_duration").is_some(), "total_duration field should exist");

    let contributions = json["contributions"].as_array().unwrap();
    // May have 0-2 contributions depending on price matching
    assert!(contributions.len() <= 2, "Should have at most 2 contributions");

    // If there are contributions, check their structure
    for contrib in contributions {
        assert!(contrib["instrument_id"].is_string());
        assert!(contrib.get("weight").is_some());
        assert!(contrib.get("contribution").is_some());
    }
}

#[tokio::test]
async fn test_portfolio_analytics_empty_portfolio() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "EMPTY",
            "name": "Empty Portfolio",
            "positions": []
        },
        "bond_prices": []
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/analytics", request_body).await;

    // Should still return OK but with zero values or error
    assert!(status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_portfolio_analytics_missing_prices() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Portfolio has positions but no matching prices
    let request_body = json!({
        "portfolio": {
            "portfolio_id": "NO_PRICES",
            "name": "No Prices Portfolio",
            "positions": [
                { "instrument_id": "UNKNOWN_BOND", "notional": 1000000 }
            ]
        },
        "bond_prices": []
    });

    let (status, _json) = post_json(app, "/api/v1/portfolio/analytics", request_body).await;

    // Should handle gracefully (either OK with partial data or error)
    assert!(status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR);
}

// =============================================================================
// CURVES ENDPOINT TESTS
// =============================================================================

#[tokio::test]
async fn test_list_curves() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .uri("/api/v1/curves")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert!(json["curves"].is_array());
}

#[tokio::test]
async fn test_get_curve_not_found() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .uri("/api/v1/curves/NONEXISTENT")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_create_curve() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "curve_id": "USD_OIS",
        "reference_date": "2025-06-17",
        "points": [
            { "tenor": 0.25, "rate": 0.04 },
            { "tenor": 0.5, "rate": 0.0425 },
            { "tenor": 1.0, "rate": 0.045 },
            { "tenor": 2.0, "rate": 0.048 },
            { "tenor": 5.0, "rate": 0.052 },
            { "tenor": 10.0, "rate": 0.055 }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/curves", request_body).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(json["curve_id"], "USD_OIS");
    assert_eq!(json["reference_date"], "2025-06-17");
    assert!(json["points"].is_array());
    assert_eq!(json["points"].as_array().unwrap().len(), 6);
    assert!(json["built_at"].is_number());
}

#[tokio::test]
async fn test_create_curve_invalid_date() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "curve_id": "USD_OIS",
        "reference_date": "invalid-date",
        "points": [
            { "tenor": 1.0, "rate": 0.045 }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/curves", request_body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json["error"].is_string());
}

#[tokio::test]
async fn test_create_curve_empty_points() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "curve_id": "EMPTY_CURVE",
        "reference_date": "2025-06-17",
        "points": []
    });

    let (status, json) = post_json(app, "/api/v1/curves", request_body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json["error"].as_str().unwrap().contains("point"));
}

#[tokio::test]
async fn test_create_curve_and_query_zero_rate() {
    let engine = create_test_engine();

    // Create curve first
    let app1 = create_router(engine.clone());
    let request_body = json!({
        "curve_id": "USD_SOFR",
        "reference_date": "2025-06-17",
        "points": [
            { "tenor": 1.0, "rate": 0.045 },
            { "tenor": 2.0, "rate": 0.048 },
            { "tenor": 5.0, "rate": 0.052 }
        ]
    });

    let (status, _) = post_json(app1, "/api/v1/curves", request_body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Now query zero rate at 1.5 years (interpolated)
    let app2 = create_router(engine);
    let request = Request::builder()
        .uri("/api/v1/curves/USD_SOFR/zero/1.5")
        .body(Body::empty())
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["curve_id"], "USD_SOFR");
    assert_eq!(json["tenor"], 1.5);
    // Rate should be interpolated between 0.045 and 0.048
    let rate = json["zero_rate"].as_f64().unwrap();
    assert!(rate > 0.045 && rate < 0.048, "Rate {} should be between 0.045 and 0.048", rate);
}

#[tokio::test]
async fn test_create_curve_and_query_discount_factor() {
    let engine = create_test_engine();

    // Create curve first
    let app1 = create_router(engine.clone());
    let request_body = json!({
        "curve_id": "USD_DF_TEST",
        "reference_date": "2025-06-17",
        "points": [
            { "tenor": 1.0, "rate": 0.05 },
            { "tenor": 2.0, "rate": 0.05 }
        ]
    });

    let (status, _) = post_json(app1, "/api/v1/curves", request_body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Query discount factor at 1 year
    let app2 = create_router(engine);
    let request = Request::builder()
        .uri("/api/v1/curves/USD_DF_TEST/discount/1.0")
        .body(Body::empty())
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["curve_id"], "USD_DF_TEST");
    assert_eq!(json["tenor"], 1.0);
    // DF at 1Y with 5% continuous rate ≈ exp(-0.05 * 1) ≈ 0.9512
    let df = json["discount_factor"].as_f64().unwrap();
    assert!((df - 0.9512).abs() < 0.001, "DF {} should be approximately 0.9512", df);
}

#[tokio::test]
async fn test_create_curve_and_query_forward_rate() {
    let engine = create_test_engine();

    // Create a flat curve for easier testing
    let app1 = create_router(engine.clone());
    let request_body = json!({
        "curve_id": "USD_FWD_TEST",
        "reference_date": "2025-06-17",
        "points": [
            { "tenor": 1.0, "rate": 0.05 },
            { "tenor": 2.0, "rate": 0.05 },
            { "tenor": 5.0, "rate": 0.05 }
        ]
    });

    let (status, _) = post_json(app1, "/api/v1/curves", request_body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Query forward rate from 1Y to 2Y
    let app2 = create_router(engine);
    let request = Request::builder()
        .uri("/api/v1/curves/USD_FWD_TEST/forward/1.0/2.0")
        .body(Body::empty())
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["curve_id"], "USD_FWD_TEST");
    assert_eq!(json["t1"], 1.0);
    assert_eq!(json["t2"], 2.0);
    // Flat curve should have forward rate = spot rate
    let fwd = json["forward_rate"].as_f64().unwrap();
    assert!((fwd - 0.05).abs() < 0.001, "Forward rate {} should be approximately 0.05", fwd);
}

#[tokio::test]
async fn test_query_zero_rate_curve_not_found() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .uri("/api/v1/curves/NONEXISTENT/zero/1.0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_curve() {
    let engine = create_test_engine();

    // Create curve first
    let app1 = create_router(engine.clone());
    let request_body = json!({
        "curve_id": "TO_DELETE",
        "reference_date": "2025-06-17",
        "points": [
            { "tenor": 1.0, "rate": 0.045 }
        ]
    });

    let (status, _) = post_json(app1, "/api/v1/curves", request_body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Delete the curve
    let app2 = create_router(engine.clone());
    let request = Request::builder()
        .method("DELETE")
        .uri("/api/v1/curves/TO_DELETE")
        .body(Body::empty())
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // Verify it's deleted
    let app3 = create_router(engine);
    let request = Request::builder()
        .uri("/api/v1/curves/TO_DELETE")
        .body(Body::empty())
        .unwrap();

    let response = app3.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_curve_not_found() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .method("DELETE")
        .uri("/api/v1/curves/NONEXISTENT")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_query_zero_rate_with_compounding() {
    let engine = create_test_engine();

    // Create curve first
    let app1 = create_router(engine.clone());
    let request_body = json!({
        "curve_id": "USD_COMP_TEST",
        "reference_date": "2025-06-17",
        "points": [
            { "tenor": 1.0, "rate": 0.05 }
        ]
    });

    let (status, _) = post_json(app1, "/api/v1/curves", request_body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Query with annual compounding
    let app2 = create_router(engine);
    let request = Request::builder()
        .uri("/api/v1/curves/USD_COMP_TEST/zero/1.0?compounding=annual")
        .body(Body::empty())
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["compounding"], "annual");
    // Annual rate from 5% continuous: exp(0.05) - 1 ≈ 0.0513
    let rate = json["zero_rate"].as_f64().unwrap();
    assert!((rate - 0.0513).abs() < 0.001, "Annual rate {} should be approximately 0.0513", rate);
}

// =============================================================================
// SINGLE BOND QUOTE TESTS
// =============================================================================

#[tokio::test]
async fn test_get_bond_quote_not_found() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // GET request for a bond that doesn't exist (empty bond reference source)
    let request = Request::builder()
        .uri("/api/v1/quotes/NONEXISTENT_BOND")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert!(json["error"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn test_post_single_bond_quote() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let bond = create_test_bond("US912810TD00", dec!(0.05), 2030);

    let request_body = json!({
        "bond": bond,
        "settlement_date": "2025-06-17",
        "market_price": "99.50"
    });

    let (status, json) = post_json(app, "/api/v1/quote", request_body).await;

    assert_eq!(status, StatusCode::OK);
    // Verify we got a quote response
    assert!(json.get("instrument_id").is_some());
    assert_eq!(json["instrument_id"], "US912810TD00");
}

#[tokio::test]
async fn test_post_single_bond_quote_minimal() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let bond = create_test_bond("US037833DV24", dec!(0.04), 2028);

    let request_body = json!({
        "bond": bond,
        "settlement_date": "2025-06-17"
    });

    let (status, json) = post_json(app, "/api/v1/quote", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["instrument_id"], "US037833DV24");
}

#[tokio::test]
async fn test_post_single_bond_quote_invalid_date() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let bond = create_test_bond("US912810TD00", dec!(0.05), 2030);

    let request_body = json!({
        "bond": bond,
        "settlement_date": "not-a-date"
    });

    let (status, json) = post_json(app, "/api/v1/quote", request_body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json["error"].is_string());
}

// =============================================================================
// BOND REFERENCE DATA CRUD TESTS
// =============================================================================

#[tokio::test]
async fn test_list_bonds_empty() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .uri("/api/v1/bonds")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert!(json["bonds"].is_array());
    assert_eq!(json["bonds"].as_array().unwrap().len(), 0);
    assert_eq!(json["total"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn test_create_bond() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let bond = create_test_bond("US912810TD00", dec!(0.05), 2030);

    let (status, json) = post_json(app, "/api/v1/bonds", serde_json::to_value(&bond).unwrap()).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(json["instrument_id"], "US912810TD00");
    assert!(json["last_updated"].is_number());
}

#[tokio::test]
async fn test_create_and_get_bond() {
    let (engine, bond_store) = create_test_resources();

    // Create bond
    let app1 = create_router_with_bond_store(engine.clone(), bond_store.clone());
    let bond = create_test_bond("US037833DV24", dec!(0.04), 2028);
    let (status, _) = post_json(app1, "/api/v1/bonds", serde_json::to_value(&bond).unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);

    // Get bond
    let app2 = create_router_with_bond_store(engine, bond_store);
    let request = Request::builder()
        .uri("/api/v1/bonds/US037833DV24")
        .body(Body::empty())
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["instrument_id"], "US037833DV24");
}

#[tokio::test]
async fn test_get_bond_not_found() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .uri("/api/v1/bonds/NONEXISTENT")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_create_bond_duplicate() {
    let (engine, bond_store) = create_test_resources();

    // Create bond
    let app1 = create_router_with_bond_store(engine.clone(), bond_store.clone());
    let bond = create_test_bond("DUPE001", dec!(0.05), 2030);
    let (status, _) = post_json(app1, "/api/v1/bonds", serde_json::to_value(&bond).unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);

    // Try to create duplicate
    let app2 = create_router_with_bond_store(engine, bond_store);
    let (status, json) = post_json(app2, "/api/v1/bonds", serde_json::to_value(&bond).unwrap()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(json["error"].as_str().unwrap().contains("already exists"));
}

#[tokio::test]
async fn test_update_bond() {
    let (engine, bond_store) = create_test_resources();

    // Create bond
    let app1 = create_router_with_bond_store(engine.clone(), bond_store.clone());
    let bond = create_test_bond("UPDATE001", dec!(0.05), 2030);
    let (status, _) = post_json(app1, "/api/v1/bonds", serde_json::to_value(&bond).unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);

    // Update bond
    let app2 = create_router_with_bond_store(engine, bond_store);
    let mut updated_bond = bond.clone();
    updated_bond.description = "Updated Description".to_string();

    let request = Request::builder()
        .method("PUT")
        .uri("/api/v1/bonds/UPDATE001")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&updated_bond).unwrap()))
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["description"], "Updated Description");
}

#[tokio::test]
async fn test_update_bond_not_found() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let bond = create_test_bond("NONEXISTENT", dec!(0.05), 2030);

    let request = Request::builder()
        .method("PUT")
        .uri("/api/v1/bonds/NONEXISTENT")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&bond).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_bond() {
    let (engine, bond_store) = create_test_resources();

    // Create bond
    let app1 = create_router_with_bond_store(engine.clone(), bond_store.clone());
    let bond = create_test_bond("DELETE001", dec!(0.05), 2030);
    let (status, _) = post_json(app1, "/api/v1/bonds", serde_json::to_value(&bond).unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);

    // Delete bond
    let app2 = create_router_with_bond_store(engine.clone(), bond_store.clone());
    let request = Request::builder()
        .method("DELETE")
        .uri("/api/v1/bonds/DELETE001")
        .body(Body::empty())
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // Verify deleted
    let app3 = create_router_with_bond_store(engine, bond_store);
    let request = Request::builder()
        .uri("/api/v1/bonds/DELETE001")
        .body(Body::empty())
        .unwrap();

    let response = app3.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_bond_not_found() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .method("DELETE")
        .uri("/api/v1/bonds/NONEXISTENT")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_list_bonds_with_pagination() {
    let (engine, bond_store) = create_test_resources();

    // Create multiple bonds
    for i in 0..5 {
        let app = create_router_with_bond_store(engine.clone(), bond_store.clone());
        let bond = create_test_bond(&format!("PAGE{:03}", i), dec!(0.04) + Decimal::from(i) * dec!(0.001), 2030 + i as i32);
        let (status, _) = post_json(app, "/api/v1/bonds", serde_json::to_value(&bond).unwrap()).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    // Get first page
    let app = create_router_with_bond_store(engine.clone(), bond_store.clone());
    let request = Request::builder()
        .uri("/api/v1/bonds?limit=2&offset=0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["bonds"].as_array().unwrap().len(), 2);
    assert_eq!(json["total"].as_u64().unwrap(), 5);
    assert_eq!(json["limit"].as_u64().unwrap(), 2);
    assert_eq!(json["offset"].as_u64().unwrap(), 0);

    // Get second page
    let app = create_router_with_bond_store(engine, bond_store);
    let request = Request::builder()
        .uri("/api/v1/bonds?limit=2&offset=2")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["bonds"].as_array().unwrap().len(), 2);
    assert_eq!(json["offset"].as_u64().unwrap(), 2);
}

#[tokio::test]
async fn test_list_bonds_with_filter() {
    let (engine, bond_store) = create_test_resources();

    // Create bonds with different sectors
    let app1 = create_router_with_bond_store(engine.clone(), bond_store.clone());
    let mut bond1 = create_test_bond("SECTOR001", dec!(0.05), 2030);
    bond1.sector = "Technology".to_string();
    let (status, _) = post_json(app1, "/api/v1/bonds", serde_json::to_value(&bond1).unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);

    let app2 = create_router_with_bond_store(engine.clone(), bond_store.clone());
    let mut bond2 = create_test_bond("SECTOR002", dec!(0.05), 2030);
    bond2.sector = "Financials".to_string();
    let (status, _) = post_json(app2, "/api/v1/bonds", serde_json::to_value(&bond2).unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);

    // Filter by sector
    let app3 = create_router_with_bond_store(engine, bond_store);
    let request = Request::builder()
        .uri("/api/v1/bonds?sector=Technology")
        .body(Body::empty())
        .unwrap();

    let response = app3.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"].as_u64().unwrap(), 1);
    assert_eq!(json["bonds"][0]["sector"], "Technology");
}

#[tokio::test]
async fn test_batch_create_bonds() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let bonds = vec![
        create_test_bond("BATCH001", dec!(0.04), 2028),
        create_test_bond("BATCH002", dec!(0.045), 2029),
        create_test_bond("BATCH003", dec!(0.05), 2030),
    ];

    let request_body = json!({
        "bonds": bonds
    });

    let (status, json) = post_json(app, "/api/v1/bonds/batch", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["created"].as_u64().unwrap(), 3);
    assert_eq!(json["skipped"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn test_batch_create_bonds_with_duplicates() {
    let (engine, bond_store) = create_test_resources();

    // Create one bond first
    let app1 = create_router_with_bond_store(engine.clone(), bond_store.clone());
    let bond = create_test_bond("BATCHDUPE001", dec!(0.05), 2030);
    let (status, _) = post_json(app1, "/api/v1/bonds", serde_json::to_value(&bond).unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);

    // Batch create with duplicate
    let app2 = create_router_with_bond_store(engine, bond_store);
    let bonds = vec![
        create_test_bond("BATCHDUPE001", dec!(0.04), 2028), // duplicate
        create_test_bond("BATCHDUPE002", dec!(0.045), 2029),
    ];

    let request_body = json!({
        "bonds": bonds
    });

    let (status, json) = post_json(app2, "/api/v1/bonds/batch", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["created"].as_u64().unwrap(), 1);
    assert_eq!(json["skipped"].as_u64().unwrap(), 1);
}

#[tokio::test]
async fn test_get_bond_by_isin() {
    let (engine, bond_store) = create_test_resources();

    // Create bond
    let app1 = create_router_with_bond_store(engine.clone(), bond_store.clone());
    let bond = create_test_bond("ISINTEST001", dec!(0.05), 2030);
    let isin = bond.isin.clone().unwrap();
    let (status, _) = post_json(app1, "/api/v1/bonds", serde_json::to_value(&bond).unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);

    // Get by ISIN
    let app2 = create_router_with_bond_store(engine, bond_store);
    let request = Request::builder()
        .uri(&format!("/api/v1/bonds/isin/{}", isin))
        .body(Body::empty())
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["instrument_id"], "ISINTEST001");
}

#[tokio::test]
async fn test_get_bond_by_isin_not_found() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .uri("/api/v1/bonds/isin/NONEXISTENT")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_text_search_bonds() {
    let (engine, bond_store) = create_test_resources();

    // Create bonds with specific descriptions
    let app1 = create_router_with_bond_store(engine.clone(), bond_store.clone());
    let mut bond1 = create_test_bond("SEARCH001", dec!(0.05), 2030);
    bond1.description = "Apple Inc 5% 2030".to_string();
    bond1.issuer_name = "Apple Inc".to_string();
    let (status, _) = post_json(app1, "/api/v1/bonds", serde_json::to_value(&bond1).unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);

    let app2 = create_router_with_bond_store(engine.clone(), bond_store.clone());
    let mut bond2 = create_test_bond("SEARCH002", dec!(0.04), 2028);
    bond2.description = "Microsoft Corp 4% 2028".to_string();
    bond2.issuer_name = "Microsoft Corp".to_string();
    let (status, _) = post_json(app2, "/api/v1/bonds", serde_json::to_value(&bond2).unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);

    // Search for Apple
    let app3 = create_router_with_bond_store(engine, bond_store);
    let request = Request::builder()
        .uri("/api/v1/bonds?q=Apple")
        .body(Body::empty())
        .unwrap();

    let response = app3.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"].as_u64().unwrap(), 1);
    assert!(json["bonds"][0]["description"].as_str().unwrap().contains("Apple"));
}

// =============================================================================
// WEBSOCKET STATUS TESTS
// =============================================================================

#[tokio::test]
async fn test_websocket_status_endpoint() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .uri("/api/v1/ws/status")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert!(json.get("active_connections").is_some());
    assert_eq!(json["active_connections"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn test_websocket_upgrade_request() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Test that the /ws endpoint exists (without actual WebSocket upgrade)
    // A normal GET without upgrade headers should return an error
    let request = Request::builder()
        .uri("/api/v1/ws")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    // Without proper WebSocket upgrade headers, this returns an error status
    // This just confirms the route exists
    assert!(response.status().is_client_error() || response.status().is_success());
}

// =============================================================================
// PORTFOLIO CRUD TESTS
// =============================================================================

/// Create test resources with all stores (engine + bond store + portfolio store).
fn create_all_test_resources() -> (
    Arc<convex_engine::PricingEngine>,
    Arc<InMemoryBondStore>,
    Arc<InMemoryPortfolioStore>,
) {
    let engine = create_test_engine();
    let bond_store = Arc::new(InMemoryBondStore::new());
    let portfolio_store = Arc::new(InMemoryPortfolioStore::new());
    (engine, bond_store, portfolio_store)
}

#[tokio::test]
async fn test_list_portfolios_empty() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .uri("/api/v1/portfolios")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert!(json["portfolios"].is_array());
    assert_eq!(json["portfolios"].as_array().unwrap().len(), 0);
    assert_eq!(json["total"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn test_create_portfolio() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio_id": "PORT001",
        "name": "Test Portfolio",
        "currency": "USD",
        "description": "A test portfolio",
        "positions": []
    });

    let (status, json) = post_json(app, "/api/v1/portfolios", request_body).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(json["portfolio_id"], "PORT001");
    assert_eq!(json["name"], "Test Portfolio");
    assert_eq!(json["currency"], "USD");
    assert!(json["created_at"].is_number());
    assert!(json["updated_at"].is_number());
}

#[tokio::test]
async fn test_create_and_get_portfolio() {
    let (engine, bond_store, portfolio_store) = create_all_test_resources();

    // Create portfolio
    let app1 = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let request_body = json!({
        "portfolio_id": "PORT002",
        "name": "Investment Portfolio",
        "currency": "EUR"
    });
    let (status, _) = post_json(app1, "/api/v1/portfolios", request_body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Get portfolio
    let app2 = create_router_with_stores(engine, bond_store, portfolio_store);
    let request = Request::builder()
        .uri("/api/v1/portfolios/PORT002")
        .body(Body::empty())
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["portfolio_id"], "PORT002");
    assert_eq!(json["name"], "Investment Portfolio");
    assert_eq!(json["currency"], "EUR");
}

#[tokio::test]
async fn test_get_portfolio_not_found() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .uri("/api/v1/portfolios/NONEXISTENT")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_create_portfolio_duplicate() {
    let (engine, bond_store, portfolio_store) = create_all_test_resources();

    // Create portfolio
    let app1 = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let request_body = json!({
        "portfolio_id": "DUPE_PORT",
        "name": "First Portfolio"
    });
    let (status, _) = post_json(app1, "/api/v1/portfolios", request_body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Try to create duplicate
    let app2 = create_router_with_stores(engine, bond_store, portfolio_store);
    let request_body = json!({
        "portfolio_id": "DUPE_PORT",
        "name": "Second Portfolio"
    });
    let (status, json) = post_json(app2, "/api/v1/portfolios", request_body).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(json["error"].as_str().unwrap().contains("already exists"));
}

#[tokio::test]
async fn test_update_portfolio() {
    let (engine, bond_store, portfolio_store) = create_all_test_resources();

    // Create portfolio
    let app1 = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let request_body = json!({
        "portfolio_id": "UPDATE_PORT",
        "name": "Original Name",
        "currency": "USD"
    });
    let (status, _) = post_json(app1, "/api/v1/portfolios", request_body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Update portfolio
    let app2 = create_router_with_stores(engine, bond_store, portfolio_store);
    let request = Request::builder()
        .method("PUT")
        .uri("/api/v1/portfolios/UPDATE_PORT")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&json!({
            "name": "Updated Name",
            "description": "New description"
        })).unwrap()))
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["name"], "Updated Name");
    assert_eq!(json["description"], "New description");
    assert_eq!(json["currency"], "USD"); // unchanged
}

#[tokio::test]
async fn test_update_portfolio_not_found() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .method("PUT")
        .uri("/api/v1/portfolios/NONEXISTENT")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&json!({
            "name": "New Name"
        })).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_portfolio() {
    let (engine, bond_store, portfolio_store) = create_all_test_resources();

    // Create portfolio
    let app1 = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let request_body = json!({
        "portfolio_id": "DELETE_PORT",
        "name": "To Delete"
    });
    let (status, _) = post_json(app1, "/api/v1/portfolios", request_body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Delete portfolio
    let app2 = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let request = Request::builder()
        .method("DELETE")
        .uri("/api/v1/portfolios/DELETE_PORT")
        .body(Body::empty())
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // Verify deleted
    let app3 = create_router_with_stores(engine, bond_store, portfolio_store);
    let request = Request::builder()
        .uri("/api/v1/portfolios/DELETE_PORT")
        .body(Body::empty())
        .unwrap();

    let response = app3.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_portfolio_not_found() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .method("DELETE")
        .uri("/api/v1/portfolios/NONEXISTENT")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_create_portfolio_with_positions() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio_id": "POS_PORT",
        "name": "Portfolio with Positions",
        "currency": "USD",
        "positions": [
            {
                "instrument_id": "BOND001",
                "notional": "1000000",
                "sector": "Financials",
                "rating": "A"
            },
            {
                "instrument_id": "BOND002",
                "notional": "500000",
                "sector": "Technology"
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/portfolios", request_body).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(json["positions"].as_array().unwrap().len(), 2);
    assert_eq!(json["positions"][0]["instrument_id"], "BOND001");
    assert_eq!(json["positions"][1]["instrument_id"], "BOND002");
}

#[tokio::test]
async fn test_add_position_to_portfolio() {
    let (engine, bond_store, portfolio_store) = create_all_test_resources();

    // Create portfolio
    let app1 = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let request_body = json!({
        "portfolio_id": "ADDPOS_PORT",
        "name": "Add Position Test",
        "positions": []
    });
    let (status, _) = post_json(app1, "/api/v1/portfolios", request_body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Add position
    let app2 = create_router_with_stores(engine, bond_store, portfolio_store);
    let position = json!({
        "instrument_id": "BOND003",
        "notional": "2000000",
        "sector": "Healthcare"
    });
    let (status, json) = post_json(app2, "/api/v1/portfolios/ADDPOS_PORT/positions", position).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["positions"].as_array().unwrap().len(), 1);
    assert_eq!(json["positions"][0]["instrument_id"], "BOND003");
}

#[tokio::test]
async fn test_remove_position_from_portfolio() {
    let (engine, bond_store, portfolio_store) = create_all_test_resources();

    // Create portfolio with positions
    let app1 = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let request_body = json!({
        "portfolio_id": "REMPOS_PORT",
        "name": "Remove Position Test",
        "positions": [
            { "instrument_id": "BOND_A", "notional": "1000000" },
            { "instrument_id": "BOND_B", "notional": "500000" }
        ]
    });
    let (status, _) = post_json(app1, "/api/v1/portfolios", request_body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Remove position
    let app2 = create_router_with_stores(engine, bond_store, portfolio_store);
    let request = Request::builder()
        .method("DELETE")
        .uri("/api/v1/portfolios/REMPOS_PORT/positions/BOND_A")
        .body(Body::empty())
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["positions"].as_array().unwrap().len(), 1);
    assert_eq!(json["positions"][0]["instrument_id"], "BOND_B");
}

#[tokio::test]
async fn test_update_position_in_portfolio() {
    let (engine, bond_store, portfolio_store) = create_all_test_resources();

    // Create portfolio with position
    let app1 = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let request_body = json!({
        "portfolio_id": "UPDPOS_PORT",
        "name": "Update Position Test",
        "positions": [
            { "instrument_id": "BOND_X", "notional": "1000000", "sector": "Financials" }
        ]
    });
    let (status, _) = post_json(app1, "/api/v1/portfolios", request_body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Update position
    let app2 = create_router_with_stores(engine, bond_store, portfolio_store);
    let request = Request::builder()
        .method("PUT")
        .uri("/api/v1/portfolios/UPDPOS_PORT/positions/BOND_X")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&json!({
            "instrument_id": "BOND_X",
            "notional": "2000000",
            "sector": "Technology",
            "rating": "AA"
        })).unwrap()))
        .unwrap();

    let response = app2.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["positions"][0]["notional"], "2000000");
    assert_eq!(json["positions"][0]["sector"], "Technology");
    assert_eq!(json["positions"][0]["rating"], "AA");
}

#[tokio::test]
async fn test_list_portfolios_with_pagination() {
    let (engine, bond_store, portfolio_store) = create_all_test_resources();

    // Create multiple portfolios
    for i in 0..5 {
        let app = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
        let request_body = json!({
            "portfolio_id": format!("PAGE_PORT{:03}", i),
            "name": format!("Portfolio {}", i),
            "currency": "USD"
        });
        let (status, _) = post_json(app, "/api/v1/portfolios", request_body).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    // Get first page
    let app = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let request = Request::builder()
        .uri("/api/v1/portfolios?limit=2&offset=0")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["portfolios"].as_array().unwrap().len(), 2);
    assert_eq!(json["total"].as_u64().unwrap(), 5);
    assert_eq!(json["limit"].as_u64().unwrap(), 2);
    assert_eq!(json["offset"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn test_list_portfolios_with_currency_filter() {
    let (engine, bond_store, portfolio_store) = create_all_test_resources();

    // Create portfolios with different currencies
    let app1 = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let (status, _) = post_json(app1, "/api/v1/portfolios", json!({
        "portfolio_id": "USD_PORT",
        "name": "USD Portfolio",
        "currency": "USD"
    })).await;
    assert_eq!(status, StatusCode::CREATED);

    let app2 = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let (status, _) = post_json(app2, "/api/v1/portfolios", json!({
        "portfolio_id": "EUR_PORT",
        "name": "EUR Portfolio",
        "currency": "EUR"
    })).await;
    assert_eq!(status, StatusCode::CREATED);

    // Filter by currency
    let app3 = create_router_with_stores(engine, bond_store, portfolio_store);
    let request = Request::builder()
        .uri("/api/v1/portfolios?currency=EUR")
        .body(Body::empty())
        .unwrap();

    let response = app3.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"].as_u64().unwrap(), 1);
    assert_eq!(json["portfolios"][0]["currency"], "EUR");
}

#[tokio::test]
async fn test_text_search_portfolios() {
    let (engine, bond_store, portfolio_store) = create_all_test_resources();

    // Create portfolios with specific names
    let app1 = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let (status, _) = post_json(app1, "/api/v1/portfolios", json!({
        "portfolio_id": "SEARCH_A",
        "name": "Global Equity Fund",
        "description": "Invests in global equities"
    })).await;
    assert_eq!(status, StatusCode::CREATED);

    let app2 = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let (status, _) = post_json(app2, "/api/v1/portfolios", json!({
        "portfolio_id": "SEARCH_B",
        "name": "Fixed Income Portfolio",
        "description": "Bond investments"
    })).await;
    assert_eq!(status, StatusCode::CREATED);

    // Search for "Equity"
    let app3 = create_router_with_stores(engine, bond_store, portfolio_store);
    let request = Request::builder()
        .uri("/api/v1/portfolios?q=Equity")
        .body(Body::empty())
        .unwrap();

    let response = app3.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"].as_u64().unwrap(), 1);
    assert!(json["portfolios"][0]["name"].as_str().unwrap().contains("Equity"));
}

#[tokio::test]
async fn test_batch_create_portfolios() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolios": [
            { "portfolio_id": "BATCH_PORT1", "name": "Batch Portfolio 1", "currency": "USD" },
            { "portfolio_id": "BATCH_PORT2", "name": "Batch Portfolio 2", "currency": "EUR" },
            { "portfolio_id": "BATCH_PORT3", "name": "Batch Portfolio 3", "currency": "GBP" }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/portfolios/batch", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["created"].as_u64().unwrap(), 3);
    assert_eq!(json["skipped"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn test_batch_create_portfolios_with_duplicates() {
    let (engine, bond_store, portfolio_store) = create_all_test_resources();

    // Create one portfolio first
    let app1 = create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let (status, _) = post_json(app1, "/api/v1/portfolios", json!({
        "portfolio_id": "BATCH_DUPE",
        "name": "First"
    })).await;
    assert_eq!(status, StatusCode::CREATED);

    // Batch create with duplicate
    let app2 = create_router_with_stores(engine, bond_store, portfolio_store);
    let request_body = json!({
        "portfolios": [
            { "portfolio_id": "BATCH_DUPE", "name": "Duplicate" },
            { "portfolio_id": "BATCH_NEW", "name": "New One" }
        ]
    });

    let (status, json) = post_json(app2, "/api/v1/portfolios/batch", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["created"].as_u64().unwrap(), 1);
    assert_eq!(json["skipped"].as_u64().unwrap(), 1);
}
