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

    /// A session, under `token` when one is given, past its `connected` frame.
    pub async fn session(&self, token: Option<&str>) -> Socket {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut request = self.ws_url().into_client_request().unwrap();
        if let Some(token) = token {
            request
                .headers_mut()
                .insert("authorization", format!("Bearer {token}").parse().unwrap());
        }
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        assert_eq!(next_frame(&mut socket).await["type"], "connected");
        socket
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

    /// `method` on `path` under the idempotency `key`, over `version` in `If-Match` when
    /// given, with a JSON `body`: its status and JSON reply (`null` if none).
    pub async fn send_with(
        &self,
        method: reqwest::Method,
        path: &str,
        key: &str,
        version: Option<u64>,
        body: Option<&Value>,
    ) -> (reqwest::StatusCode, Value) {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .header("idempotency-key", key);
        if let Some(version) = version {
            request = request.header("if-match", format!("\"{version}\""));
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    /// `PUT` `body` on `path` with an idempotency key of its path, which must succeed.
    pub async fn put(&self, path: &str, body: &Value) {
        let response = self
            .client
            .put(format!("{}{path}", self.base))
            .header("idempotency-key", format!("put{}", path.replace('/', "-")))
            .json(body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        assert!(
            status.is_success(),
            "{path}: {status} {}",
            response.text().await.unwrap_or_default()
        );
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

/// One image for each of `keys`, at `price`.
pub fn images(keys: &[QuoteKey], sequence: u64, price: &str) -> Vec<QuoteEvent> {
    keys.iter()
        .map(|key| {
            let mut event = image(sequence, price);
            event.key = key.clone();
            event
        })
        .collect()
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
    wide_live_setup_measuring(width, &[]).await
}

/// [`wide_live_setup`] with the runtime computing `measures`, or the plan's own when none
/// is named.
pub async fn wide_live_setup_measuring(
    width: usize,
    measures: &[&str],
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    ServerConfig,
    Vec<QuoteKey>,
) {
    let s = setup().await;
    let mut plan = live_plan(&s);
    if !measures.is_empty() {
        plan.measures = measures
            .iter()
            .map(|measure| convex_core::contracts::MeasureId::new(*measure).unwrap())
            .collect();
    }
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

/// Stage and activate, in the database at `database` before the server opens it, the
/// document of the Live plan beside it, over the stored pricing configuration, with
/// `tiers` as its client tiers: request-time pricing and requests for quote resolve it,
/// and the runtime starts under it.
pub async fn activate_client_tiers(database: &Path, tiers: Value) {
    use convex_engine::ports::configuration::*;
    let plan: RuntimePlan =
        serde_json::from_slice(&std::fs::read(database.with_file_name("runtime.json")).unwrap())
            .unwrap();
    let storage = convex_ext_redb::open_redb_storage(database)
        .unwrap()
        .storage;
    let config = storage
        .configs
        .get_version("CONFIG-RESTORE", 1)
        .await
        .unwrap()
        .unwrap();
    let mut staged = configuration_fixture::staged(1);
    let document = &mut staged.document;
    document.pricing = config;
    document.process.result_transport =
        convex_core::contracts::SourceId::new("display-only").unwrap();
    document.providers = Vec::new();
    for quote in &plan.assembly.quotes {
        let source = convex_core::contracts::SourceId::new(&quote.key.source).unwrap();
        if document.providers.iter().all(|p| p.source != source) {
            document.providers.push(ProviderBinding {
                source,
                capabilities: vec![
                    ProviderCapability::CanonicalQuotes,
                    ProviderCapability::LiveImages,
                ],
                credential: None,
            });
        }
    }
    document.runtime = RuntimeSettings {
        runtime_id: plan.runtime_id,
        mode: plan.mode,
        settlement_date: plan.settlement_date,
        build_id: plan.build_id,
        measures: plan.measures,
        numerical_settings: plan.numerical_settings,
    };
    document.assembly = plan.assembly;
    document.client_tiers = serde_json::from_value(tiers).unwrap();
    storage.configuration.stage(&staged).await.unwrap();
    storage
        .configuration
        .activate(&configuration_fixture::activate(1, 0, "client-tiers"))
        .await
        .unwrap();
}

pub type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The next frame other than a heartbeat.
pub async fn next_frame(socket: &mut Socket) -> Value {
    use futures_util::StreamExt;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match socket.next().await {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    if value["type"] != "heartbeat" {
                        return value;
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended: {other:?}"),
            }
        }
    })
    .await
    .expect("no frame")
}

pub async fn send_frame(socket: &mut Socket, message: Value) {
    use futures_util::SinkExt;
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            message.to_string().into(),
        ))
        .await
        .unwrap();
}

/// Follow the requests for quote on `books`: every frame sent before the acknowledgement,
/// which must count the books, or the refusal as the last.
pub async fn subscribe_rfqs(socket: &mut Socket, books: &[&str]) -> Vec<Value> {
    subscribe(socket, "rfqs", books, |_| false).await
}

/// Follow the summaries of `books`: every frame sent before the acknowledgement, which must
/// count the books, or the refusal as the last. A summary withheld from the session is sent
/// in its place as an error, which does not refuse the subscription.
pub async fn subscribe_books(socket: &mut Socket, books: &[&str]) -> Vec<Value> {
    subscribe(socket, "books", books, |frame| {
        frame["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("the summary of book"))
    })
    .await
}

/// Follow `books` on `topic`: every frame sent before the acknowledgement, which must count
/// the books, or the refusal as the last. An error `is_withheld` holds stands in for one
/// item and does not refuse the subscription.
async fn subscribe(
    socket: &mut Socket,
    topic: &str,
    books: &[&str],
    is_withheld: impl Fn(&Value) -> bool,
) -> Vec<Value> {
    send_frame(
        socket,
        json!({"type": format!("subscribe_{topic}"), "book_ids": books}),
    )
    .await;
    let mut frames = Vec::new();
    loop {
        let frame = next_frame(socket).await;
        match frame["type"].as_str() {
            Some("subscribed") => {
                assert_eq!(frame["subscription_type"], topic, "{frame}");
                assert_eq!(frame["count"], books.len(), "{frame}");
                return frames;
            }
            Some("error") if !is_withheld(&frame) => {
                frames.push(frame);
                return frames;
            }
            _ => frames.push(frame),
        }
    }
}
