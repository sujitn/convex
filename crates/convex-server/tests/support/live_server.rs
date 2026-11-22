//! A `live` profile server over the real composition on a loopback listener, fed by
//! `POST /api/v1/market/quotes`. `live_ingress.rs` and the `live_hop` bench share it;
//! each declares the engine's `runtime_setup.rs` as its crate's `support` module.
#![allow(dead_code)] // The test and the bench use different parts.

use crate::support::*;
use convex_core::contracts::{ModelId, UtcTimestamp};
use convex_engine::ports::quote_event::{QuoteEvent, QuoteKey};
use convex_engine::{RuntimeMode, RuntimePlan};
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
        // No clear hold: every change is recorded at once; a test of the hold sets its own.
        limits: convex_server::LimitSettings {
            clear_hold_seconds: 0,
            ..Default::default()
        },
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

    /// Post `events` 256 a request, as many requests as they take. A request the full
    /// ingress queue refuses (429) is sent again once the runtime has taken some.
    pub async fn post_all_quotes(&self, events: &[QuoteEvent]) {
        for batch in events.chunks(256) {
            tokio::time::timeout(Duration::from_secs(30), async {
                loop {
                    let posted = self.post_quotes(batch.to_vec()).await;
                    match posted.status() {
                        status if status.is_success() => return,
                        reqwest::StatusCode::TOO_MANY_REQUESTS => {
                            tokio::time::sleep(Duration::from_millis(20)).await
                        }
                        status => panic!("quotes refused: {status}"),
                    }
                }
            })
            .await
            .expect("the ingress takes every batch");
        }
    }

    /// `method` on `path` with a JSON `body`, its status and JSON reply (`null` if none).
    pub async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> (reqwest::StatusCode, Value) {
        let mut request = self.client.request(method, format!("{}{path}", self.base));
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        (status, response.json().await.unwrap_or(Value::Null))
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
    let mut plan = live_plan(&s);
    widen(&s, &mut plan, width).await;
    let keys = plan.assembly.quotes.iter().map(|q| q.key.clone()).collect();
    let (dir, database, live) = live_config(s, &plan);
    (dir, database, live, keys)
}

/// The runtime fixture's plan in Live mode at the current time, under the server's build.
fn live_plan(s: &Setup) -> RuntimePlan {
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
    plan
}

/// Write `plan` beside the setup's database and return a Live server configuration over
/// both. Every database handle is released first, so the composition can reopen the file.
fn live_config(
    s: Setup,
    plan: &RuntimePlan,
) -> (tempfile::TempDir, std::path::PathBuf, ServerConfig) {
    let plan_path = s.dir.path().join("runtime.json");
    std::fs::write(&plan_path, serde_json::to_vec(plan).unwrap()).unwrap();
    let database = s.dir.path().join("inputs.redb");
    let Setup { dir, .. } = s;
    let live = config(ServerProfile::Live, &database, Some(&plan_path));
    (dir, database, live)
}

/// The Treasuries of the set the bonds of [`level_live_setup`] are quoted to: tenor,
/// instrument, maturity, coupon and CUSIP.
const TREASURIES: [(&str, &str, &str, &str, &str); 7] = [
    ("2Y", "UST-2Y", "2028-01-15", "0.035", "91282CAA9"),
    ("3Y", "UST-3Y", "2029-07-15", "0.03625", "91282CAB7"),
    ("5Y", "UST-5Y", "2031-07-15", "0.0375", "91282CAC5"),
    ("7Y", "UST-7Y", "2033-07-15", "0.04", "91282CAD3"),
    ("10Y", "UST-10Y", "2036-07-15", "0.04125", "91282CBB6"),
    ("20Y", "UST-20Y", "2046-07-15", "0.045", "912810TA6"),
    ("30Y", "UST-30Y", "2056-07-15", "0.04625", "912810TB4"),
];

/// The Treasury the bonds of [`level_live_setup`] are quoted over.
pub const BENCHMARK: &str = "UST-10Y";

/// The quote keys of a [`level_live_setup`] server.
pub struct LevelKeys {
    /// The bonds quoted over the benchmark, each by a spread.
    pub bonds: Vec<QuoteKey>,
    /// Every Treasury of the set, each by a price; the benchmark is one of them.
    pub treasuries: Vec<QuoteKey>,
}

/// A Live server configuration over `dependents` bonds, each quoted 125 bp over the
/// 10-year of the `UST-OTR` set of seven Treasuries, which the runtime values beside them.
pub async fn level_live_setup(
    dependents: usize,
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    ServerConfig,
    LevelKeys,
) {
    level_live_setup_from(dependents, None).await
}

/// [`level_live_setup`] with the bonds' spreads read from `level_source` when named, and
/// the Treasuries' prices from the feed's source.
pub async fn level_live_setup_from(
    dependents: usize,
    level_source: Option<&str>,
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    ServerConfig,
    LevelKeys,
) {
    use convex_core::contracts::{InstrumentId, RequestId};
    use convex_engine::ports::{
        configuration::InstrumentPin,
        quote_event::QuoteField,
        reference_data::IssuerType,
        reference_master::MasterChange,
        reference_repository::{ReferenceChange, ReferenceCommand, MAX_REFERENCE_CHANGES},
    };
    let s = setup().await;
    let mut plan = live_plan(&s);
    plan.measures
        .push(convex_core::contracts::MeasureId::new("g_spread_mid").unwrap());
    widen(&s, &mut plan, dependents).await;
    let feed_source = plan.assembly.quotes[0].key.source.clone();
    for quote in &mut plan.assembly.quotes {
        quote.fields = vec![QuoteField::MidSpread];
        if let Some(source) = level_source {
            quote.key.source = source.into();
        }
    }
    let bonds: Vec<QuoteKey> = plan.assembly.quotes.iter().map(|q| q.key.clone()).collect();

    let notes: Vec<_> = TREASURIES
        .iter()
        .map(|(_, id, maturity, coupon, cusip)| {
            let mut note = fixture::bond();
            note.instrument_id = convex_core::ids::InstrumentId::new(*id);
            note.issuer_type = IssuerType::Sovereign;
            note.maturity_date = convex_core::Date::parse(maturity).unwrap();
            note.coupon_rate = Some(coupon.parse().unwrap());
            note.cusip = Some((*cusip).into());
            note.isin = None;
            note
        })
        .collect();
    let provenance = json!({"source": "desk", "evidence_reference": "level-live-setup"});
    let effective = json!({"from": "2025-01-01T00:00:00Z", "until": null});
    let master = |kind: &str, definition: Value| -> MasterChange {
        serde_json::from_value(json!({
            "expected_version": null,
            "definition": {"kind": kind, "definition": definition},
        }))
        .unwrap()
    };
    let securities: Vec<Value> = TREASURIES
        .iter()
        .map(|(tenor, id, ..)| json!({"tenor": tenor, "instrument_id": id, "version": 1}))
        .collect();
    let set = master(
        "benchmark_set",
        json!({
            "benchmark_set_id": "UST-OTR",
            "currency": "USD",
            "securities": securities,
            "effective": effective,
            "provenance": provenance,
        }),
    );
    let at = fixture::time("2025-06-01T00:00:00Z");
    s.storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("level-treasuries").unwrap(),
                changes: notes
                    .iter()
                    .map(|note| ReferenceChange::Create {
                        bond: Box::new(note.clone()),
                    })
                    .collect(),
                master_changes: vec![set],
            },
            at,
        )
        .await
        .unwrap();
    let rules: Vec<MasterChange> = bonds
        .iter()
        .map(|key| {
            master(
                "pricing_rule",
                json!({
                    "instrument_id": key.instrument_id.as_str(),
                    "method": {"kind": "benchmark_spread", "tenor": "10Y"},
                    "benchmark_set_id": "UST-OTR",
                    "effective": effective,
                    "provenance": provenance,
                }),
            )
        })
        .collect();
    for (chunk, rules) in rules.chunks(MAX_REFERENCE_CHANGES).enumerate() {
        s.storage
            .bonds
            .commit(
                &ReferenceCommand {
                    idempotency_key: RequestId::new(format!("level-rules-{chunk}")).unwrap(),
                    changes: vec![],
                    master_changes: rules.to_vec(),
                },
                at,
            )
            .await
            .unwrap();
    }
    let mut treasuries = Vec::with_capacity(notes.len());
    for note in &notes {
        plan.assembly.instruments.push(InstrumentPin {
            instrument_id: InstrumentId::new(note.instrument_id.as_str()).unwrap(),
            version: fixture::version(1),
        });
        let mut quote = plan.assembly.quotes[0].clone();
        quote.key.instrument_id = note.instrument_id.clone();
        quote.key.source = feed_source.clone();
        quote.fields = vec![QuoteField::MidPrice];
        treasuries.push(quote.key.clone());
        plan.assembly.quotes.push(quote);
    }
    let (dir, database, live) = live_config(s, &plan);
    (dir, database, live, LevelKeys { bonds, treasuries })
}
