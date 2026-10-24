#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;
use axum::http::{header, Request, StatusCode};
use convex_server::access::identity::{AccessMode, AccessSettings};
use convex_server::{access::AccessControl, ConfigError, Server, ServerConfig};
use serde_json::Value;
use tower::ServiceExt;

const IMPORT: &str = include_str!("../../convex-ext-file/tests/fixtures/instrument-master.json");

async fn send(router: &axum::Router, request: Request<axum::body::Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn upload(body: impl Into<axum::body::Body>) -> Request<axum::body::Body> {
    Request::builder()
        .method("POST")
        .uri("/api/v1/reference/import")
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.into())
        .unwrap()
}

fn read(path: &str) -> Request<axum::body::Body> {
    Request::builder()
        .uri(path)
        .body(axum::body::Body::empty())
        .unwrap()
}

#[tokio::test]
async fn an_uploaded_import_commits_once_reports_rows_and_lists_masters_by_kind() {
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
    // A bad row is located and nothing commits.
    let mut invalid: Value = serde_json::from_str(IMPORT).unwrap();
    invalid["command"]["changes"][0]["bond"]["day_count"] = "UNKNOWN".into();
    let (status, refused) = send(&router, upload(serde_json::to_vec(&invalid).unwrap())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_eq!(refused["diagnostics"][0]["section"], "changes");
    assert_eq!(refused["diagnostics"][0]["row"], 0);
    assert_eq!(refused["diagnostics"][0]["field"], "day_count");
    let (status, _) = send(&router, read("/api/v1/bonds/INSTRUMENT-001")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, receipt) = send(&router, upload(IMPORT)).await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    // The file's own idempotency key makes a re-upload return the retained receipt.
    let (status, again) = send(&router, upload(IMPORT)).await;
    assert_eq!((status, again), (StatusCode::OK, receipt));
    let oversized = vec![b' '; convex_ext_file::MAX_REFERENCE_IMPORT_BYTES as usize + 1];
    let (status, _) = send(&router, upload(oversized)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let (status, calendars) = send(&router, read("/api/v1/reference/masters/calendar")).await;
    assert_eq!(status, StatusCode::OK, "{calendars}");
    let imported: Value = serde_json::from_str(IMPORT).unwrap();
    let calendar = imported["command"]["master_changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["definition"]["kind"] == "calendar")
        .unwrap()["definition"]["definition"]["calendar_id"]
        .clone();
    // The setup's own calendar is listed beside the imported one, in identity order.
    let id = |item: &Value| item["definition"]["definition"]["calendar_id"].clone();
    let items = calendars["items"].as_array().unwrap();
    assert!(items.iter().any(|item| id(item) == calendar), "{calendars}");
    assert!(items
        .windows(2)
        .all(|pair| id(&pair[0]).as_str() < id(&pair[1]).as_str()));
    assert_eq!(calendars["next"], Value::Null);
    let (_, first) = send(
        &router,
        read("/api/v1/reference/masters/identifier?limit=1"),
    )
    .await;
    let next = first["next"].as_str().unwrap().to_owned();
    let (_, second) = send(
        &router,
        read(&format!(
            "/api/v1/reference/masters/identifier?limit=1&after={next}"
        )),
    )
    .await;
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert_ne!(second["items"][0], first["items"][0]);
    for refused in [
        "/api/v1/reference/masters/book",
        "/api/v1/reference/masters/unknown",
        "/api/v1/reference/masters/calendar?limit=0",
        "/api/v1/reference/masters/calendar?page=1",
    ] {
        let (status, body) = send(&router, read(refused)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}: {body}");
    }
}

#[tokio::test]
async fn the_admin_app_is_served_only_when_configured_with_a_restrictive_policy() {
    let s = support::setup().await;
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        s.plan.clone(),
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    let ui = tempfile::tempdir().unwrap();
    std::fs::write(
        ui.path().join("index.html"),
        "<!doctype html><title>admin</title>",
    )
    .unwrap();
    std::fs::create_dir(ui.path().join("assets")).unwrap();
    std::fs::write(ui.path().join("assets").join("app.js"), "export {};").unwrap();
    let origin = "http://127.0.0.1:8080";
    let server = |admin_ui_dir: Option<String>| {
        Server::with_access_control(
            ServerConfig {
                host: "127.0.0.1".into(),
                admin_ui_dir,
                access: AccessSettings {
                    cors_origins: vec![origin.into()],
                    ..Default::default()
                },
                ..Default::default()
            },
            engine.clone(),
            AccessControl::local_development(),
        )
        .router()
    };
    let configured = server(Some(ui.path().to_string_lossy().into_owned()));
    for (path, kind) in [
        ("/admin/", "text/html"),
        ("/admin/configuration/history", "text/html"),
        ("/admin/assets/app.js", "javascript"),
    ] {
        let response = configured.clone().oneshot(read(path)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let headers = response.headers();
        assert!(headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .contains(kind));
        let policy = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(policy.contains("script-src 'self'") && policy.contains("frame-ancestors 'none'"));
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    }
    // A module script is fetched with the page's Origin, which must be listed.
    let script = |from: &str| {
        Request::builder()
            .uri("/admin/assets/app.js")
            .header(header::ORIGIN, from)
            .body(axum::body::Body::empty())
            .unwrap()
    };
    let response = configured.clone().oneshot(script(origin)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = configured
        .clone()
        .oneshot(script("http://elsewhere.example"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = configured
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    let response = server(None).oneshot(read("/admin/")).await.unwrap();
    assert_ne!(response.status(), StatusCode::OK);
    assert!(response
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .is_none());
    // Startup refuses a directory without the built app.
    let empty = tempfile::tempdir().unwrap();
    let mut config = ServerConfig {
        host: "127.0.0.1".into(),
        profile: convex_server::ServerProfile::Demo,
        access: AccessSettings {
            mode: AccessMode::LocalDevelopment,
            ..Default::default()
        },
        ..Default::default()
    };
    config.validate_startup().unwrap();
    config.admin_ui_dir = Some(empty.path().to_string_lossy().into_owned());
    assert!(matches!(
        config.validate_startup(),
        Err(ConfigError::InvalidValue {
            field: "admin_ui_dir",
            ..
        })
    ));
    // The page's origin must be listed, or its own script and writes are refused.
    config.admin_ui_dir = Some(ui.path().to_string_lossy().into_owned());
    assert!(matches!(
        config.validate_startup(),
        Err(ConfigError::InvalidValue {
            field: "access.cors_origins",
            ..
        })
    ));
    config.access.cors_origins = vec![origin.into()];
    config.validate_startup().unwrap();
}
