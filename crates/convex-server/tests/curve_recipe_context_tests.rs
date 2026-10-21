#[path = "../../convex-engine/tests/support/curve_replay.rs"]
mod recipes;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use convex_core::contracts::*;
use convex_engine::{snapshot::SnapshotValuationResult, RuntimeInput};
use convex_ports::snapshot::{MarketSnapshot, SnapshotRepository};
use http_body_util::BodyExt;
use support::*;
use tower::ServiceExt;

#[tokio::test]
async fn curve_creation_rejects_an_invalid_recipe_without_storing_it() {
    let setup = setup().await;
    let engine = make_engine(
        &setup,
        archive(&setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    let curve_id = convex_core::ids::CurveId::new("REJECT-MISMATCHED-KEY");
    let router = convex_server::routes::create_router_with_access(
        engine.clone(),
        true,
        convex_server::access::AccessControl::local_development(),
    );
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/curves")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "schema_version": 1,
                        "curve_id": curve_id.as_str(),
                        "version": 1,
                        "source_time": "2026-10-02T12:00:00Z",
                        "recorded_at": "2026-10-02T12:00:00Z",
                        "source": "fixture-source",
                        "model_id": "fixture-model",
                        "input_versions": [],
                        "quality": "Valid",
                        "max_age_ms": 86_400_000,
                        "recipe": {"key": {"id": "SOME-OTHER-CURVE", "version": "1"}}
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["code"], "INVALID_INPUT", "{body}");
    let later = UtcTimestamp::parse("2027-01-01T00:00:00Z").unwrap();
    assert!(engine
        .curve_store()
        .recipe_at(&curve_id, later)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn existing_runtime_endpoint_replays_complete_recipes_and_rejects_wrong_roles() {
    let mut setup = setup().await;
    setup.plan.assembly.curves.clear();
    setup.plan.assembly.curve_recipes = vec![recipes::record(recipes::recipe())];
    let engine = make_engine(
        &setup,
        archive(&setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let router = convex_server::routes::create_router_with_access(
        engine.clone(),
        true,
        convex_server::access::AccessControl::local_development(),
    );
    let retained = engine.runtime_results()[0].clone();
    let original = setup
        .snapshots
        .get(&retained.result.provenance().snapshot_id)
        .await
        .unwrap()
        .unwrap();
    for control in 0..3 {
        let mut content = original.content().clone();
        if control == 1 {
            content.curve_recipes[0].recipe["role"] =
                serde_json::json!({"kind":"Projection","index_id":"explicit-index"});
        } else if control == 2 {
            content.curve_recipes[0].recipe["ignored_economics"] = true.into();
        }
        let snapshot = MarketSnapshot::new(content).unwrap();
        let context = setup
            .plan
            .context(&snapshot, &InstrumentId::new("BOND-001").unwrap())
            .unwrap();
        let body = serde_json::to_vec(&serde_json::json!({"snapshot":snapshot,"context":context,
            "request_id":retained.result.request_id(),"calculated_at":retained.result.provenance().calculated_at})).unwrap();
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runtime/valuation")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        if control == 0 {
            assert_eq!(response.status(), StatusCode::OK);
            let result: SnapshotValuationResult =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert_eq!(result.envelope(), &retained.result);
        } else {
            // Runtime transport acknowledges the complete failed-measure envelope.
            assert_eq!(response.status(), StatusCode::OK);
            let result: SnapshotValuationResult =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert_eq!(result.envelope().status(), ResultStatus::Failed);
            assert!(result.envelope().results().is_empty());
            assert_eq!(result.envelope().errors().len(), 4);
            assert!(result
                .envelope()
                .errors()
                .iter()
                .all(|e| e.code() == ErrorCode::InvalidInput));
        }
    }
    engine.shutdown().await.unwrap();
}
