//! The `live` profile over the real composition and HTTP: canonical quotes posted to
//! `/api/v1/market/quotes` price through the Live runtime, and a restart waits for
//! fresh images instead of restoring stored ticks.
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use convex_core::contracts::{ModelId, UtcTimestamp};
use convex_engine::ports::quote_event::QuoteEvent;
use convex_engine::RuntimeMode;
use convex_server::access::identity::{AccessMode, AccessSettings};
use convex_server::{compose, ServerConfig, ServerProfile, StartupError};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use support::*;

fn config(profile: ServerProfile, storage: &Path, plan: Option<&Path>) -> ServerConfig {
    ServerConfig {
        profile,
        access: AccessSettings {
            mode: AccessMode::LocalDevelopment,
            ..Default::default()
        },
        host: "127.0.0.1".into(),
        port: 0,
        storage_path: storage.to_string_lossy().into_owned(),
        runtime_plan: plan.map(|p| p.to_string_lossy().into_owned()),
        ..Default::default()
    }
}

struct Running {
    base: String,
    client: Client,
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), StartupError>>,
}

impl Running {
    async fn start(config: ServerConfig) -> Self {
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

    async fn runtime(&self) -> Value {
        self.client
            .get(format!("{}/api/v1/runtime", self.base))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn post_quotes(&self, events: Vec<QuoteEvent>) -> reqwest::Response {
        self.client
            .post(format!("{}/api/v1/market/quotes", self.base))
            .json(&json!({ "events": events }))
            .send()
            .await
            .unwrap()
    }

    async fn wait_ready(&self) -> Value {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let runtime = self.runtime().await;
                if runtime["ready"] == true {
                    return runtime;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap()
    }
}

/// An image for the plan's quote key; the server replaces the supplied receipt time.
fn image(sequence: u64, price: &str) -> QuoteEvent {
    fixture::event(sequence, chrono::Utc::now().timestamp_millis(), 0, price)
}

#[tokio::test]
async fn posted_quotes_price_and_a_restart_waits_for_fresh_images() {
    let s = setup().await;
    let now = UtcTimestamp::from_datetime(chrono::Utc::now()).unwrap();
    let mut plan = s.plan.clone();
    plan.mode = RuntimeMode::Live;
    plan.seed_events.clear();
    plan.build_id = ModelId::new(convex_server::SERVER_BUILD_ID).unwrap();
    plan.assembly.as_of = now;
    plan.assembly.known_at = now;
    plan.assembly.curves[0].source_time = now;
    plan.assembly.curves[0].recorded_at = now;
    plan.assembly.curves[0].max_age_ms = 600_000;
    plan.assembly.quotes[0].max_age_ms = 600_000;
    let plan_path = s.dir.path().join("runtime.json");
    std::fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
    let database = s.dir.path().join("inputs.redb");
    // Release every database handle before the composition reopens the same file.
    let dir = s.dir;
    drop(s.storage);
    drop(s.store);
    drop(s.snapshots);
    drop(s.results);
    let live = config(ServerProfile::Live, &database, Some(&plan_path));

    let server = Running::start(live.clone()).await;
    let runtime = server.runtime().await;
    assert_eq!(runtime["mode"], "live", "{runtime}");
    assert_eq!(runtime["ready"], false);

    // A body that is not a quote batch is 400; 422 is reserved for row refusals.
    for body in [
        json!({"events": [{"key": 1}]}),
        json!({"events": [], "extra": 1}),
    ] {
        let malformed = server
            .client
            .post(format!("{}/api/v1/market/quotes", server.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(malformed.status(), StatusCode::BAD_REQUEST, "{body}");
    }
    let untyped = server
        .client
        .post(format!("{}/api/v1/market/quotes", server.base))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(untyped.status(), StatusCode::BAD_REQUEST);

    let mut other = image(1, "99.25");
    other.key.source = "feed-b".into();
    let rejected = server.post_quotes(vec![image(1, "99.25"), other]).await;
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = rejected.json().await.unwrap();
    assert_eq!(body["rows"][0]["index"], 1);
    assert_eq!(body["rows"][0]["code"], "UNKNOWN_KEY");
    assert_eq!(server.runtime().await["ready"], false);

    let accepted = server.post_quotes(vec![image(1, "99.25")]).await;
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let receipt: Value = accepted.json().await.unwrap();
    assert_eq!(receipt["accepted"], 1);
    let first = server.wait_ready().await;
    let ready = server
        .client
        .get(format!("{}/ready", server.base))
        .send()
        .await
        .unwrap();
    assert_eq!(ready.status(), StatusCode::OK);
    let results: Vec<Value> = server
        .client
        .get(format!("{}/api/v1/runtime/results", server.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0]["live_valid_until"].is_string(), "{}", results[0]);
    server.stop().await;

    let server = Running::start(live).await;
    let restored = server.runtime().await;
    assert_eq!(restored["ready"], false, "{restored}");
    assert!(restored["revision"].as_u64() > first["revision"].as_u64());
    let accepted = server.post_quotes(vec![image(1, "99.50")]).await;
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let fresh = server.wait_ready().await;
    assert!(fresh["revision"].as_u64() > restored["revision"].as_u64());
    server.stop().await;
    // Live stores no market snapshot and archives no result.
    let stored = convex_ext_redb::open_redb_storage(&database).unwrap();
    assert_eq!(stored.snapshots.market_snapshot_count().unwrap(), 0);
    assert!(stored.results.is_empty().unwrap());
    drop(dir);
}

#[tokio::test]
async fn a_server_without_a_live_runtime_refuses_quotes() {
    let directory = tempfile::tempdir().unwrap();
    let server = Running::start(config(
        ServerProfile::Demo,
        &directory.path().join("state.redb"),
        None,
    ))
    .await;
    let response = server.post_quotes(vec![image(1, "99.25")]).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    server.stop().await;
}
