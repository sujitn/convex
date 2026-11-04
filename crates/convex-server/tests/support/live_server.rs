//! A `live` profile server over the real composition on a loopback listener, fed by
//! `POST /api/v1/market/quotes`. `live_ingress.rs` and the `live_hop` bench share it;
//! each declares the engine's `runtime_setup.rs` as its crate's `support` module.
#![allow(dead_code)] // The test and the bench use different parts.

use crate::support::*;
use convex_core::contracts::{ModelId, UtcTimestamp};
use convex_engine::ports::quote_event::{QuoteEvent, QuoteKey};
use convex_engine::RuntimeMode;
use convex_server::access::identity::{AccessMode, AccessSettings};
use convex_server::{compose, ServerConfig, ServerProfile, StartupError};
use reqwest::Client;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

pub fn config(profile: ServerProfile, storage: &Path, plan: Option<&Path>) -> ServerConfig {
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

pub struct Running {
    pub base: String,
    pub client: Client,
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), StartupError>>,
}

impl Running {
    pub async fn start(config: ServerConfig) -> Self {
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

    pub async fn stop(self) {
        let _ = self.shutdown.send(());
        self.task.await.unwrap().unwrap();
    }

    /// The WebSocket endpoint of this server.
    pub fn ws_url(&self) -> String {
        format!("{}/ws", self.base.replacen("http://", "ws://", 1))
    }

    pub async fn runtime(&self) -> Value {
        self.client
            .get(format!("{}/api/v1/runtime", self.base))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    pub async fn post_quotes(&self, events: Vec<QuoteEvent>) -> reqwest::Response {
        self.client
            .post(format!("{}/api/v1/market/quotes", self.base))
            .json(&json!({ "events": events }))
            .send()
            .await
            .unwrap()
    }

    pub async fn wait_ready(&self) -> Value {
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
pub fn image(sequence: u64, price: &str) -> QuoteEvent {
    fixture::event(sequence, chrono::Utc::now().timestamp_millis(), 0, price)
}

/// A Live server configuration over the runtime fixture: one bond priced from posted
/// images. The directory holds the database and must outlive the server.
pub async fn live_setup() -> (tempfile::TempDir, std::path::PathBuf, ServerConfig) {
    let (dir, database, live, _) = wide_live_setup(1).await;
    (dir, database, live)
}

/// `live_setup` widened to `width` bonds, with every quoted key.
pub async fn wide_live_setup(
    width: usize,
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    ServerConfig,
    Vec<QuoteKey>,
) {
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
    widen(&s, &mut plan, width).await;
    let keys = plan.assembly.quotes.iter().map(|q| q.key.clone()).collect();
    let plan_path = s.dir.path().join("runtime.json");
    std::fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
    let database = s.dir.path().join("inputs.redb");
    // Release every database handle before the composition reopens the same file.
    let dir = s.dir;
    drop(s.storage);
    drop(s.store);
    drop(s.snapshots);
    drop(s.results);
    drop(s.fixings);
    let live = config(ServerProfile::Live, &database, Some(&plan_path));
    (dir, database, live, keys)
}
