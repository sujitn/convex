#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use convex_core::contracts::*;
use convex_engine::ports::{
    runtime::{PublishedValuation, RuntimeStore},
    snapshot::SnapshotRepository,
};
use convex_engine::{RuntimeInput, RuntimeMode, RuntimePhase};
use convex_server::{Server, ServerConfig};
use http_body_util::BodyExt;
use std::time::Duration;
use support::*;
use tower::ServiceExt;

#[tokio::test]
async fn liveness_is_separate_from_readiness_and_rest_matches_the_actual_runtime_context() {
    let s = setup().await;
    let engine = make_engine(
        &s,
        archive(&s),
        s.plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    let router = convex_server::routes::create_router_with_access(
        engine.clone(),
        true,
        convex_server::access::AccessControl::local_development(),
    );
    for (path, status) in [
        ("/live", StatusCode::OK),
        ("/ready", StatusCode::SERVICE_UNAVAILABLE),
    ] {
        assert_eq!(
            router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            status
        );
    }
    engine.start().await.unwrap();
    assert_eq!(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let record = engine.runtime_results()[0].clone();
    let snapshot = s
        .snapshots
        .get(&record.result.provenance().snapshot_id)
        .await
        .unwrap()
        .unwrap();
    let context = s
        .plan
        .context(&snapshot, &InstrumentId::new("BOND-001").unwrap())
        .unwrap();
    let body = serde_json::to_vec(&serde_json::json!({"snapshot": snapshot, "context":context, "request_id":record.result.request_id(), "calculated_at":record.result.provenance().calculated_at})).unwrap();
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
    assert_eq!(response.status(), StatusCode::OK);
    let result: convex_engine::snapshot::SnapshotValuationResult =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(result.envelope(), &record.result);
    assert_eq!(result.cache_key(), record.context_key);
    let mut updates = engine.subscribe_results();
    engine
        .submit_update(event(2, 10_001, "98.25"), time(10_001), time(10_001))
        .await
        .unwrap();
    let failed = updates.recv().await.unwrap();
    assert_eq!(failed.result.status(), ResultStatus::Failed);
    assert_eq!(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/runtime/results")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let states: Vec<PublishedValuation> =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(states, vec![failed]);
    engine.shutdown().await.unwrap();
}
#[tokio::test]
async fn actual_websocket_opt_in_publishes_lineage_and_sessions_close_during_joined_shutdown() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let s = setup().await;
    let engine = make_engine(
        &s,
        archive(&s),
        s.plan.clone(),
        s.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = Server::new(
        ServerConfig {
            profile: convex_server::ServerProfile::Demo,
            access: convex_server::access::identity::AccessSettings {
                mode: convex_server::access::identity::AccessMode::LocalDevelopment,
                ..Default::default()
            },
            host: "127.0.0.1".into(),
            port: address.port(),
            ..Default::default()
        },
        engine.clone(),
    );
    let (shutdown, rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        server
            .serve_until(listener, async {
                let _ = rx.await;
            })
            .await
    });
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/ws"))
        .await
        .unwrap();
    assert!(socket.next().await.unwrap().is_ok());
    socket
        .send(Message::Text(
            r#"{"type":"subscribe_valuations","instrument_ids":["BOND-001"]}"#.into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            if let Message::Text(text) = message {
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                if value["type"] == "subscribed" {
                    assert_eq!(value["subscription_type"], "valuations");
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
    engine
        .submit_update(event(2, 1, "98.25"), time(1), time(1))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            if let Message::Text(text) = message {
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                if value["type"] == "valuation" {
                    assert_eq!(value["generation"], 2);
                    assert_eq!(value["result"]["status"], "Complete");
                    assert!(value["result"]["snapshot_id"]
                        .as_str()
                        .unwrap()
                        .starts_with("sha256:"));
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
    shutdown.send(()).unwrap();
    // Closing the client is not needed for the server to finish: joined engine shutdown closes sessions.
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(engine.runtime_health().phase, RuntimePhase::Stopped);
}

#[cfg(unix)]
#[tokio::test]
async fn process_replay_sigterm_and_redb_reopen_preserve_exact_published_state() {
    use std::process::{Command, Stdio};
    use tokio::io::{AsyncBufReadExt, BufReader};
    let s = setup().await;
    let mut plan = s.plan.clone();
    plan.mode = RuntimeMode::Replay;
    plan.seed_events.clear();
    plan.build_id = ModelId::new(convex_server::SERVER_BUILD_ID).unwrap();
    let plan_path = s.dir.path().join("runtime.json");
    let quotes_path = s.dir.path().join("quotes.jsonl");
    let database_path = s.dir.path().join("inputs.redb");
    std::fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
    std::fs::write(
        &quotes_path,
        [event(1, 0, "99.25"), event(2, 1, "98.25")]
            .iter()
            .map(|e| serde_json::to_string(e).unwrap() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    // Release every DB handle before another process opens the same local file.
    let dir = s.dir;
    drop(s.storage);
    drop(s.store);
    drop(s.snapshots);
    drop(s.results);
    let mut previous = None;
    for run in 0..2 {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_convex-server"));
        for (name, _) in
            std::env::vars_os().filter(|(name, _)| name.to_string_lossy().starts_with("CONVEX_"))
        {
            command.env_remove(name);
        }
        command
            .current_dir(dir.path())
            .env("CONVEX_PROFILE", "replay")
            .env("CONVEX_ACCESS_MODE", "local_development")
            .env("CONVEX_HOST", "127.0.0.1")
            .env("CONVEX_PORT", "0")
            .env("CONVEX_STORAGE_PATH", &database_path)
            .env("CONVEX_RUNTIME_PLAN", &plan_path)
            .env("CONVEX_QUOTES_FILE", &quotes_path)
            .env("RUST_LOG", "info")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let address = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let line = lines
                    .next_line()
                    .await
                    .unwrap()
                    .expect("server exited before bind");
                if let Some((_, address)) = line.split_once("Starting Convex server on ") {
                    break address.trim().to_string();
                }
            }
        })
        .await
        .unwrap();
        let client = reqwest::Client::new();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let value: serde_json::Value = client
                    .get(format!("http://{address}/api/v1/runtime"))
                    .send()
                    .await
                    .unwrap()
                    .json()
                    .await
                    .unwrap();
                if value["replay_complete"] == true {
                    assert_eq!(value["ready"], true);
                    assert_eq!(value["mode"], "replay");
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let values: Vec<PublishedValuation> = client
            .get(format!("http://{address}/api/v1/runtime/results"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if run == 0 {
            previous = Some(values.clone());
        } else {
            assert_eq!(previous.as_ref().unwrap(), &values);
        }
        assert!(Command::new("kill")
            .args(["-TERM", &child.id().unwrap().to_string()])
            .status()
            .unwrap()
            .success());
        assert!(tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success());
        let (storage, checkpoint) =
            convex_ext_redb::create_redb_storage_with_runtime(&database_path).unwrap();
        assert!(
            !checkpoint
                .load(&plan.runtime_id)
                .await
                .unwrap()
                .unwrap()
                .pending
        );
        assert!(checkpoint
            .load(&plan.runtime_id)
            .await
            .unwrap()
            .unwrap()
            .outbox
            .is_empty());
        drop(storage);
        drop(checkpoint);
    }
}
