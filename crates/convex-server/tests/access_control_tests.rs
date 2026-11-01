#[path = "support/access_fixture.rs"]
pub mod fixture;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use convex_server::access::*;
use fixture::*;
use serde_json::json;
use std::sync::{atomic::Ordering, Arc};
use tower::ServiceExt;

#[tokio::test]
async fn default_router_denies_anonymous_read_write_ws_and_job_access() {
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    let router = convex_server::routes::create_router(engine);
    for (method, path) in [
        ("GET", "/api/v1/bonds"),
        ("POST", "/api/v1/portfolios"),
        ("PUT", "/api/v1/books/BOOK-A"),
        ("GET", "/api/v1/books"),
        ("GET", "/ws"),
        ("GET", "/api/v1/ws"),
        ("POST", "/api/v1/jobs"),
        ("GET", "/api/v1/jobs/job-1"),
        ("DELETE", "/api/v1/jobs/job-1"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
    }
}

#[tokio::test]
async fn actual_introspection_roles_scopes_and_nested_batch_admission_are_fail_closed() {
    let issuer = IssuerFixture::start().await;
    let access = AccessControl::new(issuer.verifier.clone(), policy()).unwrap();
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    let router =
        convex_server::routes::create_router_with_access(engine.clone(), true, access.clone());
    let book = |id: &str| {
        convex_engine::ports::reference_master::MasterKey::Book(
            convex_core::contracts::ReferenceDataId::new(id).unwrap(),
        )
    };
    let (status, _, bond) = request(
        router.clone(),
        "GET",
        "/api/v1/bonds/BOND-001",
        Some(HUMAN_TOKEN),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{bond}");
    for path in [
        "/api/v1/bonds/BOND-002",
        "/api/v1/bonds",
        "/api/v1/bonds/isin/US0000000000",
        "/api/v1/runtime/results",
    ] {
        assert_eq!(
            request(router.clone(), "GET", path, Some(HUMAN_TOKEN), None)
                .await
                .0,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    for (method, path) in [
        ("POST", "/api/v1/portfolios"),
        ("PUT", "/api/v1/bonds/BOND-001"),
        ("POST", "/api/v1/runtime/valuation"),
        ("POST", "/api/v1/jobs"),
        ("GET", "/api/v1/jobs/job-1"),
        ("DELETE", "/api/v1/jobs/job-1"),
    ] {
        assert_eq!(
            request(
                router.clone(),
                method,
                path,
                Some(SERVICE_TOKEN),
                Some(json!({}))
            )
            .await
            .0,
            StatusCode::FORBIDDEN,
            "{method} {path}"
        );
    }
    let payload = json!({"portfolio_id":"BOOK-A","name":"fixture","currency":"USD","positions":[{"instrument_id":"BOND-002","quantity":"1"}]});
    assert_eq!(
        request(
            router.clone(),
            "POST",
            "/api/v1/portfolios",
            Some(HUMAN_TOKEN),
            Some(payload)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert!(engine
        .reference_repository()
        .get_master(&book("BOOK-A"))
        .await
        .unwrap()
        .is_none());
    let batch = json!({"portfolios":[{"portfolio_id":"BOOK-A","name":"allowed","positions":[]},{"portfolio_id":"BOOK-B","name":"forbidden","positions":[]}]});
    assert_eq!(
        request(
            router.clone(),
            "POST",
            "/api/v1/portfolios/batch",
            Some(HUMAN_TOKEN),
            Some(batch)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert!(engine
        .reference_repository()
        .get_master(&book("BOOK-A"))
        .await
        .unwrap()
        .is_none());
    let (status, _, _) = request(
        router.clone(),
        "POST",
        "/api/v1/portfolios",
        Some(HUMAN_TOKEN),
        Some(json!({"portfolio_id":"BOOK-A","name":"fixture","currency":"USD","positions":[]})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        request(
            router.clone(),
            "GET",
            "/api/v1/portfolios/BOOK-A",
            Some(HUMAN_TOKEN),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            router.clone(),
            "GET",
            "/api/v1/portfolios/BOOK-B",
            Some(HUMAN_TOKEN),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    // Plant an ungranted instrument directly in the book, bypassing HTTP entitlement.
    let planted = engine
        .reference_repository()
        .get_master(&book("BOOK-A"))
        .await
        .unwrap()
        .unwrap();
    let convex_engine::ports::reference_master::MasterDefinition::Book(mut definition) =
        planted.definition.clone()
    else {
        panic!("not a book")
    };
    definition
        .positions
        .push(convex_engine::ports::reference_master::BookPosition {
            instrument_id: convex_core::contracts::InstrumentId::new("BOND-002").unwrap(),
            quantity: convex_core::contracts::ExactDecimal::new(rust_decimal::Decimal::ONE),
            unit: convex_engine::ports::reference_master::QuantityUnit::Face,
            reference_version: None,
            source_identifier: None,
            sector: None,
            rating: None,
        });
    definition.effective.from = planted.recorded_at;
    let command = convex_engine::ports::reference_repository::ReferenceCommand {
        idempotency_key: convex_core::contracts::RequestId::new("access-plant-1").unwrap(),
        changes: Vec::new(),
        master_changes: vec![convex_engine::ports::reference_master::MasterChange {
            expected_version: Some(planted.version),
            definition: convex_engine::ports::reference_master::MasterDefinition::Book(definition),
        }],
    };
    engine
        .commit_reference(
            &command,
            convex_core::contracts::UtcTimestamp::from_datetime(chrono::Utc::now()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        request(
            router.clone(),
            "GET",
            "/api/v1/portfolios/BOOK-A",
            Some(HUMAN_TOKEN),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN,
        "stored response must not reveal an ungranted instrument"
    );
    let human = access
        .authenticate(Some(BearerToken::parse(HUMAN_TOKEN).unwrap()))
        .await
        .unwrap();
    let service = access
        .authenticate(Some(BearerToken::parse(SERVICE_TOKEN).unwrap()))
        .await
        .unwrap();
    let resource = Resource::new(ResourceKind::Instrument, "BOND-001");
    for action in [Action::JobSubmit, Action::JobRead, Action::JobCancel] {
        assert_eq!(
            access.authorize_job(&human, action, "BOOK-B", std::slice::from_ref(&resource)),
            Err(AccessError::Forbidden)
        );
        access
            .authorize_job(&human, action, "BOOK-A", std::slice::from_ref(&resource))
            .unwrap();
    }
    assert_eq!(
        access.authorize_job(&service, Action::JobSubmit, "BOOK-A", &[]),
        Err(AccessError::Forbidden)
    );
    assert_eq!(
        access.authorize_job(&service, Action::JobCancel, "BOOK-A", &[]),
        Err(AccessError::Forbidden)
    );
    // Client roles/identity headers and query-string tokens never authorize a request.
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/bonds/BOND-001?access_token=human-fixture-secret-marker")
                .header("x-forwarded-user", "alice")
                .header("x-role", "operator")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let duplicate = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/bonds/BOND-001")
                .header("authorization", format!("Bearer {HUMAN_TOKEN}"))
                .header("authorization", format!("Bearer {HUMAN_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::UNAUTHORIZED);
    let ambiguous = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/portfolios")
                .header("authorization", format!("Bearer {HUMAN_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"portfolio_id":"BOOK-A","portfolio_id":"BOOK-B","name":"ambiguous"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ambiguous.status(), StatusCode::BAD_REQUEST);
    assert_eq!(issuer.state.bad_authorization.load(Ordering::SeqCst), 0);
    assert!(issuer.state.calls.load(Ordering::SeqCst) > 10);
    issuer.finish().await;
}

#[tokio::test]
async fn dependency_failure_preserves_liveness_and_strategy_never_becomes_ready() {
    let issuer = IssuerFixture::start().await;
    let document = policy();
    let access = AccessControl::new(issuer.verifier.clone(), document.clone()).unwrap();
    let mut s = support::setup().await;
    // Curve expiry fails only a context that reads the curve (D58), so the plan asks for
    // a spread as well.
    s.plan
        .measures
        .push(convex_core::contracts::MeasureId::new("z_spread_mid").unwrap());
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    let router =
        convex_server::routes::create_router_with_access(engine.clone(), true, access.clone());
    assert_eq!(
        request(router.clone(), "GET", "/live", None, None).await.0,
        StatusCode::OK
    );
    let (status, _, view) = request(router.clone(), "GET", "/ready", None, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(view["dependencies"][0]["state"], "missing");
    engine.start().await.unwrap();
    assert_eq!(
        request(router.clone(), "GET", "/ready", None, None).await.0,
        StatusCode::OK
    );
    let (status, _, strategy) = request(router.clone(), "GET", "/strategy-ready", None, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(strategy["ready"], false);
    assert_eq!(strategy["analytics_ready"], true);
    assert_eq!(strategy["live_inputs_ready"], false);
    // An available issuer cannot make an expired policy ready.
    let mut expired_policy = document.clone();
    expired_policy.valid_until = now() - 1;
    expired_policy.grants.clear();
    let expired = AccessControl::new(issuer.verifier.clone(), expired_policy).unwrap();
    let expired_router =
        convex_server::routes::create_router_with_access(engine.clone(), true, expired);
    let (status, _, view) = request(expired_router.clone(), "GET", "/ready", None, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(view["dependencies"][3]["state"], "unavailable");
    assert_eq!(view["dependencies"][4]["state"], "ready");
    assert_eq!(
        request(expired_router, "GET", "/live", None, None).await.0,
        StatusCode::OK
    );
    access.mark_policy_unavailable();
    assert_eq!(
        request(router.clone(), "GET", "/ready", None, None).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        request(router.clone(), "GET", "/live", None, None).await.0,
        StatusCode::OK
    );
    access.replace_policy(1, document).unwrap();
    issuer.state.unavailable.store(true, Ordering::SeqCst);
    let (_, _, view) = request(router.clone(), "GET", "/ready", None, None).await;
    assert_eq!(view["ready"], false);
    assert_eq!(view["dependencies"][4]["state"], "unavailable");
    assert_eq!(
        request(router.clone(), "GET", "/live", None, None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            router.clone(),
            "GET",
            "/api/v1/bonds/BOND-001",
            Some(HUMAN_TOKEN),
            None
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    issuer.state.unavailable.store(false, Ordering::SeqCst);
    let mut updates = engine.subscribe_results();
    engine
        .submit_update(
            support::event(2, 10_001, "98.25"),
            support::time(10_001),
            support::time(10_001),
        )
        .await
        .unwrap();
    let result = updates.recv().await.unwrap();
    assert_eq!(
        result.result.status(),
        convex_core::contracts::ResultStatus::Failed
    );
    let (status, _, view) = request(router.clone(), "GET", "/ready", None, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(view["runtime_ready"], false);
    assert_ne!(view["dependencies"][1]["state"], "ready");
    assert_eq!(
        request(router.clone(), "GET", "/live", None, None).await.0,
        StatusCode::OK
    );
    engine.shutdown().await.unwrap();
    issuer.finish().await;
}

#[tokio::test(flavor = "current_thread")]
async fn expiry_revocation_secret_rotation_and_all_failure_output_are_redacted() {
    use std::io::Write;
    #[derive(Clone)]
    struct Log(Arc<std::sync::Mutex<Vec<u8>>>);
    impl Write for Log {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Log {
        type Writer = Log;
        fn make_writer(&'a self) -> Log {
            self.clone()
        }
    }
    let log = Log(Arc::new(std::sync::Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .without_time()
        .with_writer(log.clone())
        .finish();
    // Capture this entire test process, including spawned HTTP tasks and parallel tests.
    // A thread-local default can miss events when Tower or Tokio polls work elsewhere.
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let issuer = IssuerFixture::start().await;
    let access = AccessControl::new(issuer.verifier.clone(), policy()).unwrap();
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    let router = convex_server::routes::create_router_with_access(engine, true, access.clone());
    let original = claims("alice");
    for (field, value) in [
        ("exp", json!(now() - 1)),
        ("iss", json!("https://wrong.example.invalid")),
        ("aud", json!("login-client")),
        ("nbf", json!(now() + 3600)),
        ("token_type", json!("id_token")),
        ("active", json!(false)),
    ] {
        let mut claim = original.clone();
        claim[field] = value;
        issuer.change(HUMAN_TOKEN, claim);
        let (status, headers, body) = request(
            router.clone(),
            "GET",
            "/api/v1/bonds/BOND-001",
            Some(HUMAN_TOKEN),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{field}");
        assert_eq!(headers["www-authenticate"], "Bearer");
        assert_eq!(body["code"], "UNAUTHENTICATED");
    }
    issuer.change(HUMAN_TOKEN, original.clone());
    let context = access
        .authenticate(Some(BearerToken::parse(HUMAN_TOKEN).unwrap()))
        .await
        .unwrap();
    let identity = context.identity().unwrap();
    assert_eq!(
        access.authorize_at(
            &context,
            identity.expires_at,
            Action::ReferenceRead,
            &[DataScope::Reference],
            &[]
        ),
        Err(AccessError::Unauthenticated)
    );
    let mut changed = policy();
    changed.version = 2;
    changed.grants[0]
        .resources
        .remove(&ResourceKind::Instrument);
    access.replace_policy(1, changed.clone()).unwrap();
    assert_eq!(
        access.authorize(
            &context,
            Action::ReferenceRead,
            &[DataScope::Reference],
            &[Resource::new(ResourceKind::Instrument, "BOND-001")]
        ),
        Err(AccessError::Forbidden)
    );
    assert_eq!(
        request(
            router.clone(),
            "GET",
            "/api/v1/bonds/BOND-001",
            Some(HUMAN_TOKEN),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        access.replace_policy(1, policy()),
        Err(AccessError::Conflict)
    );
    let mut changed = policy();
    changed.version = 3;
    access.replace_policy(2, changed).unwrap();
    let mut revoked = original;
    revoked["scope"] = json!("unrelated");
    issuer.change(HUMAN_TOKEN, revoked);
    assert_eq!(
        request(
            router.clone(),
            "GET",
            "/api/v1/bonds/BOND-001",
            Some(HUMAN_TOKEN),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    issuer.change(HUMAN_TOKEN, claims("alice"));
    std::fs::write(
        issuer
            .directory
            .path()
            .join("credential-private-path-marker"),
        "rotated-client-secret-marker",
    )
    .unwrap();
    *issuer.state.expected_basic.write().unwrap() =
        "Basic Y29udmV4LXRlc3Q6cm90YXRlZC1jbGllbnQtc2VjcmV0LW1hcmtlcg==".into();
    assert_eq!(
        request(
            router.clone(),
            "GET",
            "/api/v1/bonds/BOND-001",
            Some(HUMAN_TOKEN),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    issuer.state.unavailable.store(true, Ordering::SeqCst);
    let (status, _, body) = request(
        router.clone(),
        "GET",
        "/api/v1/bonds/BOND-001",
        Some(HUMAN_TOKEN),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "ACCESS_UNAVAILABLE");
    let diagnostics = format!(
        "{:?} {:?} {:?} {}",
        issuer.settings,
        context,
        BearerToken::parse(HUMAN_TOKEN).unwrap(),
        serde_json::to_string(&body).unwrap()
    );
    let logs = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("Access denied"));
    assert!(logs.contains("POLICY_ACTIVATED"));
    for secret in [
        HUMAN_TOKEN,
        SERVICE_TOKEN,
        "test-client-secret-marker",
        "rotated-client-secret-marker",
        "private-remote-secret-marker",
        "credential-private-path-marker",
    ] {
        assert!(!diagnostics.contains(secret));
        assert!(!logs.contains(secret), "log leaked {secret}");
    }
    issuer.finish().await;
}

#[tokio::test]
async fn quotas_are_monotonic_bounded_and_release_when_admission_is_dropped() {
    let issuer = IssuerFixture::start().await;
    let mut document = policy();
    document.grants[0].quotas = Quotas {
        requests_per_minute: 3,
        concurrent_requests: 1,
        websocket_sessions: 1,
        subscriptions: 1,
    };
    let access = AccessControl::new(issuer.verifier.clone(), document).unwrap();
    let context = access
        .authenticate(Some(BearerToken::parse(HUMAN_TOKEN).unwrap()))
        .await
        .unwrap();
    let permit = access.admit_attempt().unwrap();
    let admitted = access.admit(&context, false, permit).unwrap();
    assert!(matches!(
        access.admit(&context, false, access.admit_attempt().unwrap()),
        Err(AccessError::Quota)
    ));
    drop(admitted);
    let session = access
        .admit(&context, true, access.admit_attempt().unwrap())
        .unwrap();
    assert!(matches!(
        access.admit(&context, true, access.admit_attempt().unwrap()),
        Err(AccessError::Quota)
    ));
    drop(session);
    let session = access
        .admit(&context, true, access.admit_attempt().unwrap())
        .unwrap();
    drop(session);
    assert!(matches!(
        access.admit_message(&context),
        Err(AccessError::Quota)
    ));
    let mut permits = Vec::new();
    for _ in 0..64 {
        permits.push(access.admit_attempt().unwrap());
    }
    assert!(matches!(access.admit_attempt(), Err(AccessError::Quota)));
    drop(permits);
    assert!(access.admit_attempt().is_ok());
    issuer.finish().await;
}

/// D78 (G40): the process windows count only calls to the identity provider. Requests,
/// preflights, WS messages and public probes hold an in-flight permit and count nothing.
/// Credential verifications have 6,000 a minute, and readiness probes 600 of their own,
/// so authenticated traffic cannot make `/ready` fail.
#[tokio::test]
async fn only_identity_provider_calls_count_against_the_process_windows() {
    let issuer = IssuerFixture::start().await;
    let access = AccessControl::new(issuer.verifier.clone(), policy()).unwrap();
    for _ in 0..=VERIFICATIONS_PER_MINUTE {
        drop(access.admit_attempt().unwrap());
    }
    for _ in 0..=VERIFICATIONS_PER_MINUTE {
        assert!(matches!(
            access.authenticate(None).await,
            Err(AccessError::Unauthenticated)
        ));
    }
    let token = || Some(BearerToken::parse(HUMAN_TOKEN).unwrap());
    for _ in 0..VERIFICATIONS_PER_MINUTE {
        access.authenticate(token()).await.unwrap();
    }
    assert!(matches!(
        access.authenticate(token()).await,
        Err(AccessError::Quota)
    ));
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let router =
        convex_server::routes::create_router_with_access(engine.clone(), true, access.clone());
    assert_eq!(
        request(router.clone(), "GET", "/live", None, None).await.0,
        StatusCode::OK
    );
    // One readiness probe.
    let (status, _, view) = request(router.clone(), "GET", "/ready", None, None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    let (status, _, body) = request(
        router.clone(),
        "GET",
        "/api/v1/bonds/BOND-001",
        Some(HUMAN_TOKEN),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "ACCESS_QUOTA_EXCEEDED");
    assert_eq!(
        request(router, "GET", "/api/v1/bonds/BOND-001", None, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    for _ in 1..READINESS_PROBES_PER_MINUTE {
        assert!(access.health().await.identity_ready);
    }
    assert!(!access.health().await.identity_ready);
    engine.shutdown().await.unwrap();
    issuer.finish().await;
}

/// D78 (G40): local development verifies no credential, so no request counts.
#[tokio::test]
async fn local_development_requests_count_against_no_window() {
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    let router = convex_server::routes::create_router_with_access(
        engine,
        true,
        AccessControl::local_development(),
    );
    for _ in 0..=VERIFICATIONS_PER_MINUTE {
        let (status, _, body) = request(router.clone(), "GET", "/live", None, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
}

#[tokio::test]
async fn canonical_snapshot_input_version_resources_cannot_bypass_scoped_authorization() {
    let issuer = IssuerFixture::start().await;
    let access = AccessControl::new(issuer.verifier.clone(), policy()).unwrap();
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    let router = convex_server::routes::create_router_with_access(engine, true, access);
    // Reject an unpermitted calibration dependency before parsing/evaluating the incomplete context.
    let payload = json!({"snapshot":{"curves":[{"curve_id":"CURVE-A","source":"SOURCE-A","input_versions":[{"kind":"instrument","id":"BOND-002","version":1}]}]},"context":{"subject":{"instrument_id":"BOND-001"}}});
    assert_eq!(
        request(
            router,
            "POST",
            "/api/v1/runtime/valuation",
            Some(HUMAN_TOKEN),
            Some(payload)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    issuer.finish().await;
}

#[tokio::test]
async fn opaque_oidc_subjects_scope_expiry_refresh_identity_and_response_bounds_are_enforced() {
    let issuer = IssuerFixture::start().await;
    let mut doc = policy();
    doc.grants[0].subject = "auth0|alice".into();
    doc.grants[0].valid_until = now() + 600;
    let expiry = doc.grants[0].valid_until;
    let mut identity = claims("auth0|alice");
    identity["scope"] = json!("convex.api remote.read~%");
    issuer.change(HUMAN_TOKEN, identity.clone());
    let access = AccessControl::new(issuer.verifier.clone(), doc).unwrap();
    let ctx = access
        .authenticate(Some(BearerToken::parse(HUMAN_TOKEN).unwrap()))
        .await
        .unwrap();
    assert_eq!(ctx.identity().unwrap().subject, "auth0|alice");
    access
        .authorize_at(
            &ctx,
            expiry - 1,
            Action::ReferenceRead,
            &[DataScope::Reference],
            &[],
        )
        .unwrap();
    assert_eq!(
        access.authorize_at(
            &ctx,
            expiry,
            Action::ReferenceRead,
            &[DataScope::Reference],
            &[]
        ),
        Err(AccessError::Forbidden)
    );
    issuer.change(HUMAN_TOKEN, claims("worker"));
    assert_eq!(
        access.refresh(&ctx).await.unwrap_err(),
        AccessError::Unauthenticated,
        "a refreshed token cannot transfer a retained quota/session to another subject"
    );
    let mut oversized = identity.clone();
    oversized["private_extension"] = json!("x".repeat(16384));
    issuer.change(HUMAN_TOKEN, oversized);
    assert_eq!(
        access
            .authenticate(Some(BearerToken::parse(HUMAN_TOKEN).unwrap()))
            .await
            .unwrap_err(),
        AccessError::Unavailable
    );
    let mut malformed = identity.clone();
    malformed["scope"] = json!(42);
    issuer.change(HUMAN_TOKEN, malformed);
    assert_eq!(
        access
            .authenticate(Some(BearerToken::parse(HUMAN_TOKEN).unwrap()))
            .await
            .unwrap_err(),
        AccessError::Unavailable
    );
    for scope in [
        "convex.api\tremote.read",
        "convex.api  remote.read",
        "convex.api\n",
    ] {
        let mut malformed = identity.clone();
        malformed["scope"] = json!(scope);
        issuer.change(HUMAN_TOKEN, malformed);
        assert_eq!(
            access
                .authenticate(Some(BearerToken::parse(HUMAN_TOKEN).unwrap()))
                .await
                .unwrap_err(),
            AccessError::Unauthenticated
        );
    }
    issuer.change(HUMAN_TOKEN, identity);
    // A newline after the read bound cannot make a truncated credential appear valid.
    std::fs::write(
        issuer
            .directory
            .path()
            .join("credential-private-path-marker"),
        "x".repeat(4096) + "\nmore-secret-marker",
    )
    .unwrap();
    assert_eq!(
        access
            .authenticate(Some(BearerToken::parse(HUMAN_TOKEN).unwrap()))
            .await
            .unwrap_err(),
        AccessError::Unavailable
    );
    issuer.finish().await;
}

#[tokio::test]
async fn policy_replacement_has_one_winner_and_file_errors_do_not_retain_authority() {
    let issuer = IssuerFixture::start().await;
    let document = policy();
    let access = AccessControl::new(issuer.verifier.clone(), document.clone()).unwrap();
    let context = access
        .authenticate(Some(BearerToken::parse(HUMAN_TOKEN).unwrap()))
        .await
        .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|index| {
            let access = access.clone();
            let barrier = barrier.clone();
            let mut document = document.clone();
            document.version = 2;
            document.reason = format!("candidate {index}");
            std::thread::spawn(move || {
                barrier.wait();
                access.replace_policy(1, document)
            })
        })
        .collect();
    let outcomes: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    let mut revised = document.clone();
    revised.version = 3;
    let path = issuer.directory.path().join("policy.json");
    std::fs::write(&path, serde_json::to_vec(&revised).unwrap()).unwrap();
    let (stop, rx) = tokio::sync::watch::channel(false);
    let watcher = tokio::spawn(
        access
            .clone()
            .watch_policy(path.to_str().unwrap().into(), rx),
    );
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while access.policy_version() != Some(3) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    std::fs::remove_file(&path).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while access
            .authorize(
                &context,
                Action::ReferenceRead,
                &[DataScope::Reference],
                &[],
            )
            .is_ok()
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        access.authorize(&context, Action::ReferenceRead, &[], &[]),
        Err(AccessError::Unavailable)
    );
    std::fs::write(&path, serde_json::to_vec(&revised).unwrap()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while access
            .authorize(
                &context,
                Action::ReferenceRead,
                &[DataScope::Reference],
                &[],
            )
            .is_err()
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    std::fs::write(&path, r#"{"schema_version":1,"schema_version":1}"#).unwrap();
    assert_eq!(
        AccessControl::read_policy(path.to_str().unwrap())
            .await
            .unwrap_err(),
        AccessError::InvalidPolicy
    );
    stop.send(true).unwrap();
    watcher.await.unwrap();
    let mut bad = document.clone();
    bad.grants.push(bad.grants[0].clone());
    assert_eq!(bad.validate(), Err(AccessError::InvalidPolicy));
    let mut bad = document;
    bad.grants[0]
        .resources
        .insert(ResourceKind::Book, vec!["*".into(), "BOOK-A".into()]);
    assert_eq!(bad.validate(), Err(AccessError::InvalidPolicy));
    issuer.finish().await;
}

#[tokio::test]
async fn bootstrap_curve_label_is_not_a_scoped_curve() {
    let issuer = IssuerFixture::start().await;
    let mut document = policy();
    for grant in &mut document.grants {
        grant
            .resources
            .insert(ResourceKind::Curve, vec!["CURVE-A".into()]);
    }
    let access = AccessControl::new(issuer.verifier.clone(), document).unwrap();
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    let router = convex_server::routes::create_router_with_access(engine, true, access);
    let payload = json!({"curve_id":"UNGRANTED-LABEL","reference_date":"2026-01-02","instruments":[
        {"type":"Deposit","tenor":0.5,"quote":0.04},
        {"type":"Swap","tenor":2.0,"quote":0.041}]});
    let (status, _, body) = request(
        router.clone(),
        "POST",
        "/api/v1/curves/bootstrap",
        Some(HUMAN_TOKEN),
        Some(payload),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["curve_id"], "UNGRANTED-LABEL");
    let read = request(
        router,
        "GET",
        "/api/v1/curves/UNGRANTED-LABEL",
        Some(HUMAN_TOKEN),
        None,
    )
    .await;
    assert_eq!(read.0, StatusCode::FORBIDDEN, "a stored curve stays scoped");
}

#[tokio::test]
async fn metrics_need_the_operator_role_and_the_operations_grant() {
    use http_body_util::BodyExt;
    let issuer = IssuerFixture::start().await;
    let access = AccessControl::new(issuer.verifier.clone(), policy()).unwrap();
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    assert!(convex_server::telemetry::install().is_some());
    // A refused request-time context still records its timing.
    assert!(engine
        .request_context(None, support::time(0))
        .await
        .is_err());
    let router = convex_server::routes::create_router_with_access(engine, true, access);
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(SERVICE_TOKEN), StatusCode::FORBIDDEN),
    ] {
        let (status, _, body) = request(router.clone(), "GET", "/metrics", token, None).await;
        assert_eq!(status, expected, "{token:?} {body}");
    }
    let response = router
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .header("authorization", format!("Bearer {HUMAN_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/plain; version=0.0.4; charset=utf-8"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(
        body.contains("# TYPE convex_request_context_seconds histogram")
            && body.contains("convex_request_context_seconds_bucket{le="),
        "{body}"
    );
}

#[tokio::test]
async fn live_quote_ingress_needs_writer_market_and_each_event_scope() {
    let issuer = IssuerFixture::start().await;
    let mut document = policy();
    document.grants[0]
        .resources
        .insert(ResourceKind::Source, vec!["feed-a".into()]);
    let access = AccessControl::new(issuer.verifier.clone(), document).unwrap();
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    let router = convex_server::routes::create_router_with_access(engine, true, access);
    let stamp = chrono::Utc::now().timestamp_millis();
    let event = support::fixture::event(1, stamp, 0, "99.25");
    let batch =
        |event: &convex_engine::ports::quote_event::QuoteEvent| Some(json!({ "events": [event] }));
    // Admitted: the router has no live runtime, so the handler itself answers 503.
    let (status, _, body) = request(
        router.clone(),
        "POST",
        "/api/v1/market/quotes",
        Some(HUMAN_TOKEN),
        batch(&event),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"], "this server has no live runtime");
    let mut other_source = event.clone();
    other_source.key.source = "feed-b".into();
    let mut other_instrument = event.clone();
    other_instrument.key.instrument_id = convex_core::ids::InstrumentId::new("BOND-002");
    for (token, event) in [
        (HUMAN_TOKEN, &other_source),
        (HUMAN_TOKEN, &other_instrument),
        (SERVICE_TOKEN, &event),
    ] {
        let (status, _, body) = request(
            router.clone(),
            "POST",
            "/api/v1/market/quotes",
            Some(token),
            batch(event),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
}

#[tokio::test]
async fn configuration_writes_need_the_operator_role_and_the_configuration_grant() {
    let issuer = IssuerFixture::start().await;
    let access = AccessControl::new(issuer.verifier.clone(), policy()).unwrap();
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    let router = convex_server::routes::create_router_with_access(engine, true, access);
    let body = Some(json!({"reason": "Fictional access check"}));
    // The reader-only service identity can neither inspect nor change configuration.
    for (method, path) in [
        ("POST", "/api/v1/configuration/CONFIG-RESTORE/stage"),
        ("POST", "/api/v1/configuration/CONFIG-RESTORE/activate"),
        ("POST", "/api/v1/configuration/CONFIG-RESTORE/rollback"),
        ("POST", "/api/v1/configuration/validate"),
        (
            "GET",
            "/api/v1/configuration/CONFIG-RESTORE/history?offset=0&limit=10",
        ),
        (
            "GET",
            "/api/v1/configuration/CONFIG-RESTORE/diff?from=1&to=2",
        ),
        ("POST", "/api/v1/reference/import"),
    ] {
        let payload = (method == "POST").then(|| body.clone()).flatten();
        let (status, _, _) =
            request(router.clone(), method, path, Some(SERVICE_TOKEN), payload).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
    }
    // The operator is refused a configuration it holds no grant for.
    let (status, _, _) = request(
        router.clone(),
        "POST",
        "/api/v1/configuration/OTHER-CONFIG/stage",
        Some(HUMAN_TOKEN),
        body.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // Admitted, the handler answers: the body is not a staging request.
    let (status, _, refused) = request(
        router.clone(),
        "POST",
        "/api/v1/configuration/CONFIG-RESTORE/stage",
        Some(HUMAN_TOKEN),
        body,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_eq!(refused["error"]["code"], "CONFIGURATION_INVALID");
    // This composition has no configuration use cases.
    let (status, _, unavailable) = request(
        router.clone(),
        "GET",
        "/api/v1/configuration/CONFIG-RESTORE/history?offset=0&limit=10",
        Some(HUMAN_TOKEN),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{unavailable}");
    // Listing every master resource of a kind needs an instrument wildcard grant.
    let (status, _, _) = request(
        router,
        "GET",
        "/api/v1/reference/masters/calendar",
        Some(HUMAN_TOKEN),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
