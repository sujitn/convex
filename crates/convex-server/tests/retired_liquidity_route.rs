//! Unsupported liquidation estimates cannot invent capacity or bypass route authorization.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use axum::{extract::MatchedPath, middleware::Next, response::Response};
use convex_engine::{ports::config::EngineConfig, PricingEngineBuilder};
use http_body_util::BodyExt;
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

fn app() -> Router {
    let engine = PricingEngineBuilder::new()
        .with_config(EngineConfig::default())
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
async fn inferred_liquidation_route_is_absent_and_analysis_is_descriptive() {
    let router = app().layer(axum::middleware::from_fn(observe_route));
    for method in ["POST", "GET"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/api/v1/liquidity/days-to-liquidate")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(response
            .extensions()
            .get::<ObservedRoute>()
            .unwrap()
            .0
            .is_none());
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["code"], "FORBIDDEN");
        assert!(body.get("total_days").is_none());
    }
    for (request, expected) in [
        (
            serde_json::json!({"portfolio_id":"empty","name":"supplied","positions":[]}),
            StatusCode::OK,
        ),
        (
            serde_json::json!({"portfolio_id":"empty","name":"supplied","positions":[],"max_participation_rate":20}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/liquidity/analysis")
                    .header("content-type", "application/json")
                    .body(Body::from(request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert!(body.get("days_to_liquidate").is_none());
        if expected == StatusCode::OK {
            assert_eq!(body["metrics"]["total_holdings"], 0);
            assert!(body["metrics"]["avg_liquidity_score"].is_null());
            assert_eq!(body["distribution"], serde_json::json!([]));
        } else {
            assert_eq!(body["code"], "INVALID_PORTFOLIO_INPUT");
        }
    }
}
