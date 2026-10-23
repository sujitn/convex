//! Actual core HTTP access, durable acknowledgement and cancellation boundaries.
#![cfg(unix)]
#[path = "../../convex-engine/tests/support/core_fixture.rs"]
mod fixture;
#[path = "support/access_fixture.rs"]
pub mod identity;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod runtime_setup;
use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use convex_core::contracts::*;
use convex_engine::{
    core_workflow::CoreWorkflow,
    ports::{core_workflow::*, runtime::*, TraitError},
    PricingEngineBuilder,
};
use convex_server::{access::*, Server, ServerConfig, ServerProfile};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

fn router(f: &fixture::Fixture, access: Arc<AccessControl>, core: Arc<CoreWorkflow>) -> Router {
    let engine = PricingEngineBuilder::new()
        .with_config(convex_engine::ports::config::EngineConfig {
            max_concurrent_calcs: 1,
            ..Default::default()
        })
        .with_storage(f.storage.clone())
        .build()
        .unwrap();
    let config = ServerConfig {
        profile: ServerProfile::Demo,
        host: "127.0.0.1".into(),
        port: 0,
        ..Default::default()
    };
    Server::with_access_control(config, Arc::new(engine), access)
        .with_core_workflow(core)
        .router()
}
fn request(method: &str, path: &str, body: Option<Value>, token: Option<&str>) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if let Some(key) = body
        .as_ref()
        .and_then(|v| v.get("idempotency_key"))
        .and_then(|v| v.as_str())
    {
        request = request.header("idempotency-key", key);
    }
    request
        .body(
            body.map(|v| Body::from(v.to_string()))
                .unwrap_or_else(Body::empty),
        )
        .unwrap()
}
async fn call(
    router: Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let response = router
        .oneshot(request(method, path, body, token))
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    )
}
fn broad_policy() -> PolicyDocument {
    let mut policy = identity::policy();
    for grant in &mut policy.grants {
        for kind in [
            ResourceKind::Instrument,
            ResourceKind::Curve,
            ResourceKind::Source,
            ResourceKind::Book,
        ] {
            grant.resources.insert(kind, vec!["*".into()]);
        }
    }
    policy
}

#[tokio::test]
async fn authenticated_core_reads_writes_and_reference_definitions_use_current_explicit_grants() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture::Fixture::new(dir.path()).await;
    let denied = router(&f, AccessControl::deny_all(), f.workflow());
    for (method, path) in [
        ("POST", "/api/v1/analytics/snapshots"),
        ("GET", "/api/v1/analytics/snapshots/x"),
        ("POST", "/api/v1/analytics/snapshots/x/run/risk"),
        ("GET", "/api/v1/analytics/results/x"),
        ("POST", "/api/v1/analytics/curves"),
        ("POST", "/api/v1/analytics/fixings"),
    ] {
        assert_eq!(
            call(denied.clone(), method, path, Some(json!({})), None)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let issuer = identity::IssuerFixture::start().await;
    let access = AccessControl::new(issuer.verifier.clone(), broad_policy()).unwrap();
    let app = router(&f, access.clone(), f.workflow());
    let human = Some(identity::HUMAN_TOKEN);
    let reader = Some(identity::SERVICE_TOKEN);
    let (status, snapshot) = call(
        app.clone(),
        "POST",
        "/api/v1/analytics/snapshots",
        Some(serde_json::to_value(&f.plan).unwrap()),
        human,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{snapshot}");
    let id = snapshot["snapshot_id"].as_str().unwrap();
    let run_path = format!("/api/v1/analytics/snapshots/{id}/run/risk");
    assert_eq!(
        call(app.clone(), "POST", &run_path, Some(json!({})), reader)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, receipt) = call(app.clone(), "POST", &run_path, Some(json!({})), human).await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    let result_path = format!(
        "/api/v1/analytics/results/{}",
        receipt["result_id"].as_str().unwrap()
    );
    let (status, result) = call(app.clone(), "GET", &result_path, None, reader).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["ok"], "true");
    assert_eq!(
        call(
            app.clone(),
            "GET",
            &format!("/api/v1/analytics/snapshots/{id}"),
            None,
            reader
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/v1/analytics/curves",
            Some(serde_json::to_value(&f.recipe).unwrap()),
            reader
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/v1/analytics/fixings",
            Some(serde_json::to_value(&f.published).unwrap()),
            reader
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    let mut policy = broad_policy();
    policy.version = 2;
    policy.grants[0].data.retain(|s| *s != DataScope::Books);
    access.replace_policy(1, policy.clone()).unwrap();
    assert_eq!(
        call(app.clone(), "GET", &result_path, None, human).await.0,
        StatusCode::FORBIDDEN
    );
    // A calculation definition can contain complete private book inputs. A
    // reference-writer role alone must not bypass the independent book grant.
    let record = f
        .storage
        .bonds
        .get_master(
            &convex_engine::ports::reference_master::MasterKey::Calculation(
                f.plan.definition.id.clone(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
    let mut definition = record.definition;
    if let convex_engine::ports::reference_master::MasterDefinition::Calculation(d) =
        &mut definition
    {
        d.id = ReferenceDataId::new("core.http.private.book").unwrap();
    }
    let command = json!({"idempotency_key":"core.http.private.seed","changes":[],"master_changes":[{"expected_version":null,"definition":definition}]});
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/v1/reference/transactions",
            Some(command.clone()),
            human
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    policy.version = 3;
    policy.grants[0].data.push(DataScope::Books);
    policy.grants[0].roles = vec![Role::ReferenceWriter];
    access.replace_policy(2, policy).unwrap();
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/v1/reference/transactions",
            Some(command),
            human
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/v1/analytics/curves",
            Some(serde_json::to_value(&f.recipe).unwrap()),
            human
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/v1/analytics/fixings",
            Some(serde_json::to_value(&f.published).unwrap()),
            human
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(app.clone(), "POST", &run_path, Some(json!({})), human)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let mut reference_only = broad_policy();
    reference_only.version = 4;
    reference_only.grants[0].roles = vec![Role::Reader];
    reference_only.grants[0].data = vec![DataScope::Reference];
    access.replace_policy(3, reference_only).unwrap();
    assert_eq!(
        call(
            app,
            "GET",
            "/api/v1/reference/commits/core.http.private.seed",
            None,
            human
        )
        .await
        .0,
        StatusCode::FORBIDDEN,
        "reference-receipt reads must not bypass core book/data grants"
    );
    issuer.finish().await;
}

struct DelayedCommit {
    inner: Arc<dyn RuntimeStore>,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    done: tokio::sync::Notify,
}
#[async_trait]
impl RuntimeStore for DelayedCommit {
    async fn load(&self, id: &SourceId) -> Result<Option<RuntimeCheckpoint>, TraitError> {
        self.inner.load(id).await
    }
    async fn checkpoint(&self, expected: u64, image: &RuntimeCheckpoint) -> Result<(), TraitError> {
        self.inner.checkpoint(expected, image).await
    }
    async fn get_core_result(&self, id: &str) -> Result<Option<CoreResult>, TraitError> {
        self.inner.get_core_result(id).await
    }
    async fn save_core_result(&self, result: &CoreResult) -> Result<(), TraitError> {
        self.entered.notify_one();
        self.release.notified().await;
        self.inner.save_core_result(result).await?;
        self.done.notify_one();
        Ok(())
    }
}
fn delayed(f: &fixture::Fixture) -> (Arc<DelayedCommit>, Arc<CoreWorkflow>) {
    let store = Arc::new(DelayedCommit {
        inner: f.results.clone(),
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        done: tokio::sync::Notify::new(),
    });
    let core = Arc::new(CoreWorkflow::new(
        f.storage.bonds.clone(),
        f.storage.curves.clone(),
        f.fixings.clone(),
        f.snapshots.clone(),
        store.clone(),
        ModelId::new("core.test.build").unwrap(),
    ));
    (store, core)
}
async fn notified(notify: &tokio::sync::Notify) {
    tokio::time::timeout(std::time::Duration::from_secs(10), notify.notified())
        .await
        .unwrap();
}

#[tokio::test]
async fn cancelled_http_waiter_keeps_capacity_until_its_durable_result_commit_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture::Fixture::new(dir.path()).await;
    let (store, core) = delayed(&f);
    let snapshot = core.assemble(f.plan.clone()).await.unwrap();
    let app = router(&f, AccessControl::local_development(), core);
    let path = format!("/api/v1/analytics/snapshots/{}/run/risk", snapshot.id);
    let waiter = tokio::spawn(
        app.clone()
            .oneshot(request("POST", &path, Some(json!({})), None)),
    );
    notified(&store.entered).await;
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    assert_eq!(
        call(app.clone(), "POST", &path, Some(json!({})), None)
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    store.release.notify_one();
    notified(&store.done).await;
    let id = CoreResult::key(
        &snapshot.id,
        "risk",
        &ModelId::new("core.test.build").unwrap(),
    );
    let (status, result) = call(
        app.clone(),
        "GET",
        &format!("/api/v1/analytics/results/{id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["ok"], "true");
    let (_, receipt) = call(app, "POST", &path, Some(json!({})), None).await;
    assert_eq!(receipt["reused"], true);
}

#[tokio::test]
async fn role_revocation_is_rechecked_after_calculation_and_durable_commit() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture::Fixture::new(dir.path()).await;
    let (store, core) = delayed(&f);
    let snapshot = core.assemble(f.plan.clone()).await.unwrap();
    let issuer = identity::IssuerFixture::start().await;
    let access = AccessControl::new(issuer.verifier.clone(), broad_policy()).unwrap();
    let app = router(&f, access.clone(), core);
    let path = format!("/api/v1/analytics/snapshots/{}/run/risk", snapshot.id);
    let pending = tokio::spawn(app.clone().oneshot(request(
        "POST",
        &path,
        Some(json!({})),
        Some(identity::HUMAN_TOKEN),
    )));
    notified(&store.entered).await;
    let mut policy = broad_policy();
    policy.version = 2;
    policy.grants[0].roles = vec![Role::Reader];
    access.replace_policy(1, policy).unwrap();
    store.release.notify_one();
    let response = pending.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let id = CoreResult::key(
        &snapshot.id,
        "risk",
        &ModelId::new("core.test.build").unwrap(),
    );
    assert_eq!(
        call(
            app,
            "GET",
            &format!("/api/v1/analytics/results/{id}"),
            None,
            Some(identity::HUMAN_TOKEN)
        )
        .await
        .0,
        StatusCode::OK
    );
    issuer.finish().await;
}
