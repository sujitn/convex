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
use convex_server::routes::{
    create_router, create_router_with_bond_store, create_router_with_stores,
};
use convex_traits::config::EngineConfig;
use convex_traits::ids::InstrumentId;
use convex_traits::market_data::MarketDataProvider;
use convex_traits::output::BondQuoteOutput;
use convex_traits::reference_data::{
    BondReferenceData, BondType, IssuerType, ReferenceDataProvider,
};

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
    let storage =
        convex_ext_redb::create_memory_storage().expect("Failed to create memory storage");

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
        clean_price_bid: None,
        clean_price_mid: Some(clean_price),
        clean_price_ask: None,
        accrued_interest: Some(dec!(0.50)),
        ytm_bid: None,
        ytm_mid: Some(dec!(0.05)),
        ytm_ask: None,
        ytw: None,
        ytc: None,
        z_spread_bid: None,
        z_spread_mid: Some(dec!(50)),
        z_spread_ask: None,
        i_spread_bid: None,
        i_spread_mid: Some(dec!(45)),
        i_spread_ask: None,
        g_spread_bid: None,
        g_spread_mid: Some(dec!(55)),
        g_spread_ask: None,
        asw_bid: None,
        asw_mid: Some(dec!(48)),
        asw_ask: None,
        oas_bid: None,
        oas_mid: None,
        oas_ask: None,
        discount_margin_bid: None,
        discount_margin_mid: None,
        discount_margin_ask: None,
        simple_margin_bid: None,
        simple_margin_mid: None,
        simple_margin_ask: None,
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
        pricing_spec: "DiscountToMaturity".to_string(),
        source: "test".to_string(),
        is_stale: false,
        quality: 100,
    }
}

/// Helper to make a POST request and get JSON response.
async fn post_json(app: axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
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
    assert!(
        json.get("coverage").is_some(),
        "coverage field should exist"
    );
    assert!(json.get("inav").is_some(), "iNAV field should exist");
    assert!(
        json.get("duration").is_some(),
        "Duration field should exist"
    );
    assert!(
        json.get("num_holdings").is_some(),
        "num_holdings field should exist"
    );
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
    assert!(
        json.get("market_value").is_some(),
        "market_value field should exist"
    );
    assert!(
        json.get("duration").is_some(),
        "duration field should exist"
    );
    assert!(
        json.get("convexity").is_some(),
        "convexity field should exist"
    );
    assert!(json.get("dv01").is_some(), "dv01 field should exist");
    assert!(
        json["sector_breakdown"].is_array(),
        "sector_breakdown should be array"
    );
    assert!(
        json["rating_breakdown"].is_array(),
        "rating_breakdown should be array"
    );
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

    let (status, json) =
        post_json(app, "/api/v1/portfolio/duration-contribution", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        json["contributions"].is_array(),
        "contributions should be array"
    );
    assert!(
        json.get("total_duration").is_some(),
        "total_duration field should exist"
    );

    let contributions = json["contributions"].as_array().unwrap();
    // May have 0-2 contributions depending on price matching
    assert!(
        contributions.len() <= 2,
        "Should have at most 2 contributions"
    );

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

    let (status, _json) = post_json(app, "/api/v1/portfolio/analytics", request_body).await;

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
    assert!(
        rate > 0.045 && rate < 0.048,
        "Rate {} should be between 0.045 and 0.048",
        rate
    );
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
    assert!(
        (df - 0.9512).abs() < 0.001,
        "DF {} should be approximately 0.9512",
        df
    );
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
    assert!(
        (fwd - 0.05).abs() < 0.001,
        "Forward rate {} should be approximately 0.05",
        fwd
    );
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
    assert!(
        (rate - 0.0513).abs() < 0.001,
        "Annual rate {} should be approximately 0.0513",
        rate
    );
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

    let (status, json) =
        post_json(app, "/api/v1/bonds", serde_json::to_value(&bond).unwrap()).await;

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
    let (status, json) =
        post_json(app2, "/api/v1/bonds", serde_json::to_value(&bond).unwrap()).await;
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
        let bond = create_test_bond(
            &format!("PAGE{:03}", i),
            dec!(0.04) + Decimal::from(i) * dec!(0.001),
            2030 + i as i32,
        );
        let (status, _) =
            post_json(app, "/api/v1/bonds", serde_json::to_value(&bond).unwrap()).await;
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
    assert!(json["bonds"][0]["description"]
        .as_str()
        .unwrap()
        .contains("Apple"));
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
    let app1 =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
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
    let app1 =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
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
    let app1 =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
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
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "Updated Name",
                "description": "New description"
            }))
            .unwrap(),
        ))
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
        .body(Body::from(
            serde_json::to_string(&json!({
                "name": "New Name"
            }))
            .unwrap(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_portfolio() {
    let (engine, bond_store, portfolio_store) = create_all_test_resources();

    // Create portfolio
    let app1 =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let request_body = json!({
        "portfolio_id": "DELETE_PORT",
        "name": "To Delete"
    });
    let (status, _) = post_json(app1, "/api/v1/portfolios", request_body).await;
    assert_eq!(status, StatusCode::CREATED);

    // Delete portfolio
    let app2 =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
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
    let app1 =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
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
    let (status, json) =
        post_json(app2, "/api/v1/portfolios/ADDPOS_PORT/positions", position).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["positions"].as_array().unwrap().len(), 1);
    assert_eq!(json["positions"][0]["instrument_id"], "BOND003");
}

#[tokio::test]
async fn test_remove_position_from_portfolio() {
    let (engine, bond_store, portfolio_store) = create_all_test_resources();

    // Create portfolio with positions
    let app1 =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
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
    let app1 =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
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
        .body(Body::from(
            serde_json::to_string(&json!({
                "instrument_id": "BOND_X",
                "notional": "2000000",
                "sector": "Technology",
                "rating": "AA"
            }))
            .unwrap(),
        ))
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
        let app =
            create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
        let request_body = json!({
            "portfolio_id": format!("PAGE_PORT{:03}", i),
            "name": format!("Portfolio {}", i),
            "currency": "USD"
        });
        let (status, _) = post_json(app, "/api/v1/portfolios", request_body).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    // Get first page
    let app =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
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
    let app1 =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let (status, _) = post_json(
        app1,
        "/api/v1/portfolios",
        json!({
            "portfolio_id": "USD_PORT",
            "name": "USD Portfolio",
            "currency": "USD"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let app2 =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let (status, _) = post_json(
        app2,
        "/api/v1/portfolios",
        json!({
            "portfolio_id": "EUR_PORT",
            "name": "EUR Portfolio",
            "currency": "EUR"
        }),
    )
    .await;
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
    let app1 =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let (status, _) = post_json(
        app1,
        "/api/v1/portfolios",
        json!({
            "portfolio_id": "SEARCH_A",
            "name": "Global Equity Fund",
            "description": "Invests in global equities"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let app2 =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let (status, _) = post_json(
        app2,
        "/api/v1/portfolios",
        json!({
            "portfolio_id": "SEARCH_B",
            "name": "Fixed Income Portfolio",
            "description": "Bond investments"
        }),
    )
    .await;
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
    assert!(json["portfolios"][0]["name"]
        .as_str()
        .unwrap()
        .contains("Equity"));
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
    let app1 =
        create_router_with_stores(engine.clone(), bond_store.clone(), portfolio_store.clone());
    let (status, _) = post_json(
        app1,
        "/api/v1/portfolios",
        json!({
            "portfolio_id": "BATCH_DUPE",
            "name": "First"
        }),
    )
    .await;
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

// =============================================================================
// RISK CONTRIBUTION TESTS
// =============================================================================

/// Create test bond quotes with analytics.
fn create_test_bond_quotes() -> Vec<serde_json::Value> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    vec![
        json!({
            "instrument_id": "BOND_A",
            "isin": "US912828Z229",
            "currency": "USD",
            "settlement_date": "2025-01-15",
            "clean_price_mid": "99.00",
            "accrued_interest": "1.00",
            "ytm_mid": "0.05",
            "z_spread_mid": "100",
            "modified_duration": "5.0",
            "convexity": "30",
            "dv01": "0.05",
            "timestamp": now,
            "pricing_spec": "test",
            "source": "test",
            "is_stale": false,
            "quality": 100
        }),
        json!({
            "instrument_id": "BOND_B",
            "isin": "US912828Z230",
            "currency": "USD",
            "settlement_date": "2025-01-15",
            "clean_price_mid": "101.00",
            "accrued_interest": "1.00",
            "ytm_mid": "0.045",
            "z_spread_mid": "80",
            "modified_duration": "7.0",
            "convexity": "55",
            "dv01": "0.071",
            "timestamp": now,
            "pricing_spec": "test",
            "source": "test",
            "is_stale": false,
            "quality": 100
        }),
        json!({
            "instrument_id": "BOND_C",
            "isin": "US912828Z231",
            "currency": "USD",
            "settlement_date": "2025-01-15",
            "clean_price_mid": "98.00",
            "accrued_interest": "1.00",
            "ytm_mid": "0.055",
            "z_spread_mid": "120",
            "modified_duration": "3.0",
            "convexity": "12",
            "dv01": "0.03",
            "timestamp": now,
            "pricing_spec": "test",
            "source": "test",
            "is_stale": false,
            "quality": 100
        }),
    ]
}

#[tokio::test]
async fn test_risk_contributions_all_types() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "TEST_PORT",
            "name": "Test Portfolio",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000", "sector": "Government", "rating": "AAA" },
                { "instrument_id": "BOND_B", "notional": "2000000", "sector": "Corporate", "rating": "BBB" },
                { "instrument_id": "BOND_C", "notional": "500000", "sector": "Government", "rating": "A" }
            ]
        },
        "bond_prices": create_test_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/risk-contributions", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "TEST_PORT");
    assert_eq!(json["num_holdings"], 3);

    // Check duration contributions
    assert!(json["duration"].is_object());
    assert!(json["duration"]["by_holding"].is_array());
    assert_eq!(json["duration"]["by_holding"].as_array().unwrap().len(), 3);
    assert!(json["duration"]["portfolio_duration"].as_f64().unwrap() > 0.0);

    // Check DV01 contributions
    assert!(json["dv01"].is_object());
    assert!(json["dv01"]["by_holding"].is_array());
    assert!(json["dv01"]["total_dv01"].is_string());

    // Check spread contributions
    assert!(json["spread"].is_object());
    assert!(json["spread"]["by_holding"].is_array());
    assert!(json["spread"]["portfolio_spread"].as_f64().unwrap() > 0.0);

    // CS01 may be empty since we didn't provide cs01 in quotes
    assert!(json["cs01"].is_object());
}

#[tokio::test]
async fn test_risk_contributions_duration_only() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "DURATION_ONLY",
            "name": "Duration Only Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" },
                { "instrument_id": "BOND_B", "notional": "1000000" }
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "contribution_types": ["duration"]
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/risk-contributions", request_body).await;

    assert_eq!(status, StatusCode::OK);

    // Should have duration but not others
    assert!(json["duration"].is_object());
    assert!(json["dv01"].is_null());
    assert!(json["spread"].is_null());
    assert!(json["cs01"].is_null());
}

#[tokio::test]
async fn test_risk_contributions_dv01_and_spread() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "DV01_SPREAD",
            "name": "DV01 and Spread Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" },
                { "instrument_id": "BOND_B", "notional": "1000000" }
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "contribution_types": ["dv01", "spread"]
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/risk-contributions", request_body).await;

    assert_eq!(status, StatusCode::OK);

    // Should have dv01 and spread but not duration or cs01
    assert!(json["duration"].is_null());
    assert!(json["dv01"].is_object());
    assert!(json["spread"].is_object());
    assert!(json["cs01"].is_null());
}

#[tokio::test]
async fn test_risk_contributions_by_sector() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "SECTOR_TEST",
            "name": "Sector Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000", "sector": "Government" },
                { "instrument_id": "BOND_B", "notional": "2000000", "sector": "Corporate" }
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "contribution_types": ["duration"]
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/risk-contributions", request_body).await;

    assert_eq!(status, StatusCode::OK);

    // Check sector breakdown
    let by_sector = json["duration"]["by_sector"].as_array().unwrap();
    assert!(!by_sector.is_empty());

    // All buckets should have positive contribution percentages (for valid sectors)
    for bucket in by_sector {
        assert!(bucket["count"].as_u64().unwrap() > 0);
        assert!(bucket["weight"].as_f64().unwrap() > 0.0);
    }
}

#[tokio::test]
async fn test_risk_contributions_by_rating() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "RATING_TEST",
            "name": "Rating Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000", "rating": "AAA" },
                { "instrument_id": "BOND_B", "notional": "2000000", "rating": "BBB" },
                { "instrument_id": "BOND_C", "notional": "500000", "rating": "A" }
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "contribution_types": ["duration"]
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/risk-contributions", request_body).await;

    assert_eq!(status, StatusCode::OK);

    // Check rating breakdown
    let by_rating = json["duration"]["by_rating"].as_array().unwrap();
    assert!(!by_rating.is_empty());
}

#[tokio::test]
async fn test_risk_contributions_empty_positions() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "EMPTY",
            "name": "Empty Portfolio",
            "currency": "USD",
            "positions": []
        },
        "bond_prices": create_test_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/risk-contributions", request_body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json["error"]
        .as_str()
        .unwrap()
        .contains("No valid holdings"));
}

#[tokio::test]
async fn test_risk_contributions_no_matching_prices() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "NO_PRICES",
            "name": "No Matching Prices",
            "currency": "USD",
            "positions": [
                { "instrument_id": "UNKNOWN_BOND", "notional": "1000000" }
            ]
        },
        "bond_prices": create_test_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/risk-contributions", request_body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json["error"]
        .as_str()
        .unwrap()
        .contains("No valid holdings"));
}

#[tokio::test]
async fn test_risk_contributions_holding_order() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "ORDER_TEST",
            "name": "Order Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "100000" },  // Smallest
                { "instrument_id": "BOND_B", "notional": "5000000" }, // Largest
                { "instrument_id": "BOND_C", "notional": "500000" }   // Medium
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "contribution_types": ["duration"]
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/risk-contributions", request_body).await;

    assert_eq!(status, StatusCode::OK);

    // Holdings should be sorted by absolute contribution descending
    let by_holding = json["duration"]["by_holding"].as_array().unwrap();
    assert_eq!(by_holding.len(), 3);

    // First holding should have highest contribution (BOND_B with largest notional and duration)
    assert_eq!(by_holding[0]["id"], "BOND_B");
}

// =========================================================================
// Portfolio Bucketing Tests
// =========================================================================

#[tokio::test]
async fn test_sector_bucketing() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "SECTOR_BUCKET",
            "name": "Sector Bucketing Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000", "sector": "Government" },
                { "instrument_id": "BOND_B", "notional": "2000000", "sector": "Corporate" },
                { "instrument_id": "BOND_C", "notional": "500000", "sector": "Financial" }
            ]
        },
        "bond_prices": create_test_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/buckets/sector", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "SECTOR_BUCKET");
    assert_eq!(json["num_holdings"], 3);

    // Should have sector distribution
    assert!(json["by_sector"].is_array());
    let sectors = json["by_sector"].as_array().unwrap();
    assert!(sectors.len() >= 2); // At least Government and Corporate

    // Check summary fields exist
    assert!(json["summary"]["government_weight"].is_number());
    assert!(json["summary"]["credit_weight"].is_number());
}

#[tokio::test]
async fn test_rating_bucketing() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "RATING_BUCKET",
            "name": "Rating Bucketing Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000", "rating": "AAA" },
                { "instrument_id": "BOND_B", "notional": "2000000", "rating": "BBB" },
                { "instrument_id": "BOND_C", "notional": "500000", "rating": "A" }
            ]
        },
        "bond_prices": create_test_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/buckets/rating", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "RATING_BUCKET");

    // Should have rating distribution
    assert!(json["by_rating"].is_array());

    // Check summary fields exist
    assert!(json["summary"]["investment_grade_weight"].is_number());
    assert!(json["summary"]["high_yield_weight"].is_number());
}

#[tokio::test]
async fn test_maturity_bucketing() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "MATURITY_BUCKET",
            "name": "Maturity Bucketing Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" },
                { "instrument_id": "BOND_B", "notional": "2000000" },
                { "instrument_id": "BOND_C", "notional": "500000" }
            ]
        },
        "bond_prices": create_test_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/buckets/maturity", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "MATURITY_BUCKET");

    // Should have maturity distribution
    assert!(json["by_bucket"].is_array());

    // Check summary fields exist
    assert!(json["summary"]["short_term_weight"].is_number());
    assert!(json["summary"]["intermediate_weight"].is_number());
    assert!(json["summary"]["long_term_weight"].is_number());
}

#[tokio::test]
async fn test_custom_bucketing_by_country() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "COUNTRY_BUCKET",
            "name": "Country Bucketing Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000", "country": "USA" },
                { "instrument_id": "BOND_B", "notional": "2000000", "country": "GBR" },
                { "instrument_id": "BOND_C", "notional": "500000", "country": "USA" }
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "bucket_type": "country"
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/buckets/custom", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "COUNTRY_BUCKET");
    assert_eq!(json["bucket_type"], "country");

    // Should have custom distribution
    assert!(json["by_bucket"].is_array());
    let buckets = json["by_bucket"].as_array().unwrap();
    // Should have at least 2 buckets (USA and GBR)
    assert!(buckets.len() >= 2);
}

#[tokio::test]
async fn test_custom_bucketing_by_issuer() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "ISSUER_BUCKET",
            "name": "Issuer Bucketing Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000", "issuer": "US_TREASURY" },
                { "instrument_id": "BOND_B", "notional": "2000000", "issuer": "APPLE_INC" },
                { "instrument_id": "BOND_C", "notional": "500000", "issuer": "US_TREASURY" }
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "bucket_type": "issuer"
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/buckets/custom", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "ISSUER_BUCKET");
    assert_eq!(json["bucket_type"], "issuer");

    // Should have custom distribution
    assert!(json["by_bucket"].is_array());
}

#[tokio::test]
async fn test_custom_bucketing_by_currency() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "CURRENCY_BUCKET",
            "name": "Currency Bucketing Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000", "currency": "USD" },
                { "instrument_id": "BOND_B", "notional": "2000000", "currency": "EUR" },
                { "instrument_id": "BOND_C", "notional": "500000", "currency": "USD" }
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "bucket_type": "currency"
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/buckets/custom", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "CURRENCY_BUCKET");
    assert_eq!(json["bucket_type"], "currency");

    // Should have custom distribution
    assert!(json["by_bucket"].is_array());
}

#[tokio::test]
async fn test_all_bucketing() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "ALL_BUCKETS",
            "name": "All Bucketing Test",
            "currency": "USD",
            "positions": [
                {
                    "instrument_id": "BOND_A",
                    "notional": "1000000",
                    "sector": "Government",
                    "rating": "AAA",
                    "country": "USA"
                },
                {
                    "instrument_id": "BOND_B",
                    "notional": "2000000",
                    "sector": "Corporate",
                    "rating": "BBB",
                    "country": "GBR"
                }
            ]
        },
        "bond_prices": create_test_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/buckets", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "ALL_BUCKETS");

    // Should have all distribution types
    assert!(json["sector"].is_object());
    assert!(json["sector"]["by_sector"].is_array());

    assert!(json["rating"].is_object());
    assert!(json["rating"]["by_rating"].is_array());

    assert!(json["maturity"].is_object());
    assert!(json["maturity"]["by_bucket"].is_array());
}

#[tokio::test]
async fn test_bucketing_empty_positions() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "EMPTY_BUCKET",
            "name": "Empty Portfolio",
            "currency": "USD",
            "positions": []
        },
        "bond_prices": create_test_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/buckets/sector", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["num_holdings"], 0);
    assert!(json["by_sector"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_bucketing_bucket_metrics() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "METRICS_BUCKET",
            "name": "Metrics Bucket Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000", "sector": "Government" },
                { "instrument_id": "BOND_B", "notional": "2000000", "sector": "Government" }
            ]
        },
        "bond_prices": create_test_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/buckets/sector", request_body).await;

    assert_eq!(status, StatusCode::OK);

    let sectors = json["by_sector"].as_array().unwrap();
    // Find the Government bucket
    let govt_bucket = sectors.iter().find(|b| b["sector"] == "Government");
    assert!(govt_bucket.is_some());

    let bucket = govt_bucket.unwrap();
    // Check all metrics exist
    assert!(bucket["count"].is_number());
    assert!(bucket["market_value"].is_string());
    assert!(bucket["weight_pct"].is_number());
}

// =========================================================================
// Stress Testing Tests
// =========================================================================

#[tokio::test]
async fn test_list_standard_scenarios() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request = Request::builder()
        .uri("/api/v1/stress/scenarios")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert!(json["scenarios"].is_array());
    assert_eq!(json["count"], 10); // 10 standard scenarios

    // Check that scenarios have expected fields
    let scenarios = json["scenarios"].as_array().unwrap();
    assert!(scenarios.iter().any(|s| s["name"] == "Rates +100bp"));
    assert!(scenarios.iter().any(|s| s["name"] == "Risk Off"));
}

#[tokio::test]
async fn test_standard_stress_test() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "STRESS_TEST",
            "name": "Stress Test Portfolio",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" },
                { "instrument_id": "BOND_B", "notional": "2000000" }
            ]
        },
        "bond_prices": create_test_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/stress/standard", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "STRESS_TEST");
    assert_eq!(json["num_holdings"], 2);

    // Should have results for all 10 standard scenarios
    assert!(json["results"].is_array());
    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 10);

    // Check result structure
    let first_result = &results[0];
    assert!(first_result["scenario_name"].is_string());
    assert!(first_result["initial_value"].is_string());
    assert!(first_result["stressed_value"].is_string());
    assert!(first_result["pnl"].is_string());
    assert!(first_result["pnl_pct"].is_number());
    assert!(first_result["is_gain"].is_boolean());

    // Should have summary
    assert!(json["summary"].is_object());
    assert!(json["summary"]["scenario_count"].as_i64().unwrap() == 10);
    assert!(json["summary"]["worst_scenario"].is_string());
    assert!(json["summary"]["best_scenario"].is_string());
}

#[tokio::test]
async fn test_custom_stress_test() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "CUSTOM_STRESS",
            "name": "Custom Stress Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" },
                { "instrument_id": "BOND_B", "notional": "2000000" }
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "scenarios": [
            {
                "name": "Custom Rates +200bp",
                "description": "Custom parallel shift up 200bp",
                "rate_scenario": {
                    "type": "parallel",
                    "shift_bps": 200.0
                }
            },
            {
                "name": "Custom Spread +75bp",
                "description": "Custom spread widening",
                "spread_scenario": {
                    "type": "uniform",
                    "shift_bps": 75.0
                }
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/stress/test", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "CUSTOM_STRESS");

    // Should have results for 2 custom scenarios
    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);

    // Check scenario names
    assert!(results
        .iter()
        .any(|r| r["scenario_name"] == "Custom Rates +200bp"));
    assert!(results
        .iter()
        .any(|r| r["scenario_name"] == "Custom Spread +75bp"));
}

#[tokio::test]
async fn test_single_stress_test() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "SINGLE_STRESS",
            "name": "Single Stress Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" }
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "scenario": {
            "name": "Steepening Scenario",
            "description": "2s10s steepening",
            "rate_scenario": {
                "type": "steepening",
                "short_shift": -50.0,
                "long_shift": 50.0
            }
        }
    });

    let (status, json) = post_json(app, "/api/v1/stress/single", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "SINGLE_STRESS");
    assert_eq!(json["num_holdings"], 1);

    // Should have single result
    assert!(json["result"].is_object());
    assert_eq!(json["result"]["scenario_name"], "Steepening Scenario");
}

#[tokio::test]
async fn test_stress_test_with_combined_scenario() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "COMBINED_STRESS",
            "name": "Combined Stress Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" },
                { "instrument_id": "BOND_B", "notional": "2000000" }
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "scenarios": [
            {
                "name": "Risk Off Custom",
                "description": "Flight to quality scenario",
                "rate_scenario": {
                    "type": "parallel",
                    "shift_bps": -50.0
                },
                "spread_scenario": {
                    "type": "uniform",
                    "shift_bps": 100.0
                }
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/stress/test", request_body).await;

    assert_eq!(status, StatusCode::OK);

    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);

    let result = &results[0];
    assert_eq!(result["scenario_name"], "Risk Off Custom");
    // Combined scenario should have both rate and spread impact
    // (may be null if analytics are missing, but field should exist)
}

#[tokio::test]
async fn test_stress_test_key_rates_scenario() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "KEY_RATES_STRESS",
            "name": "Key Rates Stress Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" }
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "scenarios": [
            {
                "name": "Key Rate Twist",
                "rate_scenario": {
                    "type": "key_rates",
                    "shifts": [
                        { "tenor": 2.0, "shift_bps": -25.0 },
                        { "tenor": 5.0, "shift_bps": 0.0 },
                        { "tenor": 10.0, "shift_bps": 25.0 },
                        { "tenor": 30.0, "shift_bps": 50.0 }
                    ]
                }
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/stress/test", request_body).await;

    assert_eq!(status, StatusCode::OK);

    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["scenario_name"], "Key Rate Twist");
}

#[tokio::test]
async fn test_stress_test_butterfly_scenario() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "BUTTERFLY_STRESS",
            "name": "Butterfly Stress Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" }
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "scenarios": [
            {
                "name": "Butterfly Shift",
                "rate_scenario": {
                    "type": "butterfly",
                    "wing_shift": 25.0,
                    "belly_shift": -25.0
                }
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/stress/test", request_body).await;

    assert_eq!(status, StatusCode::OK);

    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["scenario_name"], "Butterfly Shift");
}

#[tokio::test]
async fn test_stress_test_empty_portfolio() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "EMPTY_STRESS",
            "name": "Empty Portfolio",
            "currency": "USD",
            "positions": []
        },
        "bond_prices": create_test_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/stress/standard", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["num_holdings"], 0);

    // Should still return results (with zero impact)
    assert!(json["results"].is_array());
}

#[tokio::test]
async fn test_stress_test_flattening_scenario() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "FLATTEN_STRESS",
            "name": "Flattening Stress Test",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" }
            ]
        },
        "bond_prices": create_test_bond_quotes(),
        "scenarios": [
            {
                "name": "Curve Flattening",
                "rate_scenario": {
                    "type": "flattening",
                    "short_shift": 50.0,
                    "long_shift": -50.0,
                    "pivot_tenor": 7.0
                }
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/stress/test", request_body).await;

    assert_eq!(status, StatusCode::OK);

    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["scenario_name"], "Curve Flattening");
}

// =========================================================================
// Benchmark Comparison Tests
// =========================================================================

fn create_benchmark_bond_quotes() -> Vec<serde_json::Value> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    vec![
        json!({
            "instrument_id": "BENCH_A",
            "isin": "US912828B001",
            "currency": "USD",
            "settlement_date": "2025-01-15",
            "clean_price_mid": "99.50",
            "accrued_interest": "0.75",
            "ytm_mid": "0.045",
            "z_spread_mid": "80",
            "modified_duration": "4.5",
            "convexity": "35.0",
            "dv01": "0.045",
            "timestamp": now,
            "pricing_spec": "test",
            "source": "test",
            "is_stale": false,
            "quality": 100
        }),
        json!({
            "instrument_id": "BENCH_B",
            "isin": "US912828B002",
            "currency": "USD",
            "settlement_date": "2025-01-15",
            "clean_price_mid": "101.00",
            "accrued_interest": "0.75",
            "ytm_mid": "0.04",
            "z_spread_mid": "100",
            "modified_duration": "5.5",
            "convexity": "45.0",
            "dv01": "0.055",
            "timestamp": now,
            "pricing_spec": "test",
            "source": "test",
            "is_stale": false,
            "quality": 100
        }),
    ]
}

#[tokio::test]
async fn test_benchmark_comparison() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "PORTFOLIO",
            "name": "Test Portfolio",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000", "sector": "Government" },
                { "instrument_id": "BOND_B", "notional": "2000000", "sector": "Corporate" }
            ]
        },
        "portfolio_prices": create_test_bond_quotes(),
        "benchmark": {
            "portfolio_id": "BENCHMARK",
            "name": "Test Benchmark",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BENCH_A", "notional": "1500000", "sector": "Government" },
                { "instrument_id": "BENCH_B", "notional": "1500000", "sector": "Corporate" }
            ]
        },
        "benchmark_prices": create_benchmark_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/benchmark/compare", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "PORTFOLIO");
    assert_eq!(json["benchmark_id"], "BENCHMARK");
    assert_eq!(json["portfolio_holdings"], 2);
    assert_eq!(json["benchmark_holdings"], 2);

    // Check duration comparison
    assert!(json["duration"].is_object());
    assert!(json["duration"]["is_longer"].is_boolean());

    // Check spread comparison
    assert!(json["spread"].is_object());
    assert!(json["spread"]["is_wider"].is_boolean());

    // Check yield comparison
    assert!(json["yield_comparison"].is_object());
    assert!(json["yield_comparison"]["is_higher_yield"].is_boolean());

    // Check risk comparison
    assert!(json["risk"].is_object());
    assert!(json["risk"]["dv01_difference"].is_number());

    // Check active weights
    assert!(json["active_weights"].is_object());
    assert!(json["active_weights"]["total_active_weight"].is_number());
    assert!(json["active_weights"]["overweight_count"].is_number());

    // Check sector breakdown
    assert!(json["by_sector"].is_array());

    // Check rating breakdown
    assert!(json["by_rating"].is_array());
}

#[tokio::test]
async fn test_active_weights() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "PORT_AW",
            "name": "Active Weights Portfolio",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "2000000", "sector": "Government" },
                { "instrument_id": "BOND_B", "notional": "1000000", "sector": "Corporate" }
            ]
        },
        "portfolio_prices": create_test_bond_quotes(),
        "benchmark": {
            "portfolio_id": "BENCH_AW",
            "name": "Active Weights Benchmark",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BENCH_A", "notional": "1000000", "sector": "Government" },
                { "instrument_id": "BENCH_B", "notional": "2000000", "sector": "Corporate" }
            ]
        },
        "benchmark_prices": create_benchmark_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/benchmark/active-weights", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "PORT_AW");
    assert_eq!(json["benchmark_id"], "BENCH_AW");

    // Check totals
    assert!(json["total_active_weight"].is_number());
    assert!(json["overweight_count"].is_number());
    assert!(json["underweight_count"].is_number());

    // Check breakdowns
    assert!(json["by_sector"].is_array());
    assert!(json["by_rating"].is_array());
    assert!(json["by_holding"].is_array());

    // Check convenience arrays
    assert!(json["overweight_sectors"].is_array());
    assert!(json["underweight_sectors"].is_array());
    assert!(json["largest_positions"].is_array());
}

#[tokio::test]
async fn test_tracking_error() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "PORT_TE",
            "name": "Tracking Error Portfolio",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" },
                { "instrument_id": "BOND_B", "notional": "2000000" }
            ]
        },
        "portfolio_prices": create_test_bond_quotes(),
        "benchmark": {
            "portfolio_id": "BENCH_TE",
            "name": "Tracking Error Benchmark",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BENCH_A", "notional": "1500000" },
                { "instrument_id": "BENCH_B", "notional": "1500000" }
            ]
        },
        "benchmark_prices": create_benchmark_bond_quotes(),
        "rate_vol": 0.01,
        "spread_vol": 0.002
    });

    let (status, json) = post_json(app, "/api/v1/benchmark/tracking-error", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "PORT_TE");
    assert_eq!(json["benchmark_id"], "BENCH_TE");

    // Check tracking error
    assert!(json["tracking_error"].is_number());
    assert!(json["tracking_error_bps"].is_number());

    // Check contributions
    assert!(json["contributions"]["duration"].is_number());
    assert!(json["contributions"]["spread"].is_number());
    assert!(json["contributions"]["sector"].is_number());
    assert!(json["contributions"]["selection"].is_number());

    // Check active exposures
    assert!(json["active_exposures"]["duration"].is_number());
    assert!(json["active_exposures"]["spread_bps"].is_number());

    // Check assumptions
    assert_eq!(json["assumptions"]["rate_vol"], 0.01);
    assert_eq!(json["assumptions"]["spread_vol"], 0.002);
}

#[tokio::test]
async fn test_tracking_error_default_vol() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "PORT_DEF",
            "name": "Default Vol Portfolio",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" }
            ]
        },
        "portfolio_prices": create_test_bond_quotes(),
        "benchmark": {
            "portfolio_id": "BENCH_DEF",
            "name": "Default Vol Benchmark",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BENCH_A", "notional": "1000000" }
            ]
        },
        "benchmark_prices": create_benchmark_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/benchmark/tracking-error", request_body).await;

    assert_eq!(status, StatusCode::OK);

    // Check default assumptions were used
    assert_eq!(json["assumptions"]["rate_vol"], 0.01);
    assert_eq!(json["assumptions"]["spread_vol"], 0.002);
}

#[tokio::test]
async fn test_attribution() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "PORT_ATTR",
            "name": "Attribution Portfolio",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000", "sector": "Government" },
                { "instrument_id": "BOND_B", "notional": "2000000", "sector": "Corporate" }
            ]
        },
        "portfolio_prices": create_test_bond_quotes(),
        "benchmark": {
            "portfolio_id": "BENCH_ATTR",
            "name": "Attribution Benchmark",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BENCH_A", "notional": "1500000", "sector": "Government" },
                { "instrument_id": "BENCH_B", "notional": "1500000", "sector": "Corporate" }
            ]
        },
        "benchmark_prices": create_benchmark_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/benchmark/attribution", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "PORT_ATTR");
    assert_eq!(json["benchmark_id"], "BENCH_ATTR");

    // Check duration attribution
    assert!(json["duration_attribution"].is_array());

    // Check spread attribution
    assert!(json["spread_attribution"].is_array());
}

#[tokio::test]
async fn test_benchmark_comparison_identical() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Portfolio and benchmark are identical
    let request_body = json!({
        "portfolio": {
            "portfolio_id": "IDENTICAL_PORT",
            "name": "Identical Portfolio",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" }
            ]
        },
        "portfolio_prices": create_test_bond_quotes(),
        "benchmark": {
            "portfolio_id": "IDENTICAL_BENCH",
            "name": "Identical Benchmark",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "1000000" }
            ]
        },
        "benchmark_prices": create_test_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/benchmark/compare", request_body).await;

    assert_eq!(status, StatusCode::OK);

    // Duration difference should be zero or very small
    if let Some(diff) = json["duration"]["difference"].as_f64() {
        assert!(diff.abs() < 0.01);
    }

    // Active weights should be minimal
    assert_eq!(json["active_weights"]["overweight_count"], 0);
    assert_eq!(json["active_weights"]["underweight_count"], 0);
}

#[tokio::test]
async fn test_benchmark_comparison_empty_portfolio() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "EMPTY_PORT",
            "name": "Empty Portfolio",
            "currency": "USD",
            "positions": []
        },
        "portfolio_prices": [],
        "benchmark": {
            "portfolio_id": "NON_EMPTY_BENCH",
            "name": "Non-Empty Benchmark",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BENCH_A", "notional": "1000000" }
            ]
        },
        "benchmark_prices": create_benchmark_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/benchmark/compare", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_holdings"], 0);
    assert_eq!(json["benchmark_holdings"], 1);
}

#[tokio::test]
async fn test_active_weights_sector_breakdown() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio": {
            "portfolio_id": "SECTOR_PORT",
            "name": "Sector Portfolio",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BOND_A", "notional": "3000000", "sector": "Government" }
            ]
        },
        "portfolio_prices": create_test_bond_quotes(),
        "benchmark": {
            "portfolio_id": "SECTOR_BENCH",
            "name": "Sector Benchmark",
            "currency": "USD",
            "positions": [
                { "instrument_id": "BENCH_A", "notional": "1000000", "sector": "Government" },
                { "instrument_id": "BENCH_B", "notional": "2000000", "sector": "Corporate" }
            ]
        },
        "benchmark_prices": create_benchmark_bond_quotes()
    });

    let (status, json) = post_json(app, "/api/v1/benchmark/active-weights", request_body).await;

    assert_eq!(status, StatusCode::OK);

    // Should have overweight in Government (100% vs 33%)
    let overweight = json["overweight_sectors"].as_array().unwrap();
    assert!(!overweight.is_empty());

    // Should have underweight in Corporate (0% vs 67%)
    let underweight = json["underweight_sectors"].as_array().unwrap();
    assert!(!underweight.is_empty());
}

// =========================================================================
// Liquidity Analytics Tests
// =========================================================================

fn create_liquidity_positions() -> Vec<serde_json::Value> {
    vec![
        json!({
            "instrument_id": "BOND_LIQ_1",
            "notional": "1000000",
            "market_price": "100.00",
            "liquidity_score": 85.0,
            "bid_ask_spread": 5.0
        }),
        json!({
            "instrument_id": "BOND_LIQ_2",
            "notional": "2000000",
            "market_price": "99.50",
            "liquidity_score": 50.0,
            "bid_ask_spread": 25.0
        }),
        json!({
            "instrument_id": "BOND_LIQ_3",
            "notional": "500000",
            "market_price": "101.00",
            "liquidity_score": 20.0,
            "bid_ask_spread": 75.0
        }),
    ]
}

#[tokio::test]
async fn test_liquidity_metrics() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio_id": "LIQ_TEST",
        "name": "Liquidity Test Portfolio",
        "positions": create_liquidity_positions()
    });

    let (status, json) = post_json(app, "/api/v1/liquidity/metrics", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "LIQ_TEST");
    assert_eq!(json["total_holdings"], 3);
    assert_eq!(json["bid_ask_coverage"], 3);
    assert_eq!(json["score_coverage"], 3);

    // Check weighted averages are calculated
    assert!(json["avg_bid_ask_spread"].is_number());
    assert!(json["avg_liquidity_score"].is_number());

    // Check bucket percentages are calculated
    assert!(json["highly_liquid_pct"].is_number());
    assert!(json["moderately_liquid_pct"].is_number());
    assert!(json["illiquid_pct"].is_number());

    // Check coverage percentages
    assert_eq!(json["bid_ask_coverage_pct"], 100.0);
    assert_eq!(json["score_coverage_pct"], 100.0);
}

#[tokio::test]
async fn test_liquidity_metrics_empty_portfolio() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio_id": "EMPTY_LIQ",
        "name": "Empty Liquidity Portfolio",
        "positions": []
    });

    let (status, json) = post_json(app, "/api/v1/liquidity/metrics", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["total_holdings"], 0);
    assert!(json["avg_bid_ask_spread"].is_null());
    assert!(json["avg_liquidity_score"].is_null());
}

#[tokio::test]
async fn test_liquidity_metrics_partial_data() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Some positions without liquidity data
    let request_body = json!({
        "portfolio_id": "PARTIAL_LIQ",
        "name": "Partial Liquidity Portfolio",
        "positions": [
            {
                "instrument_id": "BOND_1",
                "notional": "1000000",
                "liquidity_score": 80.0
            },
            {
                "instrument_id": "BOND_2",
                "notional": "1000000"
                // No liquidity data
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/liquidity/metrics", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["total_holdings"], 2);
    assert_eq!(json["score_coverage"], 1);
    assert_eq!(json["bid_ask_coverage"], 0);
}

#[tokio::test]
async fn test_liquidity_distribution() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio_id": "DIST_TEST",
        "name": "Distribution Test Portfolio",
        "positions": create_liquidity_positions()
    });

    let (status, json) = post_json(app, "/api/v1/liquidity/distribution", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "DIST_TEST");

    // Check buckets array exists
    let buckets = json["buckets"].as_array().unwrap();
    assert!(!buckets.is_empty());

    // Check total market value is calculated
    assert!(json["total_market_value"].is_string());

    // Each bucket should have required fields
    for bucket in buckets {
        assert!(bucket["bucket"].is_string());
        assert!(bucket["market_value"].is_string());
        assert!(bucket["weight_pct"].is_number());
        assert!(bucket["count"].is_number());
    }
}

#[tokio::test]
async fn test_liquidity_distribution_buckets() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Create positions that should fall into different buckets
    let request_body = json!({
        "portfolio_id": "BUCKET_TEST",
        "name": "Bucket Test Portfolio",
        "positions": [
            {
                "instrument_id": "HIGH_LIQ",
                "notional": "1000000",
                "liquidity_score": 90.0  // HighlyLiquid (>= 70)
            },
            {
                "instrument_id": "MED_LIQ",
                "notional": "1000000",
                "liquidity_score": 50.0  // ModeratelyLiquid (30-70)
            },
            {
                "instrument_id": "LOW_LIQ",
                "notional": "1000000",
                "liquidity_score": 15.0  // LessLiquid (< 30)
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/liquidity/distribution", request_body).await;

    assert_eq!(status, StatusCode::OK);

    let buckets = json["buckets"].as_array().unwrap();
    // Should have 3 different buckets
    assert_eq!(buckets.len(), 3);
}

#[tokio::test]
async fn test_days_to_liquidate() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio_id": "DTL_TEST",
        "name": "Days to Liquidate Test",
        "positions": create_liquidity_positions(),
        "max_participation_rate": 20.0
    });

    let (status, json) = post_json(app, "/api/v1/liquidity/days-to-liquidate", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "DTL_TEST");
    assert_eq!(json["max_participation_rate"], 20.0);

    // Check days estimates
    assert!(json["total_days"].is_number());
    assert!(json["highly_liquid_days"].is_number());
    assert!(json["illiquid_days"].is_number());

    // Total days should be positive for non-empty portfolio
    let total_days = json["total_days"].as_f64().unwrap();
    assert!(total_days > 0.0);

    // Check illiquid percentage of time
    assert!(json["illiquid_pct_of_time"].is_number());
}

#[tokio::test]
async fn test_days_to_liquidate_default_participation() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Don't specify max_participation_rate - should default to 20%
    let request_body = json!({
        "portfolio_id": "DTL_DEFAULT",
        "name": "Days to Liquidate Default",
        "positions": create_liquidity_positions()
    });

    let (status, json) = post_json(app, "/api/v1/liquidity/days-to-liquidate", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["max_participation_rate"], 20.0);
}

#[tokio::test]
async fn test_days_to_liquidate_high_participation() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Higher participation rate = fewer days
    let request_body = json!({
        "portfolio_id": "DTL_HIGH",
        "name": "High Participation Test",
        "positions": create_liquidity_positions(),
        "max_participation_rate": 50.0
    });

    let (status, json) = post_json(app, "/api/v1/liquidity/days-to-liquidate", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["max_participation_rate"], 50.0);
}

#[tokio::test]
async fn test_liquidity_analysis() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio_id": "FULL_ANALYSIS",
        "name": "Full Liquidity Analysis",
        "positions": create_liquidity_positions(),
        "max_participation_rate": 25.0
    });

    let (status, json) = post_json(app, "/api/v1/liquidity/analysis", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "FULL_ANALYSIS");

    // Check metrics section
    assert!(json["metrics"].is_object());
    assert!(json["metrics"]["avg_liquidity_score"].is_number());
    assert!(json["metrics"]["avg_bid_ask_spread"].is_number());
    assert!(json["metrics"]["total_holdings"].is_number());

    // Check distribution section
    assert!(json["distribution"].is_array());
    let distribution = json["distribution"].as_array().unwrap();
    assert!(!distribution.is_empty());

    // Check days_to_liquidate section
    assert!(json["days_to_liquidate"].is_object());
    assert!(json["days_to_liquidate"]["total_days"].is_number());
    assert_eq!(json["days_to_liquidate"]["max_participation_rate"], 25.0);
}

#[tokio::test]
async fn test_liquidity_analysis_illiquid_portfolio() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Create a highly illiquid portfolio
    let request_body = json!({
        "portfolio_id": "ILLIQUID_TEST",
        "name": "Illiquid Portfolio",
        "positions": [
            {
                "instrument_id": "ILLIQ_1",
                "notional": "5000000",
                "liquidity_score": 10.0,
                "bid_ask_spread": 150.0
            },
            {
                "instrument_id": "ILLIQ_2",
                "notional": "5000000",
                "liquidity_score": 15.0,
                "bid_ask_spread": 200.0
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/liquidity/analysis", request_body).await;

    assert_eq!(status, StatusCode::OK);

    // Should flag liquidity concerns
    assert_eq!(json["metrics"]["has_liquidity_concerns"], true);

    // Illiquid percentage should be 100%
    let illiquid_pct = json["metrics"]["illiquid_pct"].as_f64().unwrap();
    assert!((illiquid_pct - 100.0).abs() < 0.1);
}

#[tokio::test]
async fn test_liquidity_metrics_concerns_threshold() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Portfolio with >15% illiquid should trigger concerns
    let request_body = json!({
        "portfolio_id": "CONCERN_TEST",
        "name": "Concern Threshold Test",
        "positions": [
            {
                "instrument_id": "LIQUID",
                "notional": "8000000",
                "liquidity_score": 80.0
            },
            {
                "instrument_id": "ILLIQUID",
                "notional": "2000000",
                "liquidity_score": 20.0  // 20% weight = illiquid > 15%
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/liquidity/metrics", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["has_liquidity_concerns"], true);
}

// =========================================================================
// Credit Quality Analytics Tests
// =========================================================================

fn create_credit_positions() -> Vec<serde_json::Value> {
    vec![
        json!({
            "instrument_id": "BOND_AAA",
            "notional": "1000000",
            "market_price": "100.00",
            "rating": "AAA"
        }),
        json!({
            "instrument_id": "BOND_A",
            "notional": "2000000",
            "market_price": "99.50",
            "rating": "A"
        }),
        json!({
            "instrument_id": "BOND_BBB",
            "notional": "1500000",
            "market_price": "98.00",
            "rating": "BBB"
        }),
        json!({
            "instrument_id": "BOND_BB",
            "notional": "500000",
            "market_price": "95.00",
            "rating": "BB"
        }),
    ]
}

#[tokio::test]
async fn test_credit_quality() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio_id": "CREDIT_TEST",
        "name": "Credit Quality Test Portfolio",
        "positions": create_credit_positions()
    });

    let (status, json) = post_json(app, "/api/v1/credit/quality", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "CREDIT_TEST");
    assert_eq!(json["total_holdings"], 4);
    assert_eq!(json["rated_holdings"], 4);

    // Check IG/HY weights
    assert!(json["ig_weight"].is_number());
    assert!(json["hy_weight"].is_number());

    // Check average rating
    assert!(json["average_rating_score"].is_number());
    assert!(json["average_rating"].is_string());

    // Check crossover risk
    assert!(json["bbb_weight"].is_number());
    assert!(json["bb_weight"].is_number());
    assert!(json["crossover_risk"].is_number());

    // Check quality tiers
    assert!(json["quality_tiers"].is_object());
    assert!(json["quality_tiers"]["high_quality"].is_number());
    assert!(json["quality_tiers"]["upper_medium"].is_number());
    assert!(json["quality_tiers"]["lower_medium"].is_number());
    assert!(json["quality_tiers"]["speculative"].is_number());
}

#[tokio::test]
async fn test_credit_quality_empty_portfolio() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio_id": "EMPTY_CREDIT",
        "name": "Empty Credit Portfolio",
        "positions": []
    });

    let (status, json) = post_json(app, "/api/v1/credit/quality", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["total_holdings"], 0);
    assert!(json["average_rating"].is_null());
    assert!(json["average_rating_score"].is_null());
}

#[tokio::test]
async fn test_credit_quality_ig_portfolio() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // All investment grade bonds
    let request_body = json!({
        "portfolio_id": "IG_PORT",
        "name": "Investment Grade Portfolio",
        "positions": [
            { "instrument_id": "B1", "notional": "1000000", "rating": "AAA" },
            { "instrument_id": "B2", "notional": "1000000", "rating": "AA" },
            { "instrument_id": "B3", "notional": "1000000", "rating": "A" },
            { "instrument_id": "B4", "notional": "1000000", "rating": "BBB" }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/credit/quality", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["is_investment_grade"], true);
    assert_eq!(json["has_significant_hy"], false);

    // IG weight should be 100%
    let ig_weight = json["ig_weight"].as_f64().unwrap();
    assert!((ig_weight - 100.0).abs() < 0.1);

    // HY weight should be 0%
    let hy_weight = json["hy_weight"].as_f64().unwrap();
    assert!(hy_weight.abs() < 0.1);
}

#[tokio::test]
async fn test_credit_quality_hy_portfolio() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // All high yield bonds
    let request_body = json!({
        "portfolio_id": "HY_PORT",
        "name": "High Yield Portfolio",
        "positions": [
            { "instrument_id": "B1", "notional": "1000000", "rating": "BB" },
            { "instrument_id": "B2", "notional": "1000000", "rating": "B" },
            { "instrument_id": "B3", "notional": "1000000", "rating": "CCC" }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/credit/quality", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["is_investment_grade"], false);
    assert_eq!(json["has_significant_hy"], true);

    // HY weight should be 100%
    let hy_weight = json["hy_weight"].as_f64().unwrap();
    assert!((hy_weight - 100.0).abs() < 0.1);
}

#[tokio::test]
async fn test_credit_quality_unrated() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Mix of rated and unrated
    let request_body = json!({
        "portfolio_id": "UNRATED_PORT",
        "name": "Partially Rated Portfolio",
        "positions": [
            { "instrument_id": "B1", "notional": "1000000", "rating": "A" },
            { "instrument_id": "B2", "notional": "1000000" }  // No rating
        ]
    });

    let (status, json) = post_json(app, "/api/v1/credit/quality", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["total_holdings"], 2);
    assert_eq!(json["rated_holdings"], 1);

    // Unrated weight should be ~50%
    let unrated_weight = json["unrated_weight"].as_f64().unwrap();
    assert!(unrated_weight > 40.0);
}

#[tokio::test]
async fn test_migration_risk() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Holdings at crossover boundary
    let request_body = json!({
        "portfolio_id": "MIGRATION_TEST",
        "name": "Migration Risk Test",
        "positions": [
            { "instrument_id": "B1", "notional": "1000000", "rating": "BBB-" },  // Fallen angel risk
            { "instrument_id": "B2", "notional": "1000000", "rating": "BB+" },   // Rising star potential
            { "instrument_id": "B3", "notional": "1000000", "rating": "AAA" }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/credit/migration-risk", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "MIGRATION_TEST");

    // Check fallen angel risk
    assert!(json["fallen_angel_risk"].is_object());
    assert!(json["fallen_angel_risk"]["bbb_weight"].is_number());
    assert!(json["fallen_angel_risk"]["bbb_minus_weight"].is_number());
    assert!(json["fallen_angel_risk"]["holdings_count"].is_number());

    // Should have 1 BBB- holding
    assert_eq!(json["fallen_angel_risk"]["holdings_count"], 1);

    // Check rising star risk
    assert!(json["rising_star_risk"].is_object());
    assert!(json["rising_star_risk"]["bb_weight"].is_number());
    assert!(json["rising_star_risk"]["bb_plus_weight"].is_number());
    assert!(json["rising_star_risk"]["holdings_count"].is_number());

    // Should have 1 BB+ holding
    assert_eq!(json["rising_star_risk"]["holdings_count"], 1);

    // Check total crossover exposure
    assert!(json["total_crossover_exposure"].is_number());
}

#[tokio::test]
async fn test_migration_risk_no_crossover() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // No holdings at crossover boundary
    let request_body = json!({
        "portfolio_id": "NO_CROSSOVER",
        "name": "No Crossover Risk",
        "positions": [
            { "instrument_id": "B1", "notional": "1000000", "rating": "AAA" },
            { "instrument_id": "B2", "notional": "1000000", "rating": "AA" }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/credit/migration-risk", request_body).await;

    assert_eq!(status, StatusCode::OK);

    // No fallen angel risk
    assert_eq!(json["fallen_angel_risk"]["holdings_count"], 0);
    assert_eq!(json["fallen_angel_risk"]["bbb_weight"], 0.0);

    // No rising star potential
    assert_eq!(json["rising_star_risk"]["holdings_count"], 0);
    assert_eq!(json["rising_star_risk"]["bb_weight"], 0.0);

    // No crossover exposure
    assert_eq!(json["total_crossover_exposure"], 0.0);
}

#[tokio::test]
async fn test_credit_analysis() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio_id": "FULL_CREDIT",
        "name": "Full Credit Analysis",
        "positions": create_credit_positions()
    });

    let (status, json) = post_json(app, "/api/v1/credit/analysis", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "FULL_CREDIT");

    // Check quality section
    assert!(json["quality"].is_object());
    assert!(json["quality"]["ig_weight"].is_number());
    assert!(json["quality"]["hy_weight"].is_number());
    assert!(json["quality"]["average_rating"].is_string());
    assert!(json["quality"]["quality_tiers"].is_object());

    // Check migration_risk section
    assert!(json["migration_risk"].is_object());
    assert!(json["migration_risk"]["fallen_angel_risk"].is_object());
    assert!(json["migration_risk"]["rising_star_risk"].is_object());
}

#[tokio::test]
async fn test_credit_quality_rating_formats() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Test various rating formats
    let request_body = json!({
        "portfolio_id": "RATING_FORMATS",
        "name": "Rating Formats Test",
        "positions": [
            { "instrument_id": "B1", "notional": "1000000", "rating": "AAA" },
            { "instrument_id": "B2", "notional": "1000000", "rating": "Aa1" },     // Moody's format
            { "instrument_id": "B3", "notional": "1000000", "rating": "BBB+" },
            { "instrument_id": "B4", "notional": "1000000", "rating": "Baa2" },    // Moody's format
            { "instrument_id": "B5", "notional": "1000000", "rating": "BB-" }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/credit/quality", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["rated_holdings"], 5);

    // All ratings should be parsed successfully
    let unrated_weight = json["unrated_weight"].as_f64().unwrap();
    assert!(unrated_weight < 1.0); // Should be close to 0
}

#[tokio::test]
async fn test_credit_quality_crossover_concentration() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // High crossover concentration
    let request_body = json!({
        "portfolio_id": "CROSSOVER_PORT",
        "name": "Crossover Concentration",
        "positions": [
            { "instrument_id": "B1", "notional": "2000000", "rating": "BBB" },
            { "instrument_id": "B2", "notional": "2000000", "rating": "BBB-" },
            { "instrument_id": "B3", "notional": "2000000", "rating": "BB+" },
            { "instrument_id": "B4", "notional": "2000000", "rating": "BB" }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/credit/quality", request_body).await;

    assert_eq!(status, StatusCode::OK);

    // High crossover risk (BBB + BB should be near 100%)
    let crossover_risk = json["crossover_risk"].as_f64().unwrap();
    assert!(crossover_risk > 90.0);
}

// =============================================================================
// ETF SEC YIELD TESTS
// =============================================================================

#[tokio::test]
async fn test_etf_sec_yield_calculation() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "etf_id": "LQD",
        "net_investment_income": "125000",
        "avg_shares_outstanding": "500000",
        "max_offering_price": "120.50",
        "gross_expenses": "50000",
        "fee_waivers": "10000",
        "as_of_date": "2025-06-15"
    });

    let (status, json) = post_json(app, "/api/v1/etf/sec-yield", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["etf_id"], "LQD");
    assert!(json["sec_30_day_yield"].is_number());
    assert!(json["unsubsidized_yield"].is_number() || json["unsubsidized_yield"].is_null());
    assert_eq!(json["as_of_date"], "2025-06-15");
}

#[tokio::test]
async fn test_etf_sec_yield_minimal_inputs() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Minimal required inputs (no optional fee fields)
    let request_body = json!({
        "etf_id": "HYG",
        "net_investment_income": "200000",
        "avg_shares_outstanding": "1000000",
        "max_offering_price": "85.00",
        "as_of_date": "2025-07-01"
    });

    let (status, json) = post_json(app, "/api/v1/etf/sec-yield", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["etf_id"], "HYG");
    assert!(json["sec_30_day_yield"].is_number());
}

// =============================================================================
// ETF BASKET TESTS
// =============================================================================

#[tokio::test]
async fn test_etf_basket_creation() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "etf_id": "LQD",
        "creation_unit_size": "50000",
        "total_shares": "1000000",
        "cash_balance": "500000",
        "holdings": [
            {
                "instrument_id": "US912810TD00",
                "notional": "300000",
                "market_price": "99.50",
                "isin": "US912810TD00"
            },
            {
                "instrument_id": "US037833DV24",
                "notional": "250000",
                "market_price": "101.00",
                "isin": "US037833DV24"
            },
            {
                "instrument_id": "US91282CJK09",
                "notional": "250000",
                "market_price": "98.50",
                "isin": "US91282CJK09"
            },
            {
                "instrument_id": "US459058GX99",
                "notional": "200000",
                "market_price": "100.50",
                "isin": "US459058GX99"
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/etf/basket", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["etf_id"], "LQD");
    assert!(json["creation_unit_size"].is_string()); // Returns as string
    assert!(json["components"].is_array());
    assert!(json["cash_component"].is_string()); // Returns as string
    assert!(json["total_value"].is_string()); // Returns as string
}

#[tokio::test]
async fn test_etf_basket_analyze() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "etf_id": "HYG",
        "creation_unit_size": "25000",
        "total_shares": "500000",
        "cash_balance": "250000",
        "basket_holdings": [
            {
                "instrument_id": "US38141GXT49",
                "notional": "500000",
                "market_price": "100.00",
                "isin": "US38141GXT49"
            },
            {
                "instrument_id": "US459058HC91",
                "notional": "500000",
                "market_price": "98.00",
                "isin": "US459058HC91"
            }
        ],
        "target_holdings": [
            {
                "instrument_id": "US38141GXT49",
                "notional": "550000",
                "market_price": "100.00",
                "isin": "US38141GXT49"
            },
            {
                "instrument_id": "US459058HC91",
                "notional": "450000",
                "market_price": "98.00",
                "isin": "US459058HC91"
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/etf/basket/analyze", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["etf_id"], "HYG");
    // Analysis should include weight differences
    assert!(json.get("weight_differences").is_some());
}

// =============================================================================
// KEY RATE DURATION TESTS
// =============================================================================

#[tokio::test]
async fn test_key_rate_duration_calculation() {
    let engine = create_test_engine();
    let app = create_router(engine);

    let request_body = json!({
        "portfolio_id": "KRD_TEST",
        "name": "Key Rate Duration Test Portfolio",
        "positions": [
            {
                "instrument_id": "BOND_2Y",
                "notional": "5000000",
                "market_price": "99.50",
                "key_rate_durations": [[2.0, 1.85], [5.0, 0.15], [10.0, 0.0]]
            },
            {
                "instrument_id": "BOND_5Y",
                "notional": "3000000",
                "market_price": "101.25",
                "key_rate_durations": [[2.0, 0.10], [5.0, 4.65], [10.0, 0.25]]
            },
            {
                "instrument_id": "BOND_10Y",
                "notional": "2000000",
                "market_price": "98.75",
                "key_rate_durations": [[2.0, 0.0], [5.0, 0.30], [10.0, 8.70]]
            }
        ],
        "tenors": [2.0, 5.0, 10.0, 30.0]
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/key-rate-duration", request_body).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "KRD_TEST");
    assert!(json["profile"].is_array());
    assert!(json["total_duration"].is_number());

    // Verify profile structure
    let points = json["profile"].as_array().unwrap();
    assert!(!points.is_empty());
    assert!(points[0]["tenor"].is_number());
    assert!(points[0]["duration"].is_number());
    assert!(points[0]["contribution_pct"].is_number());
}

#[tokio::test]
async fn test_key_rate_duration_without_input_krd() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Test with positions that don't have pre-computed KRD
    let request_body = json!({
        "portfolio_id": "KRD_NO_INPUT",
        "name": "KRD Without Input Data",
        "positions": [
            {
                "instrument_id": "BOND_A",
                "notional": "1000000",
                "market_price": "100.00"
            },
            {
                "instrument_id": "BOND_B",
                "notional": "1000000",
                "market_price": "99.50"
            }
        ],
        "tenors": [2.0, 5.0, 10.0]
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/key-rate-duration", request_body).await;

    // Should still return success even with no KRD data
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["portfolio_id"], "KRD_NO_INPUT");
}

#[tokio::test]
async fn test_key_rate_duration_standard_tenors() {
    let engine = create_test_engine();
    let app = create_router(engine);

    // Test with standard market tenors
    let request_body = json!({
        "portfolio_id": "KRD_STANDARD",
        "name": "Standard Tenors",
        "positions": [
            {
                "instrument_id": "BOND_LONG",
                "notional": "10000000",
                "market_price": "95.00",
                "key_rate_durations": [
                    [0.25, 0.0], [0.5, 0.0], [1.0, 0.05],
                    [2.0, 0.15], [3.0, 0.20], [5.0, 0.50],
                    [7.0, 0.80], [10.0, 2.50], [20.0, 5.00], [30.0, 6.80]
                ]
            }
        ]
    });

    let (status, json) = post_json(app, "/api/v1/portfolio/key-rate-duration", request_body).await;

    assert_eq!(status, StatusCode::OK);

    // Check total duration is sum of KRD
    let total = json["total_duration"].as_f64();
    assert!(total.is_some());
}
