//! Complete supplied quote books preserve exact amounts and checked admission.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use convex_engine::{
    ports::{
        config::EngineConfig, market_data::MarketDataProvider,
        reference_data::ReferenceDataProvider,
    },
    PricingEngineBuilder,
};
use convex_ext_file::{
    EmptyCurveInputSource, EmptyEtfHoldingsSource, EmptyEtfQuoteSource, EmptyFxRateSource,
    EmptyIndexFixingSource, EmptyInflationFixingSource, EmptyIssuerReferenceSource,
    EmptyQuoteSource, EmptyRatingSource, EmptyVolatilitySource,
};
use http_body_util::BodyExt;
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

fn app() -> Router {
    let market = MarketDataProvider {
        quotes: Arc::new(EmptyQuoteSource),
        curve_inputs: Arc::new(EmptyCurveInputSource),
        index_fixings: Arc::new(EmptyIndexFixingSource),
        volatility: Arc::new(EmptyVolatilitySource),
        fx_rates: Arc::new(EmptyFxRateSource),
        inflation_fixings: Arc::new(EmptyInflationFixingSource),
        etf_quotes: Arc::new(EmptyEtfQuoteSource),
    };
    let reference = ReferenceDataProvider {
        issuers: Arc::new(EmptyIssuerReferenceSource),
        ratings: Arc::new(EmptyRatingSource),
        etf_holdings: Arc::new(EmptyEtfHoldingsSource),
    };
    let engine = PricingEngineBuilder::new()
        .with_config(EngineConfig::default())
        .with_market_data(Arc::new(market))
        .with_reference_data(Arc::new(reference))
        .with_storage(Arc::new(convex_ext_redb::create_test_storage().unwrap()))
        .build()
        .unwrap();
    convex_server::routes::create_router_with_access(
        Arc::new(engine),
        true,
        convex_server::access::AccessControl::local_development(),
    )
}

fn quote(id: &str) -> Value {
    serde_json::json!({"instrument_id":id,"currency":"USD","settlement_date":"2025-01-15","clean_price_mid":"99","accrued_interest":"1",
        "modified_duration":"5","convexity":"30","ytm_mid":"0.05","z_spread_mid":"100","dv01":"0.05","timestamp":1736899200000i64,
        "pricing_spec":"supplied-test","source":"independent-test","is_stale":false,"quality":100})
}
fn portfolio(id: &str) -> Value {
    serde_json::json!({"portfolio_id":id,"name":"supplied quote cohort","currency":"USD","positions":[{"instrument_id":"a","notional":"1000000","sector":"Government","rating":"AAA"}]})
}
async fn post(router: Router, path: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}
#[tokio::test]
async fn actual_quotes_keep_exact_currency_and_clean_accrued_basis() {
    let router = app();
    let body = serde_json::json!({"portfolio":portfolio("one"),"bond_prices":[quote("a")]});
    let (status, value) = post(router.clone(), "/api/v1/portfolio/analytics", body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        value["market_value"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        rust_decimal::Decimal::from(1000000)
    );
    assert_eq!(
        value["dv01"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        rust_decimal::Decimal::from(500)
    );
    assert_eq!(value["settlement_date"], "2025-01-15");
    assert_eq!(value["timestamp"], 1736899200000i64);
    let (status, value) = post(router, "/api/v1/portfolio/buckets/sector", body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["num_holdings"], 1);
    assert_eq!(
        value["total_market_value"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        rust_decimal::Decimal::from(990000)
    );
}
#[tokio::test]
async fn missing_duplicate_or_incomplete_quotes_never_return_a_partial_success() {
    let router = app();
    let mut no_clean = quote("a");
    no_clean["clean_price_mid"] = Value::Null;
    let mut no_accrued = quote("a");
    no_accrued["accrued_interest"] = Value::Null;
    let mut foreign = quote("a");
    foreign["currency"] = "EUR".into();
    for prices in [
        serde_json::json!([]),
        serde_json::json!([quote("a"), quote("a")]),
        serde_json::json!([no_clean]),
        serde_json::json!([no_accrued]),
        serde_json::json!([foreign]),
    ] {
        for path in [
            "/api/v1/portfolio/analytics",
            "/api/v1/portfolio/buckets/sector",
        ] {
            let (status, value) = post(
                router.clone(),
                path,
                serde_json::json!({"portfolio":portfolio("bad"),"bond_prices":prices.clone()}),
            )
            .await;
            assert!(status.is_client_error(), "{path}: {status} {value}");
            assert!(value.get("market_value").is_none());
            assert!(value.get("total_market_value").is_none());
        }
    }
}
#[tokio::test]
async fn batch_retains_each_original_portfolio_success_or_error() {
    let router = app();
    let mut bad = portfolio("missing");
    bad["positions"][0]["instrument_id"] = "absent".into();
    let (status, value) = post(
        router.clone(),
        "/api/v1/portfolio/analytics/batch",
        serde_json::json!({"portfolios":[portfolio("good"),bad],"bond_prices":[quote("a")]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["outcomes"].as_array().unwrap().len(), 2);
    assert_eq!(value["outcomes"][0]["portfolio_id"], "good");
    assert_eq!(value["outcomes"][0]["status"], "success");
    assert_eq!(value["outcomes"][1]["portfolio_id"], "missing");
    assert_eq!(value["outcomes"][1]["status"], "error");
    let mut same_bad = portfolio("same");
    same_bad["positions"][0]["instrument_id"] = "absent".into();
    let (status, value) = post(router, "/api/v1/portfolio/analytics/batch",
        serde_json::json!({"portfolios":[portfolio("same"),same_bad,portfolio("same")],"bond_prices":[quote("a")]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["outcomes"].as_array().unwrap().len(), 3);
    for (index, expected) in ["success", "error", "success"].into_iter().enumerate() {
        assert_eq!(value["outcomes"][index]["input_index"], index);
        assert_eq!(value["outcomes"][index]["portfolio_id"], "same");
        assert_eq!(value["outcomes"][index]["status"], expected);
    }
}
#[tokio::test]
async fn invalid_currency_or_reference_never_becomes_usd_or_a_fabricated_identifier() {
    let router = app();
    let mut invalid = portfolio("bad-currency");
    invalid["currency"] = "INVALID".into();
    let (status, _) = post(
        router.clone(),
        "/api/v1/portfolio/analytics",
        serde_json::json!({"portfolio":invalid,"bond_prices":[quote("a")]}),
    )
    .await;
    assert!(status.is_client_error());
    let mut quote = quote("a");
    quote["isin"] = "invalid".into();
    let (status, value) = post(
        router,
        "/api/v1/portfolio/buckets/sector",
        serde_json::json!({"portfolio":portfolio("bad-reference"),"bond_prices":[quote]}),
    )
    .await;
    assert!(status.is_client_error());
    assert!(value.get("total_market_value").is_none());
}

#[tokio::test]
async fn contribution_selection_is_explicit_and_never_silently_ignored() {
    let router = app();
    let body = serde_json::json!({"portfolio":portfolio("selected"),"bond_prices":[quote("a")],
        "contribution_types":["duration"],"duration_measure":"modified"});
    let (status, value) = post(
        router.clone(),
        "/api/v1/portfolio/risk-contributions",
        body.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["duration"]["portfolio_duration"], 5.0);
    assert_eq!(value["duration"]["by_holding"].as_array().unwrap().len(), 1);
    assert!(value["dv01"].is_null());
    for types in [
        serde_json::json!(["unknown"]),
        serde_json::json!(["duration", "DURATION"]),
    ] {
        let mut invalid = body.clone();
        invalid["contribution_types"] = types;
        let (status, value) = post(
            router.clone(),
            "/api/v1/portfolio/risk-contributions",
            invalid,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(value["code"], "INVALID_PORTFOLIO_INPUT");
        assert!(value.get("duration").is_none());
    }
    let mut missing = body;
    missing.as_object_mut().unwrap().remove("duration_measure");
    let (status, value) = post(router, "/api/v1/portfolio/risk-contributions", missing).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(value.get("duration").is_none());
}
