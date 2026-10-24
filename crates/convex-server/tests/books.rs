//! Durable books over real HTTP and the real composition: snapshot ingest, optimistic
//! concurrency, idempotency, atomic row rejection, history, the portfolio alias and
//! analytics by book. Inputs are the committed showcase seed.
use convex_server::access::identity::{AccessMode, AccessSettings};
use convex_server::{compose, ServerConfig, ServerProfile, StartupError};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const INSTRUMENT: &str = "INSTRUMENT-001";

fn seed() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/showcase/seed.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

struct Running {
    base: String,
    client: Client,
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), StartupError>>,
}

impl Running {
    async fn start(storage: &Path) -> Self {
        let config = ServerConfig {
            profile: ServerProfile::Demo,
            access: AccessSettings {
                mode: AccessMode::LocalDevelopment,
                ..Default::default()
            },
            host: "127.0.0.1".into(),
            port: 0,
            storage_path: storage.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let composition = compose(config).await.unwrap();
        composition.engine.start().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (shutdown, stopped) = tokio::sync::oneshot::channel();
        let server = composition.server;
        let task = tokio::spawn(async move {
            server
                .serve_until(listener, async {
                    let _ = stopped.await;
                })
                .await
        });
        Self {
            base,
            client: Client::builder().no_proxy().build().unwrap(),
            shutdown,
            task,
        }
    }

    async fn stop(self) {
        let _ = self.shutdown.send(());
        self.task.await.unwrap().unwrap();
    }

    /// One request; returns status, ETag (if any) and the JSON body (Null if empty).
    async fn send(
        &self,
        method: Method,
        path: &str,
        key: Option<&str>,
        if_match: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Option<String>, Value) {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .header("content-type", "application/json");
        if let Some(key) = key {
            request = request.header("idempotency-key", key);
        }
        if let Some(if_match) = if_match {
            request = request.header("if-match", if_match);
        }
        if let Some(body) = body {
            request = request.body(serde_json::to_vec(&body).unwrap());
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let etag = response
            .headers()
            .get("etag")
            .map(|v| v.to_str().unwrap().to_owned());
        let bytes = response.bytes().await.unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, etag, body)
    }

    async fn put(
        &self,
        id: &str,
        key: &str,
        if_match: Option<&str>,
        positions: Value,
    ) -> (StatusCode, Option<String>, Value) {
        self.send(
            Method::PUT,
            &format!("/api/v1/books/{id}"),
            Some(key),
            if_match,
            Some(json!({ "positions": positions })),
        )
        .await
    }

    async fn get(&self, path: &str) -> (StatusCode, Option<String>, Value) {
        self.send(Method::GET, path, None, None, None).await
    }
}

fn storage() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.redb");
    (directory, path)
}

async fn seeded() -> (tempfile::TempDir, PathBuf, Running) {
    let (directory, path) = storage();
    let server = Running::start(&path).await;
    for reference in seed()["references"].as_array().unwrap() {
        let command = &reference["command"];
        let key = command["idempotency_key"].as_str().unwrap();
        let (status, _, body) = server
            .send(
                Method::POST,
                "/api/v1/reference/transactions",
                Some(key),
                None,
                Some(command.clone()),
            )
            .await;
        assert!(status.is_success(), "{key}: {status} {body}");
    }
    (directory, path, server)
}

fn row(quantity: &str) -> Value {
    json!({"instrument_id": INSTRUMENT, "quantity": quantity})
}

#[tokio::test]
async fn absent_if_match_creates_and_present_if_match_is_compare_and_set() {
    let (_directory, _path, server) = seeded().await;
    let (status, etag, body) = server.put("B1", "k-1", None, json!([row("1000")])).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(etag.as_deref(), Some("\"1\""));
    assert_eq!(body["resolutions"][0]["instrument_version"], 1);

    // Create-only: a second create under another key finds the book and conflicts.
    let (status, _, body) = server.put("B1", "k-2", None, json!([row("2000")])).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "VERSION_CONFLICT");

    // A stale or wrong expected version conflicts; the exact current one replaces.
    let (status, _, _) = server
        .put("B1", "k-3", Some("\"7\""), json!([row("2000")]))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, etag, _) = server
        .put("B1", "k-4", Some("\"1\""), json!([row("2000")]))
        .await;
    assert_eq!((status, etag.as_deref()), (StatusCode::OK, Some("\"2\"")));
    let (status, _, _) = server
        .put("B1", "k-5", Some("\"1\""), json!([row("3000")]))
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "stale If-Match must lose the race"
    );

    // Idempotency-Key is mandatory; malformed If-Match is a client error.
    let (status, _, _) = server
        .send(
            Method::PUT,
            "/api/v1/books/B1",
            None,
            Some("\"2\""),
            Some(json!({"positions": []})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, _) = server.put("B1", "k-6", Some("abc"), json!([])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    server.stop().await;
}

#[tokio::test]
async fn retries_return_the_original_receipt_and_never_apply_twice() {
    let (_directory, _path, server) = seeded().await;
    let first = server
        .put("B2", "retry-1", None, json!([row("1000")]))
        .await;
    assert_eq!(first.0, StatusCode::CREATED);
    let again = server
        .put("B2", "retry-1", None, json!([row("1000")]))
        .await;
    assert_eq!((again.0, again.1.clone()), (StatusCode::CREATED, first.1));
    assert_eq!(again.2["recorded_at"], first.2["recorded_at"]);

    // The same key with another body is a payload conflict, not a second version.
    let (status, _, _) = server.put("B2", "retry-1", None, json!([row("9")])).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // A replace retried with its key and If-Match also replays.
    let replace = server
        .put("B2", "retry-2", Some("\"1\""), json!([row("2000")]))
        .await;
    assert_eq!(replace.0, StatusCode::OK);
    let replay = server
        .put("B2", "retry-2", Some("\"1\""), json!([row("2000")]))
        .await;
    assert_eq!((replay.0, replay.1), (StatusCode::OK, replace.1));
    let (_, etag, current) = server.get("/api/v1/books/B2").await;
    assert_eq!(etag.as_deref(), Some("\"2\""), "{current}");
    server.stop().await;
}

#[tokio::test]
async fn one_bad_row_commits_nothing_and_every_bad_row_is_reported() {
    let (_directory, _path, server) = seeded().await;
    server.put("B3", "ok-1", None, json!([row("1000")])).await;

    let rows = json!([
        row("1"),
        {"instrument_id": "NOT-A-BOND", "quantity": "1"},
        {"identifier": {"kind": "isin", "value": "NO-SUCH"}, "quantity": "1"},
        {"instrument_id": INSTRUMENT, "identifier": {"kind": "isin", "value": "OLD-ISIN"}, "quantity": "1"},
        {"quantity": "1"},
        row("2"),
        {"instrument_id": "OTHER", "reference_version": 9, "quantity": "1"},
    ]);
    let (status, _, body) = server.put("B3", "bad-1", Some("\"1\""), rows).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "BOOK_ROWS_REJECTED");
    let reported: Vec<(u64, &str)> = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["index"].as_u64().unwrap(), r["code"].as_str().unwrap()))
        .collect();
    assert_eq!(
        reported,
        vec![
            (1, "UNKNOWN_INSTRUMENT"),
            (2, "UNKNOWN_IDENTIFIER"),
            (3, "INVALID_ROW"),
            (4, "INVALID_ROW"),
            (5, "DUPLICATE_INSTRUMENT"),
            (6, "UNKNOWN_INSTRUMENT"),
        ]
    );
    // Still version 1, with the original content.
    let (_, etag, current) = server.get("/api/v1/books/B3").await;
    assert_eq!(etag.as_deref(), Some("\"1\""));
    assert_eq!(current["positions"][0]["quantity"], "1000");
    server.stop().await;
}

#[tokio::test]
async fn pinned_versions_must_exist_and_ambiguous_identifiers_are_never_guessed() {
    let (_directory, _path, server) = seeded().await;
    let pinned = json!([{"instrument_id": INSTRUMENT, "reference_version": 1, "quantity": "1"}]);
    let (status, _, body) = server.put("B4", "pin-1", None, pinned).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["positions"][0]["reference_version"], 1);
    let missing = json!([{"instrument_id": INSTRUMENT, "reference_version": 5, "quantity": "1"}]);
    let (status, _, body) = server.put("B4", "pin-2", Some("\"1\""), missing).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["rows"][0]["code"], "UNKNOWN_REFERENCE_VERSION");

    // A second active assertion for the same ISIN makes it ambiguous.
    let (status, _, body) = server
        .send(
            Method::POST,
            "/api/v1/reference/transactions",
            Some("dup-mapping"),
            None,
            Some(json!({
                "idempotency_key": "dup-mapping",
                "changes": [],
                "master_changes": [{
                    "expected_version": null,
                    "definition": {"kind": "identifier", "definition": {
                        "mapping_id": "isin-old-duplicate",
                        "identifier": {"kind": "isin", "value": "OLD-ISIN"},
                        "target": {"kind": "instrument", "instrument_id": INSTRUMENT},
                        "effective": {"from": "2000-01-01T00:00:00Z", "until": null},
                        "provenance": {"source": "fictional-fixture", "evidence_reference": "test"}
                    }}
                }]
            })),
        )
        .await;
    assert!(status.is_success(), "{body}");
    let by_isin = json!([{"identifier": {"kind": "isin", "value": "OLD-ISIN"}, "quantity": "1"}]);
    let (status, _, body) = server.put("B5", "amb-1", None, by_isin).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["rows"][0]["code"], "AMBIGUOUS_IDENTIFIER");
    let (status, _, _) = server.get("/api/v1/books/B5").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    server.stop().await;
}

#[tokio::test]
async fn position_bound_is_typed_and_ids_are_validated() {
    let (_directory, _path, server) = seeded().await;
    let rows: Vec<Value> = (0..1001)
        .map(|i| json!({"instrument_id": format!("X-{i}"), "quantity": "1"}))
        .collect();
    let (status, _, body) = server.put("BIG", "big-1", None, json!(rows)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["code"], "BOOK_TOO_LARGE");
    let (status, _, _) = server.put("bad id", "id-1", None, json!([])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Unknown fields are refused rather than silently dropped.
    let (status, _, _) = server
        .send(
            Method::PUT,
            "/api/v1/books/B6",
            Some("unknown-1"),
            None,
            Some(json!({"positions": [], "extra": 1})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    server.stop().await;
}

#[tokio::test]
async fn reads_are_paged_versioned_and_durable_across_restart() {
    let (_directory, path, server) = seeded().await;
    for (n, id) in ["P-A", "P-B", "P-C"].into_iter().enumerate() {
        let (status, _, _) = server
            .put(id, &format!("page-{n}"), None, json!([row("1")]))
            .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    server
        .put("P-A", "page-a2", Some("\"1\""), json!([row("2")]))
        .await;

    let (_, _, first) = server.get("/api/v1/books?limit=2").await;
    assert_eq!(first["books"].as_array().unwrap().len(), 2);
    assert_eq!(first["next_after"], "P-B");
    let (_, _, second) = server.get("/api/v1/books?limit=2&after=P-B").await;
    assert_eq!(second["books"].as_array().unwrap().len(), 1);
    assert!(second["next_after"].is_null());
    let (status, _, _) = server.get("/api/v1/books?limit=501").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (_, etag, v1) = server.get("/api/v1/books/P-A/versions/1").await;
    assert_eq!(
        (etag.as_deref(), v1["positions"][0]["quantity"].as_str()),
        (Some("\"1\""), Some("1"))
    );
    let (status, _, _) = server.get("/api/v1/books/P-A/versions/3").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, _, history) = server.get("/api/v1/books/P-A/history?limit=1").await;
    assert_eq!(history["versions"].as_array().unwrap().len(), 1);
    assert_eq!(
        (
            history["current_version"].as_u64(),
            history["next_from"].as_u64()
        ),
        (Some(2), Some(2))
    );
    let (_, _, history) = server.get("/api/v1/books/P-A/history?from=2").await;
    assert_eq!(history["versions"][0]["version"], 2);
    assert!(history["next_from"].is_null());
    let (status, _, _) = server.get("/api/v1/books/MISSING/history").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, before, _) = server.get("/api/v1/books/P-A").await;
    server.stop().await;
    let server = Running::start(&path).await;
    let (_, after, current) = server.get("/api/v1/books/P-A").await;
    assert_eq!(before, after);
    assert_eq!(current["positions"][0]["quantity"], "2");
    server.stop().await;
}

#[tokio::test]
async fn portfolio_alias_is_backed_by_books_and_edits_are_read_modify_write() {
    let (_directory, _path, server) = seeded().await;
    let (status, etag, body) = server
        .send(
            Method::POST,
            "/api/v1/portfolios",
            Some("alias-1"),
            None,
            Some(
                json!({"portfolio_id": "AL-1", "name": "Alias", "currency": "USD",
                        "positions": [{"instrument_id": "ANY-1", "notional": "100"}]}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(etag.as_deref(), Some("\"1\""));
    assert!(body["created_at"].is_number() && body["updated_at"].is_number());

    // Position edits use the current version as the expected version.
    let (status, etag, body) = server
        .send(
            Method::POST,
            "/api/v1/portfolios/AL-1/positions",
            Some("alias-2"),
            None,
            Some(json!({"instrument_id": "ANY-2", "notional": "5"})),
        )
        .await;
    assert_eq!(
        (status, etag.as_deref()),
        (StatusCode::OK, Some("\"2\"")),
        "{body}"
    );
    assert_eq!(body["positions"].as_array().unwrap().len(), 2);
    // A caller racing on the old version gets 409.
    let (status, _, _) = server
        .send(
            Method::DELETE,
            "/api/v1/portfolios/AL-1/positions/ANY-1",
            Some("alias-3"),
            Some("\"1\""),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // The same data is a book, with its history.
    let (_, _, book) = server.get("/api/v1/books/AL-1").await;
    assert_eq!(book["owning_system"], "portfolio-api");
    assert_eq!(book["positions"][1]["instrument_id"], "ANY-2");

    // Retired books disappear from the alias, and the id can be created again.
    let (status, _, _) = server
        .send(
            Method::DELETE,
            "/api/v1/portfolios/AL-1",
            Some("alias-4"),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = server.get("/api/v1/portfolios/AL-1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, _, list) = server.get("/api/v1/portfolios").await;
    assert_eq!(list["total"], 0);
    let (status, etag, _) = server
        .send(
            Method::POST,
            "/api/v1/portfolios",
            Some("alias-5"),
            None,
            Some(json!({"portfolio_id": "AL-1", "name": "Again"})),
        )
        .await;
    assert_eq!(
        (status, etag.as_deref()),
        (StatusCode::CREATED, Some("\"4\""))
    );
    server.stop().await;
}

#[tokio::test]
async fn analytics_by_book_refuses_the_whole_request_without_runtime_results() {
    let (_directory, _path, server) = seeded().await;
    server.put("AN-1", "an-1", None, json!([row("1000")])).await;
    let (status, _, body) = server
        .send(
            Method::POST,
            "/api/v1/portfolio/analytics",
            None,
            None,
            Some(json!({"book_id": "AN-1"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "BOOK_RESULTS_UNAVAILABLE");
    assert_eq!(body["detail"]["book_version"], 1);
    assert_eq!(body["detail"]["missing_instruments"][0], INSTRUMENT);

    for (request, expected) in [
        (json!({"book_id": "NOPE"}), StatusCode::NOT_FOUND),
        (
            json!({"book_id": "AN-1", "version": 9}),
            StatusCode::NOT_FOUND,
        ),
        (
            json!({"book_id": "AN-1", "bond_prices": []}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (json!({}), StatusCode::UNPROCESSABLE_ENTITY),
    ] {
        let (status, _, body) = server
            .send(
                Method::POST,
                "/api/v1/portfolio/analytics",
                None,
                None,
                Some(request.clone()),
            )
            .await;
        assert_eq!(status, expected, "{request} -> {body}");
    }
    server.stop().await;
}
