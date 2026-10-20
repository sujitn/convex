//! Actual HTTP router controls for supplied key-rate coverage and unavailable metrics.
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
use serde_json::{json, Value};
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
async fn call(input: Value) -> (StatusCode, Value) {
    call_raw(serde_json::to_vec(&input).unwrap()).await
}
async fn call_raw(input: Vec<u8>) -> (StatusCode, Value) {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/portfolio/key-rate-duration")
                .header("content-type", "application/json")
                .body(Body::from(input))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&bytes)}));
    (status, value)
}
fn request(positions: Value, tenors: Value) -> Value {
    json!({"portfolio_id":"SUPPLIED-RISK","name":"Supplied risk test","positions":positions,"tenors":tenors})
}
fn row(id: &str, price: &str, risk: Value) -> Value {
    let mut result = json!({"instrument_id":id,"notional":"100","market_price":price});
    if !risk.is_null() {
        result["key_rate_durations"] = risk;
    }
    result
}
fn close(actual: &Value, expected: f64) {
    assert!((actual.as_f64().unwrap() - expected).abs() <= 1e-12);
}

#[tokio::test]
async fn exact_factors_and_missing_coverage_preserve_actual_known_currency_risk() {
    let (status, result) = call(request(
        json!([row("A", "100", json!([[2.0, 1.0], [2.0005, 7.0]]))]),
        json!([2.0005, 2.0]),
    ))
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["name"], "Supplied risk test");
    assert_eq!(result["coverage_complete"], true);
    close(&result["profile"][0]["duration"], 7.0);
    close(&result["profile"][1]["duration"], 1.0);
    close(&result["profile"][0]["partial_dv01"], 0.07);
    let (status, result) = call(request(
        json!([
            row("A", "100", json!([[5.0, 5.0]])),
            row("B", "100", Value::Null)
        ]),
        json!([5.0, 30.0]),
    ))
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["coverage_complete"], false);
    close(&result["profile"][0]["known_partial_dv01"], 0.05);
    assert!(result["profile"][0]["partial_dv01"].is_null());
    assert_eq!(result["profile"][0]["covered_holdings"], 1);
    assert!(result["profile"][1]["duration"].is_null());
    assert!(result["profile"][1]["known_partial_dv01"].is_null());
    assert!(result["long_duration"].is_null());
    for weighting in ["MarketValue", "ParValue", "EqualWeight"] {
        let mut input = request(
            json!([
                row("A", "100", json!([[5.0, 1.0]])),
                row("B", "200", json!([[5.0, 9.0]]))
            ]),
            json!([5.0]),
        );
        input["weighting"] = json!(weighting);
        let (status, result) = call(input).await;
        assert_eq!(status, StatusCode::OK);
        close(&result["profile"][0]["partial_dv01"], 0.19);
    }
}

#[tokio::test]
async fn unavailable_weighting_and_zero_sum_never_become_successful_zero_ratios() {
    for positions in [json!([]), json!([row("A", "100", Value::Null)])] {
        let (status, result) = call(request(positions, json!([5.0]))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["duration_available"], false);
        for field in [
            "total_duration",
            "short_duration",
            "intermediate_duration",
            "long_duration",
        ] {
            assert!(result[field].is_null(), "{field}: {result}");
        }
        assert!(result["profile"][0]["duration"].is_null());
        assert!(result["profile"][0]["contribution_pct"].is_null());
    }
    let (status, result) = call(request(
        json!([row("A", "100", json!([[2.0, 3.0], [5.0, -3.0]]))]),
        json!([2.0, 5.0]),
    ))
    .await;
    assert_eq!(status, StatusCode::OK);
    close(&result["total_duration"], 0.0);
    assert!(result["profile"][0]["contribution_pct"].is_null());
    assert!(result["profile"][1]["contribution_pct"].is_null());
    let (status, result) = call(request(
        json!([row("A", "100", json!([[5.0, 0.0]]))]),
        json!([5.0]),
    ))
    .await;
    assert_eq!(status, StatusCode::OK);
    close(&result["profile"][0]["duration"], 0.0);
    close(&result["profile"][0]["partial_dv01"], 0.0);
}

#[tokio::test]
async fn invalid_sets_and_ignored_extra_input_are_rejected_as_whole_requests() {
    let valid = row("A", "100", json!([[5.0, 1.0]]));
    for input in [
        request(json!([valid.clone(), valid.clone()]), json!([5.0])),
        request(json!([row("A", "0", json!([[5.0, 5.0]]))]), json!([5.0])),
        request(json!([valid.clone()]), json!([5.0, 5.0])),
        request(json!([valid.clone()]), json!([])),
        request(
            json!([row("A", "100", json!([[5.0, 1.0], [7.0, 2.0], [7.0, 3.0]]))]),
            json!([5.0]),
        ),
    ] {
        let (status, _) = call(input).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let mut tiny = request(
        json!([row("A", "100", json!([[5.0, "ERASED_NUMBER"]]))]),
        json!([5.0]),
    );
    for number in ["1e-400", "-1e-400"] {
        let raw = serde_json::to_string(&tiny)
            .unwrap()
            .replace("\"ERASED_NUMBER\"", number);
        let (status, _) = call_raw(raw.into_bytes()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    tiny["positions"][0]["key_rate_durations"] = json!([[5.0, 1.0]]);
    tiny["positions"][0]["market_price"] = json!("0.00000000000000000000000000001");
    let (status, _) = call(tiny).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let mut input = request(json!([valid]), json!([5.0]));
    input["ignored_factor"] = json!(12);
    let (status, _) = call(input).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
