//! Large captured definitions retain complete authenticated scope checks.
#![cfg(unix)]
#[path = "../../convex-engine/tests/support/core_fixture.rs"]
mod fixture;
#[allow(dead_code)] // This integration suite uses only the identity/authorization subset.
#[path = "support/access_fixture.rs"]
mod identity;
#[path = "../../../tests/fixtures/packed.rs"]
mod packed;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod runtime_setup;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use convex_engine::PricingEngineBuilder;
use convex_server::{access::*, Server, ServerConfig, ServerProfile};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

fn router(f: &fixture::Fixture, access: Arc<AccessControl>) -> Router {
    let engine = PricingEngineBuilder::new()
        .with_storage(f.storage.clone())
        .build()
        .unwrap();
    Server::with_access_control(
        ServerConfig {
            profile: ServerProfile::Demo,
            host: "127.0.0.1".into(),
            port: 0,
            ..Default::default()
        },
        Arc::new(engine),
        access,
    )
    .with_core_workflow(f.workflow())
    .router()
}
async fn call(
    app: Router,
    method: &str,
    path: &str,
    value: Option<&Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if let Some(key) = value.and_then(|v| v["idempotency_key"].as_str()) {
        request = request.header("idempotency-key", key);
    }
    let response = app
        .oneshot(
            request
                .body(
                    value
                        .map(|v| Body::from(v.to_string()))
                        .unwrap_or_else(Body::empty),
                )
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn large_reference_roundtrip_scans_every_scope_and_rechecks_read_entitlements() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture::Fixture::new(dir.path()).await;
    let issuer = identity::IssuerFixture::start().await;
    let mut policy = identity::policy();
    for grant in &mut policy.grants {
        for kind in [
            ResourceKind::Instrument,
            ResourceKind::Book,
            ResourceKind::Curve,
            ResourceKind::Source,
        ] {
            grant.resources.insert(kind, vec!["*".into()]);
        }
    }
    let access = AccessControl::new(issuer.verifier.clone(), policy.clone()).unwrap();
    let app = router(&f, access.clone());
    let fixture: Value =
        serde_json::from_str(packed::text("tests/fixtures/library/date-rolls-core.json")).unwrap();
    let mut definition = fixture["definition"].clone();
    let templates: Vec<_> = definition["calculations"]
        .as_object()
        .unwrap()
        .values()
        .cloned()
        .collect();
    definition["id"] = json!("core.auth.large");
    definition["reference_inputs"]["core_capacity"] = json!("book_128");
    definition["calculations"] = Value::Object(
        (0..100)
            .map(|i| (format!("calc.{i}"), templates[i % templates.len()].clone()))
            .collect(),
    );
    let command = json!({"idempotency_key":"core.auth.large.commit","changes":[],"master_changes":[{"expected_version":null,"definition":{"kind":"calculation","definition":definition}}]});
    assert!(command.to_string().len() > 2 * 1024 * 1024);
    assert!(command.to_string().len() < 8 * 1024 * 1024);
    let (status, _) = call(
        app.clone(),
        "POST",
        "/api/v1/reference/transactions",
        Some(&command),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let human = Some(identity::HUMAN_TOKEN);
    let (status, receipt) = call(
        app.clone(),
        "POST",
        "/api/v1/reference/transactions",
        Some(&command),
        human,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    let path = "/api/v1/reference/commits/core.auth.large.commit";
    let (status, repeated) = call(app.clone(), "GET", path, None, human).await;
    assert_eq!(status, StatusCode::OK, "{repeated}");
    assert_eq!(receipt, repeated);
    let mut limited = policy.clone();
    limited.version += 1;
    limited.grants[0].data.retain(|v| *v != DataScope::Results);
    access.replace_policy(policy.version, limited).unwrap();
    assert_eq!(
        call(app.clone(), "GET", path, None, human).await.0,
        StatusCode::FORBIDDEN
    );
    policy.version += 2;
    access.replace_policy(policy.version - 1, policy).unwrap();
    assert_eq!(
        call(app.clone(), "GET", path, None, human).await.0,
        StatusCode::OK
    );
    let mut invalid = command.clone();
    invalid["idempotency_key"] = json!("core.auth.invalid.capacity");
    invalid["master_changes"][0]["expected_version"] = json!(1);
    invalid["master_changes"][0]["definition"]["definition"]["reference_inputs"]["core_capacity"] =
        json!("unbounded");
    assert!(call(
        app.clone(),
        "POST",
        "/api/v1/reference/transactions",
        Some(&invalid),
        human
    )
    .await
    .0
    .is_client_error());
    assert_eq!(
        call(
            app.clone(),
            "GET",
            "/api/v1/reference/commits/core.auth.invalid.capacity",
            None,
            human
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    // The unrelated raw analytics route retains its original body limit.
    assert!(
        call(app, "POST", "/api/v1/analytics", Some(&command), human)
            .await
            .0
            .is_client_error()
    );
    issuer.finish().await;
}
