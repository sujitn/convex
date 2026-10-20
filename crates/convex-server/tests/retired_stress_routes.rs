//! Retired incomplete stress endpoints must never execute a fallback calculation.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use axum::{extract::MatchedPath, middleware::Next, response::Response};
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

#[derive(Clone)]
struct ObservedRoute(Option<String>);
async fn observe_route(request: Request<Body>, next: Next) -> Response {
    let matched = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned());
    let mut response = next.run(request).await;
    response.extensions_mut().insert(ObservedRoute(matched));
    response
}
#[tokio::test]
async fn incomplete_stress_routes_are_absent() {
    let router = app().layer(axum::middleware::from_fn(observe_route));
    let healthy = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(healthy.status(), StatusCode::OK);
    assert_eq!(
        healthy
            .extensions()
            .get::<ObservedRoute>()
            .unwrap()
            .0
            .as_deref(),
        Some("/health")
    );
    for (method, path) in [
        ("POST", "/api/v1/stress/test"),
        ("POST", "/api/v1/stress/standard"),
        ("POST", "/api/v1/stress/single"),
        ("GET", "/api/v1/stress/scenarios"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
        assert!(
            response
                .extensions()
                .get::<ObservedRoute>()
                .unwrap()
                .0
                .is_none(),
            "retired route still matched: {path}"
        );
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["code"], "FORBIDDEN");
    }
}
