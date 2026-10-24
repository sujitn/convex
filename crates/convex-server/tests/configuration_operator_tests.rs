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
async fn http_configuration_inspection_uses_same_policy_and_exposes_no_mutation_route() {
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
