#[path = "../../convex-engine/tests/support/configuration_setup.rs"]
mod support;
use convex_core::contracts::*;
use convex_engine::ports::configuration::*;
use convex_server::{configuration_admin::effective_configuration, SERVER_BUILD_ID};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::{Command, Output},
};
use support::*;
use tower::ServiceExt;
fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_convex-config"))
        .args(arguments)
        .output()
        .unwrap()
}
fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}
fn output_json(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn write(path: &Path, value: &impl serde::Serialize) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

#[tokio::test]
async fn separate_process_cli_stage_activate_rollback_inspection_and_export_are_durable() {
    let setup = runtime::setup().await;
    let mut first = fixture::staged(1);
    first.document.runtime.mode = RuntimeMode::Replay;
    first.document.runtime.build_id = ModelId::new(SERVER_BUILD_ID).unwrap();
    first.document.providers[0]
        .capabilities
        .push(ProviderCapability::Replay);
    let mut second = first.clone();
    second.idempotency_key = RequestId::new("stage-2").unwrap();
    second.document.pricing.version = 2;
    second.document.pricing.updated_at = 200;
    second.document.assembly.configuration.version = Version::new(2).unwrap();
    second.document.assembly.quotes[0].max_age_ms = 3000;
    let dir = setup.dir.path();
    let document = dir.join("document.json");
    let staged1 = dir.join("stage1.json");
    let staged2 = dir.join("stage2.json");
    let cases_path = dir.join("fixtures.json");
    let replay = dir.join("quotes.jsonl");
    let command1 = dir.join("activate1.json");
    let command2 = dir.join("activate2.json");
    let rollback = dir.join("rollback.json");
    let db = dir.join("inputs.redb");
    write(&document, &first.document);
    write(&staged1, &first);
    write(&staged2, &second);
    write(&cases_path, &fixtures(&setup));
    std::fs::write(
        &replay,
        setup
            .plan
            .seed_events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    write(&command1, &fixture::activate(1, 0, "first"));
    write(&command2, &fixture::activate(2, 1, "second"));
    let mut request = fixture::activate(1, 2, "rollback");
    request.action = ActivationKind::Rollback {
        to_revision: Version::new(1).unwrap(),
    };
    write(&rollback, &request);
    let owned_dir = setup.dir;
    drop(setup.storage);
    drop(setup.store);
    drop(setup.snapshots);
    drop(setup.results);
    drop(setup.fixings);
    let validate = output_json(run(&[
        text(&db),
        "validate",
        text(&document),
        text(&cases_path),
        text(&replay),
    ]));
    assert_eq!(validate["valid"], true);
    output_json(run(&[
        text(&db),
        "stage",
        text(&staged1),
        text(&cases_path),
        text(&replay),
    ]));
    let empty = run(&[
        text(&db),
        "effective",
        "CONFIG-RESTORE",
        "2026-10-02T15:00:00Z",
        "2026-10-02T15:00:00Z",
    ]);
    assert!(!empty.status.success());
    let first = output_json(run(&[
        text(&db),
        "activate",
        text(&command1),
        text(&cases_path),
        text(&replay),
    ]));
    assert_eq!(first["activation"]["revision"], 1);
    output_json(run(&[
        text(&db),
        "stage",
        text(&staged2),
        text(&cases_path),
        text(&replay),
    ]));
    let before = output_json(run(&[
        text(&db),
        "effective",
        "CONFIG-RESTORE",
        "2026-10-02T15:00:00Z",
        "2026-10-02T15:00:00Z",
    ]));
    assert_eq!(before["configuration"]["pricing"]["version"], 1);
    output_json(run(&[
        text(&db),
        "activate",
        text(&command2),
        text(&cases_path),
        text(&replay),
    ]));
    let reverted = output_json(run(&[
        text(&db),
        "rollback",
        text(&rollback),
        text(&cases_path),
        text(&replay),
    ]));
    assert_eq!(reverted["activation"]["revision"], 3);
    assert_eq!(
        reverted["activation"]["command"]["configuration_version"],
        1
    );
    assert_eq!(
        output_json(run(&[
            text(&db),
            "rollback",
            text(&rollback),
            text(&cases_path),
            text(&replay)
        ])),
        reverted
    );
    let history = output_json(run(&[text(&db), "history", "CONFIG-RESTORE", "0", "100"]));
    assert_eq!(history["total"], 3);
    let retained = output_json(run(&[text(&db), "version", "CONFIG-RESTORE", "2"]));
    assert_eq!(retained["configuration"]["pricing"]["version"], 2);
    assert_eq!(retained["actor"], "fixture-operator");
    let exported = output_json(run(&[text(&db), "export-plan", "CONFIG-RESTORE"]));
    let plan: convex_engine::RuntimePlan = serde_json::from_value(exported).unwrap();
    plan.validate().unwrap();
    assert_eq!(plan.assembly.configuration.version.get(), 1);
    assert_eq!(plan.build_id.as_str(), SERVER_BUILD_ID);
    let storage = convex_ext_redb::create_redb_storage(&db).unwrap();
    assert_eq!(
        storage
            .configuration
            .active(&request.id)
            .await
            .unwrap()
            .unwrap()
            .activation
            .revision
            .get(),
        3
    );
    assert!(owned_dir.path().exists());
}

#[tokio::test]
async fn cli_invalid_schema_credentials_fixture_and_process_values_are_redacted() {
    let setup = runtime::setup().await;
    let mut document = fixture::document(1);
    document.runtime.build_id = ModelId::new(SERVER_BUILD_ID).unwrap();
    document.providers[0].credential =
        Some(SecretReference::new("env://PRIVATE_MARKER".into()).unwrap());
    let path = setup.dir.path().join("private-document.json");
    let cases_path = setup.dir.path().join("fixtures.json");
    let replay = setup.dir.path().join("quotes.jsonl");
    let db = setup.dir.path().join("inputs.redb");
    write(&path, &document);
    write(&cases_path, &fixtures(&setup));
    std::fs::write(
        &replay,
        serde_json::to_string(&setup.plan.seed_events[0]).unwrap() + "\n",
    )
    .unwrap();
    let owned_dir = setup.dir;
    drop(setup.storage);
    drop(setup.store);
    drop(setup.snapshots);
    drop(setup.results);
    drop(setup.fixings);
    let output = run(&[
        text(&db),
        "validate",
        text(&path),
        text(&cases_path),
        text(&replay),
    ]);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE_MARKER"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_MARKER"));
    let mut raw = serde_json::to_value(&document).unwrap();
    raw["providers"][0]["credential"] = json!("INLINE_SECRET_MARKER");
    write(&path, &raw);
    let output = run(&[
        text(&db),
        "validate",
        text(&path),
        text(&cases_path),
        text(&replay),
    ]);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("INLINE_SECRET_MARKER"));
    raw["providers"][0]["credential"] = Value::Null;
    raw["price_override"] = json!(0);
    write(&path, &raw);
    assert!(!run(&[
        text(&db),
        "validate",
        text(&path),
        text(&cases_path),
        text(&replay)
    ])
    .status
    .success());
    let process = owned_dir.path().join("process.toml");
    std::fs::write(
        &process,
        "profile='demo'\nhost='127.0.0.1'\nstorage_path='/PRIVATE_PROCESS_MARKER'\n[access]\nmode='local_development'\n",
    )
    .unwrap();
    let inspected = output_json(run(&["process", "explain", text(&process)]));
    assert_eq!(inspected["startup_supported"], true);
    assert!(!inspected.to_string().contains("PRIVATE_PROCESS_MARKER"));
    let schema = output_json(run(&["schema"]));
    assert_eq!(schema["$id"], "urn:convex:configuration:1");
    assert_eq!(schema["additionalProperties"], false);
}

#[tokio::test]
async fn http_configuration_inspection_uses_same_policy_and_redacts_credentials() {
    let setup = runtime::setup().await;
    let service = cases(&setup);
    let mut staged = fixture::staged(1);
    staged.document.providers[0].credential =
        Some(SecretReference::new("env://PRIVATE_MARKER".into()).unwrap());
    // Retained unsupported content is inspectable while runtime capability admission remains closed.
    service.repository().stage(&staged).await.unwrap();
    service
        .repository()
        .activate(&fixture::activate(1, 0, "inspect"))
        .await
        .unwrap();
    let engine = engine(&setup, service.clone());
    let router = create_router(engine);
    let expected = effective_configuration(
        &service,
        &SourceId::new("CONFIG-RESTORE").unwrap(),
        time(0),
        time(0),
    )
    .await
    .unwrap();
    let response = router.clone().oneshot(axum::http::Request::builder().uri("/api/v1/configuration/CONFIG-RESTORE?as_of=2026-10-02T15:00:00Z&known_at=2026-10-02T15:00:00Z").body(axum::body::Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), expected);
    assert!(!std::str::from_utf8(&bytes)
        .unwrap()
        .contains("PRIVATE_MARKER"));
    let response = router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/v1/configuration/CONFIG-RESTORE")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::METHOD_NOT_ALLOWED
    );
    let response = router
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/v1/configuration/CONFIG-RESTORE")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
}

fn create_router(engine: std::sync::Arc<convex_engine::PricingEngine>) -> axum::Router {
    convex_server::routes::create_router_with_access(
        engine,
        true,
        convex_server::access::AccessControl::local_development(),
    )
}

async fn post(
    router: &axum::Router,
    path: &str,
    key: Option<&str>,
    if_match: Option<&str>,
    body: Value,
) -> (axum::http::StatusCode, Value) {
    let mut request = axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if let Some(key) = key {
        request = request.header("idempotency-key", key);
    }
    if let Some(revision) = if_match {
        request = request.header("if-match", revision);
    }
    let response = router
        .clone()
        .oneshot(
            request
                .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn get(router: &axum::Router, path: &str) -> (axum::http::StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(path)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn http_stage_activate_rollback_record_the_caller_and_whether_fixtures_were_replay_checked() {
    let setup = runtime::setup().await;
    let service = cases(&setup);
    let engine = engine(&setup, service.clone());
    let mut first = fixture::staged(1);
    first.document.runtime.mode = RuntimeMode::Replay;
    first.document.runtime.build_id = ModelId::new(SERVER_BUILD_ID).unwrap();
    first.document.providers[0]
        .capabilities
        .push(ProviderCapability::Replay);
    let mut second = first.clone();
    second.document.pricing.version = 2;
    second.document.pricing.updated_at = 200;
    second.document.assembly.configuration.version = Version::new(2).unwrap();
    second.document.assembly.quotes[0].max_age_ms = 3000;
    let replay = setup.dir.path().join("quotes.jsonl");
    std::fs::write(
        &replay,
        setup
            .plan
            .seed_events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    let fixtures = fixtures(&setup);
    // A replay composition checks fixtures against its quotes file; a live one cannot.
    let traced = create_router(engine.clone()).layer(axum::Extension(
        convex_server::configuration_admin::ConfigurationTrace(replay),
    ));
    let untraced = create_router(engine.clone());
    let base = "/api/v1/configuration/CONFIG-RESTORE";
    let stage = |staged: &StagedConfiguration,
                 fixtures: &[convex_engine::configuration::ConfigurationFixture]| {
        json!({"document": staged.document, "fixtures": fixtures, "reason": staged.reason,
            "staged_at": staged.staged_at})
    };
    let (status, report) = post(
        &traced,
        "/api/v1/configuration/validate",
        None,
        None,
        json!({"document": first.document, "fixtures": fixtures}),
    )
    .await;
    assert_eq!(status, 200, "{report}");
    assert_eq!(report["valid"], true);
    let (status, diff) = post(
        &untraced,
        "/api/v1/configuration/diff",
        None,
        None,
        json!({"previous": first.document, "next": second.document}),
    )
    .await;
    assert_eq!(status, 200, "{diff}");
    assert!(!diff["changes"].as_array().unwrap().is_empty());
    // Fixtures that are not a prefix of the replay are refused.
    let mut foreign = fixtures.clone();
    foreign[0].events[0].key.source = "foreign-feed".into();
    let (status, refused) = post(
        &traced,
        &format!("{base}/stage"),
        Some("stage-foreign"),
        None,
        stage(&first, &foreign),
    )
    .await;
    assert_eq!(status, 400, "{refused}");
    assert_eq!(
        refused["error"]["reason"],
        "fixture trace differs from actual canonical replay prefix"
    );
    let (status, missing_key) = post(
        &traced,
        &format!("{base}/stage"),
        None,
        None,
        stage(&first, &fixtures),
    )
    .await;
    assert_eq!(status, 400, "{missing_key}");
    let (status, staged) = post(
        &traced,
        &format!("{base}/stage"),
        Some("stage-1"),
        None,
        stage(&first, &fixtures),
    )
    .await;
    assert_eq!(
        (status, &staged["valid"]),
        (axum::http::StatusCode::OK, &json!(true)),
        "{staged}"
    );
    let (status, _) = post(
        &untraced,
        &format!("{base}/stage"),
        Some("stage-2"),
        None,
        stage(&second, &fixtures),
    )
    .await;
    assert_eq!(status, 200);
    let (_, retained) = get(&untraced, &format!("{base}/versions/1")).await;
    assert_eq!(retained["actor"], "local-development");
    assert_eq!(retained["fixture_evidence"], "replay_checked");
    let (_, retained) = get(&untraced, &format!("{base}/versions/2")).await;
    assert_eq!(retained["fixture_evidence"], "operator_supplied");
    let selection = |version: u64, mode: &str| {
        json!({"configuration_version": version, "reason": "Fictional HTTP activation",
            "activated_at": "2026-10-02T15:00:00Z", "mode": mode, "fixtures": fixtures})
    };
    let (status, first_activation) = post(
        &traced,
        &format!("{base}/activate"),
        Some("activate-1"),
        Some("0"),
        selection(1, "restart_required"),
    )
    .await;
    assert_eq!(status, 200, "{first_activation}");
    assert_eq!(first_activation["activation"]["revision"], 1);
    assert_eq!(first_activation["restart_required"], true);
    // The If-Match revision is the compare-and-set expectation.
    let (status, stale) = post(
        &untraced,
        &format!("{base}/activate"),
        Some("activate-stale"),
        Some("0"),
        selection(2, "restart_required"),
    )
    .await;
    assert_eq!(status, 409, "{stale}");
    let (status, second_activation) = post(
        &untraced,
        &format!("{base}/activate"),
        Some("activate-2"),
        Some("1"),
        selection(2, "hot"),
    )
    .await;
    assert_eq!(status, 200, "{second_activation}");
    assert_eq!(
        second_activation["activation"]["command"]["fixture_evidence"],
        "operator_supplied"
    );
    let rollback = json!({"to_revision": 1, "reason": "Fictional HTTP rollback",
        "activated_at": "2026-10-02T15:00:00Z", "mode": "hot", "fixtures": fixtures});
    let (status, reverted) = post(
        &traced,
        &format!("{base}/rollback"),
        Some("rollback-1"),
        Some("2"),
        rollback.clone(),
    )
    .await;
    assert_eq!(status, 200, "{reverted}");
    assert_eq!(reverted["activation"]["revision"], 3);
    assert_eq!(
        reverted["activation"]["command"]["configuration_version"],
        1
    );
    assert_eq!(
        reverted["activation"]["command"]["actor"],
        "local-development"
    );
    assert!(reverted["activation"]["command"]
        .get("fixture_evidence")
        .is_none());
    // An exact retry returns the retained record.
    let (status, retried) = post(
        &traced,
        &format!("{base}/rollback"),
        Some("rollback-1"),
        Some("2"),
        rollback,
    )
    .await;
    assert_eq!((status, retried), (axum::http::StatusCode::OK, reverted));
    let (status, history) = get(&untraced, &format!("{base}/history?offset=0&limit=10")).await;
    assert_eq!(status, 200, "{history}");
    assert_eq!(history["total"], 3);
    assert_eq!(
        service
            .active(&SourceId::new("CONFIG-RESTORE").unwrap())
            .await
            .unwrap()
            .unwrap()
            .activation
            .revision
            .get(),
        3
    );
}
