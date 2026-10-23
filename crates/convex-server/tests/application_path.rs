//! Acceptance test for the one application path: instrument/book create or import →
//! market inputs → pricing → publication → client stream, over the real composition
//! (`convex_server::compose`) and real HTTP.
//!
//! Inputs are the committed showcase seed (`deploy/showcase/seed.json`), so the
//! fixture the Fly container imports is exercised by CI. Steps that need a later
//! phase of `.ai/server-plan/IMPLEMENTATION_PLAN.md` are `#[ignore]`d with that phase.
#[path = "support/access_fixture.rs"]
pub mod fixture;
use convex_server::access::identity::{AccessMode, AccessSettings, TlsBoundary};
use convex_server::access::AccessError;
use convex_server::{compose, ServerConfig, ServerProfile, StartupError};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn seed() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/showcase/seed.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn config(storage: &Path) -> ServerConfig {
    ServerConfig {
        profile: ServerProfile::Demo,
        access: AccessSettings {
            mode: AccessMode::LocalDevelopment,
            ..Default::default()
        },
        host: "127.0.0.1".into(),
        port: 0,
        storage_path: storage.to_string_lossy().into_owned(),
        ..Default::default()
    }
}

/// One composed server bound to an ephemeral loopback port.
struct Running {
    base: String,
    client: Client,
    shutdown: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), StartupError>>,
}

impl Running {
    async fn start(storage: &Path) -> Self {
        Self::start_config(config(storage)).await
    }

    async fn start_config(config: ServerConfig) -> Self {
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
        let client = Client::builder().no_proxy().build().unwrap();
        Self {
            base,
            client,
            shutdown,
            task,
        }
    }

    /// Drain and drop everything that holds the storage file, so it can be reopened.
    async fn stop(self) {
        let _ = self.shutdown.send(());
        self.task.await.unwrap().unwrap();
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

fn storage() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.redb");
    (directory, path)
}

/// Commit every seed reference command exactly as the importer would.
async fn commit_references(server: &Running, seed: &Value) {
    for reference in seed["references"].as_array().unwrap() {
        let command = &reference["command"];
        let key = command["idempotency_key"].as_str().unwrap();
        let response = server
            .client
            .post(server.url("/api/v1/reference/transactions"))
            .header("idempotency-key", key)
            .json(command)
            .send()
            .await
            .unwrap();
        let status = response.status();
        assert!(
            status.is_success(),
            "{key}: {status} {:?}",
            response.text().await
        );
    }
}

/// First fixed bullet the seed creates, with its contractual dates.
fn seeded_bullet(seed: &Value) -> (String, chrono::NaiveDate, chrono::NaiveDate) {
    let bond = seed["references"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|r| {
            r["command"]["changes"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .map(|change| change["bond"].clone())
        .find(|bond| bond["bond_type"] == "FixedBullet")
        .expect("seed creates a fixed bullet");
    let date = |field: &str| {
        chrono::NaiveDate::parse_from_str(bond[field].as_str().unwrap(), "%Y-%m-%d").unwrap()
    };
    (
        bond["instrument_id"].as_str().unwrap().to_owned(),
        date("issue_date"),
        date("maturity_date"),
    )
}

/// Import the seed with the offline importer, exactly as the showcase
/// entrypoint does before the server opens the store. References are recorded
/// at the capture time, so one cutoff sees them and fresh curves together.
fn import_seed(storage: &Path) {
    let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/showcase/seed.json");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_convex-core-inputs"))
        .arg(storage)
        .arg(seed)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

/// The seed curves' recording time: the explicit cutoff at which they are fresh.
fn capture_time(seed: &Value) -> String {
    seed["curves"][0]["recorded_at"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn get(server: &Running, path: &str) -> reqwest::Response {
    server.client.get(server.url(path)).send().await.unwrap()
}

/// Steps 1, 2 and the stored half of 8: import the showcase seed, retry its
/// reference and curve writes over HTTP (idempotent), price the stored bond by
/// identifier with the configured curves at an explicit cutoff, restart, and read
/// the same versions back.
#[tokio::test]
async fn stored_references_and_curve_recipes_price_and_survive_restart() {
    let seed = seed();
    let (_directory, path) = storage();
    let (instrument, _, _) = seeded_bullet(&seed);
    let bond_path = format!("/api/v1/bonds/{instrument}");
    import_seed(&path);

    let server = Running::start(&path).await;
    commit_references(&server, &seed).await;
    let recipes = save_curve_recipes(&server, &seed).await;
    let created = get(&server, &bond_path).await;
    assert_eq!(created.status(), StatusCode::OK);
    assert_eq!(created.headers()["x-convex-data-quality"], "demo");
    let etag = created.headers()["etag"].clone();

    let known_at = capture_time(&seed);
    let quote_path = format!("/api/v1/quotes/{instrument}?market_price=99.5&known_at={known_at}");
    let quote = get(&server, &quote_path).await;
    assert_eq!(quote.status(), StatusCode::OK);
    let pins = quote_pins(&quote);
    assert_eq!(pins[1], "SHOWCASE-USD@1");
    assert_eq!(pins[2], "D.OIS@1,P.SOFR@1");
    assert_eq!(pins[3], known_at);
    let body: Value = quote.json().await.unwrap();
    assert_eq!(body["instrument_id"], instrument.as_str());
    assert!(body["ytm_mid"].is_number() || body["ytm_mid"].is_string());

    // At the current time the dated capture is outside its one-day budget, and
    // the quote is refused rather than priced without its curves.
    let stale = get(
        &server,
        &format!("/api/v1/quotes/{instrument}?market_price=99.5"),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = stale.json().await.unwrap();
    assert_eq!(body["code"], "MISSING_INPUT", "{body}");

    let listing: Value = get(&server, "/api/v1/curves").await.json().await.unwrap();
    assert_eq!(listing["curves"].as_array().unwrap().len(), recipes.len());
    let zero = get(
        &server,
        &format!("/api/v1/curves/D.OIS/zero/5?known_at={known_at}"),
    )
    .await;
    assert_eq!(zero.status(), StatusCode::OK);
    assert_eq!(zero.headers()["x-convex-curve-version"], "1");
    server.stop().await;

    let server = Running::start(&path).await;
    let reopened = get(&server, &bond_path).await;
    assert_eq!(reopened.status(), StatusCode::OK);
    assert_eq!(reopened.headers()["etag"], etag);
    let quote = get(&server, &quote_path).await;
    assert_eq!(quote.status(), StatusCode::OK);
    assert_eq!(quote_pins(&quote), pins);
    commit_references(&server, &seed).await;
    recipes_survived(&server, recipes).await;
    server.stop().await;
}

/// Reference version, pricing configuration, curve versions and cutoff consumed.
fn quote_pins(response: &reqwest::Response) -> [String; 4] {
    [
        "x-convex-reference-version",
        "x-convex-pricing-config",
        "x-convex-curve-versions",
        "x-convex-known-at",
    ]
    .map(|name| response.headers()[name].to_str().unwrap().to_owned())
}

async fn save_curve_recipes(server: &Running, seed: &Value) -> Vec<Value> {
    let recipes = seed["curves"].as_array().unwrap().clone();
    assert!(!recipes.is_empty(), "seed has a curve recipe");
    for recipe in &recipes {
        let response = server
            .client
            .post(server.url("/api/v1/curves"))
            .json(recipe)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
    recipes
}

/// An identical recipe is an idempotent no-op, while different content under the
/// same identity and version conflicts: the stored version survived the restart.
/// `/api/v1/analytics/curves` is the same write as `/api/v1/curves`.
async fn recipes_survived(server: &Running, recipes: Vec<Value>) {
    for recipe in &recipes {
        let same = server
            .client
            .post(server.url("/api/v1/analytics/curves"))
            .json(recipe)
            .send()
            .await
            .unwrap();
        assert_eq!(same.status(), StatusCode::NO_CONTENT);
    }
    let mut changed = recipes[0].clone();
    changed["source"] = Value::from("changed-source");
    let conflict = server
        .client
        .post(server.url("/api/v1/curves"))
        .json(&changed)
        .send()
        .await
        .unwrap();
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
}

/// Step 3 (P6): a book management system ingests a full position snapshot that
/// resolves external identifiers through the reference repository.
#[tokio::test]
async fn book_snapshot_ingest_resolves_external_identifiers() {
    let seed = seed();
    let (_directory, path) = storage();
    let server = Running::start(&path).await;
    commit_references(&server, &seed).await;
    let (instrument, _, _) = seeded_bullet(&seed);
    let response = server
        .client
        .put(server.url("/api/v1/books/SHOWCASE-BOOK"))
        .header("idempotency-key", "showcase-book-1")
        .json(&serde_json::json!({
            "positions": [{"instrument_id": instrument, "quantity": "1000000"}]
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "{}", response.status());
    assert_eq!(response.headers()["etag"], "\"1\"");

    // The seed maps the ISIN to the same instrument: replace version 1 through it.
    let replaced = server
        .client
        .put(server.url("/api/v1/books/SHOWCASE-BOOK"))
        .header("idempotency-key", "showcase-book-2")
        .header("if-match", "\"1\"")
        .json(&serde_json::json!({
            "positions": [{"identifier": {"kind": "isin", "value": "OLD-ISIN"}, "quantity": "2500000.5"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(replaced.status(), StatusCode::OK);
    assert_eq!(replaced.headers()["etag"], "\"2\"");
    let stored: Value = server
        .client
        .get(server.url("/api/v1/books/SHOWCASE-BOOK"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(stored["positions"][0]["instrument_id"], instrument.as_str());
    assert_eq!(
        stored["positions"][0]["source_identifier"]["value"],
        "OLD-ISIN"
    );

    // An unknown identifier rejects the whole snapshot with its own row error.
    let rejected = server
        .client
        .put(server.url("/api/v1/books/SHOWCASE-BOOK"))
        .header("idempotency-key", "showcase-book-3")
        .header("if-match", "\"2\"")
        .json(&serde_json::json!({
            "positions": [
                {"instrument_id": instrument, "quantity": "1"},
                {"identifier": {"kind": "isin", "value": "NO-SUCH-ISIN"}, "quantity": "1"}
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = rejected.json().await.unwrap();
    assert_eq!(body["rows"][0]["index"], 1);
    assert_eq!(body["rows"][0]["code"], "UNKNOWN_IDENTIFIER");
    server.stop().await;
}

/// Steps 4–6 and the live half of 8 (P2b): hold the seeded bond in a book, record
/// fresh curve revisions, then compile, validate, stage and activate a live
/// configuration offline with `convex-config`. The `live` profile plans the active
/// configuration. Posted quotes price, and a restart of the same activation waits for
/// fresh images instead of restoring stored ticks. WebSocket delivery is P3.
#[tokio::test]
async fn compiled_live_configuration_prices_posted_quotes_and_restarts_fresh() {
    let seed = seed();
    let (directory, path) = storage();
    let (instrument, _, _) = seeded_bullet(&seed);
    import_seed(&path);

    let server = Running::start(&path).await;
    let book = server
        .client
        .put(server.url("/api/v1/books/SHOWCASE-BOOK"))
        .header("idempotency-key", "showcase-book-live")
        .json(&json!({"positions": [{"instrument_id": instrument, "quantity": "1000000"}]}))
        .send()
        .await
        .unwrap();
    assert!(book.status().is_success(), "{}", book.status());
    let book: Value = book.json().await.unwrap();
    not_before(book["recorded_at"].as_str().unwrap());
    // The seed's dated Demo curves cannot pass validation; record fresh revisions.
    for id in ["D.OIS", "P.SOFR"] {
        let mut recipe = seed["curves"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["curve_id"] == id)
            .unwrap()
            .clone();
        recipe["version"] = json!(2);
        recipe["recipe"]["key"]["version"] = json!("2");
        recipe["source_time"] = timestamp();
        recipe["recorded_at"] = timestamp();
        recipe["quality"] = json!("Valid");
        recipe["max_age_ms"] = json!(600_000);
        let response = server
            .client
            .post(server.url("/api/v1/curves"))
            .json(&recipe)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT, "{id}");
    }
    server.stop().await;

    let file = |name: &str, value: &Value| {
        let path = directory.path().join(name);
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    };
    let selection = file(
        "selection.json",
        &json!({
            "schema_version": 1,
            "pricing_config": "SHOWCASE-USD",
            "books": ["SHOWCASE-BOOK"],
            "source": "showcase-feed",
            "fields": ["mid_price"],
            "quote_max_age_ms": 600_000,
            "max_source_skew_ms": 600_000,
            "as_of": timestamp(),
            "known_at": timestamp(),
            "mode": "live",
            "settlement_date": chrono::Utc::now().date_naive().to_string(),
            "measures": ["clean_price_mid", "ytm_mid"],
            "numerical_settings": {"policy_id": "convex-bond-router-v1", "bump_bps": 1.0},
            "max_concurrent_calcs": 4
        }),
    );
    let document = config_cli(&path, &["compile".as_ref(), selection.as_os_str()]);
    assert_eq!(document["runtime"]["runtime_id"], "SHOWCASE-USD-v1");
    assert_eq!(
        document["assembly"]["instruments"][0]["instrument_id"],
        instrument.as_str()
    );
    let recipes = document["assembly"]["curve_recipes"].as_array().unwrap();
    assert_eq!(recipes.len(), 2);
    assert!(recipes.iter().all(|r| r["version"] == 2), "{recipes:?}");

    // A fixture under the mid market-quote model: the clean mid is the quoted mid.
    let now = chrono::Utc::now().timestamp_millis();
    let event = image(&instrument, 1, now, now, "99.5");
    let fixtures = file(
        "fixtures.json",
        &json!([{
            "id": "showcase-live-mid",
            "as_of": timestamp(),
            "known_at": timestamp(),
            "events": [event],
            "expected": [{
                "instrument_id": instrument,
                "measure": "clean_price_mid",
                "value": 99.5,
                "tolerance": 0.0
            }]
        }]),
    );
    let replay = directory.path().join("fixture-events.jsonl");
    std::fs::write(&replay, format!("{event}\n")).unwrap();
    let staged = file(
        "staged.json",
        &json!({
            "document": document,
            "idempotency_key": "showcase-stage-1",
            "actor": "operator",
            "reason": "compiled from SHOWCASE-BOOK",
            "staged_at": timestamp()
        }),
    );
    let validation = config_cli(
        &path,
        &[
            "stage".as_ref(),
            staged.as_os_str(),
            fixtures.as_os_str(),
            replay.as_os_str(),
        ],
    );
    assert_eq!(validation["valid"], true, "{validation}");
    let activation = file(
        "activation.json",
        &json!({
            "id": "SHOWCASE-USD",
            "configuration_version": 1,
            "expected_revision": 0,
            "idempotency_key": "showcase-activate-1",
            "actor": "operator",
            "reason": "start the live showcase",
            "activated_at": timestamp(),
            "mode": "restart_required",
            "action": {"kind": "activate"}
        }),
    );
    let activated = config_cli(
        &path,
        &[
            "activate".as_ref(),
            activation.as_os_str(),
            fixtures.as_os_str(),
            replay.as_os_str(),
        ],
    );
    assert_eq!(activated["application"], "restart_required");

    let live = ServerConfig {
        profile: ServerProfile::Live,
        configuration_id: Some("SHOWCASE-USD".into()),
        ..config(&path)
    };
    let server = Running::start_config(live.clone()).await;
    let state = runtime(&server).await;
    assert_eq!(state["mode"], "live", "{state}");
    assert_eq!(state["ready"], false);
    let now = chrono::Utc::now().timestamp_millis();
    post_image(&server, image(&instrument, 1, now, 0, "99.5")).await;
    let first = wait_ready(&server).await;
    let results: Value = get(&server, "/api/v1/runtime/results")
        .await
        .json()
        .await
        .unwrap();
    let results = results.as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["runtime_id"], "SHOWCASE-USD-v1");
    assert!(results[0]["live_valid_until"].is_string(), "{}", results[0]);
    // Valid results pass the gate; the book names no currency, which is not defaulted.
    let (status, body) = book_analytics(&server, "SHOWCASE-BOOK").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "BOOK_CURRENCY_REQUIRED", "{body}");
    server.stop().await;

    // The same activation plans the same digest: the checkpoint restores, and the
    // runtime waits for a fresh image above the reserved revisions.
    let server = Running::start_config(live).await;
    let restored = runtime(&server).await;
    assert_eq!(restored["ready"], false, "{restored}");
    assert!(restored["revision"].as_u64() > first["revision"].as_u64());
    let now = chrono::Utc::now().timestamp_millis();
    post_image(&server, image(&instrument, 1, now, 0, "99.75")).await;
    let fresh = wait_ready(&server).await;
    assert!(fresh["revision"].as_u64() > restored["revision"].as_u64());
    server.stop().await;
    // Live stores no market snapshot and archives no result.
    let stored = convex_ext_redb::open_redb_storage(&path).unwrap();
    assert_eq!(stored.snapshots.market_snapshot_count().unwrap(), 0);
    assert!(stored.results.is_empty().unwrap());
    drop(directory);
}

/// P4: snapshots and replay results live in the redb database. File state an
/// earlier build left beside the database is refused, never silently ignored.
#[tokio::test]
async fn file_snapshot_state_beside_the_database_is_refused() {
    for suffix in ["core-snapshots", "snapshots", "results.jsonl"] {
        let (directory, path) = storage();
        std::fs::create_dir(directory.path().join(format!("state.redb.{suffix}"))).unwrap();
        let error = compose(config(&path)).await.err().unwrap();
        assert!(
            matches!(
                error,
                StartupError::Composition {
                    stage: "file_storage",
                    ..
                }
            ),
            "{error}"
        );
        assert!(!path.exists(), "storage opened before the refusal");
    }
}

/// P2b-2, the showcase boot: `deploy/showcase/live.sh activate` records the book
/// and fresh Demo curves, then compiles, validates, stages and activates under the
/// demo profile's capabilities. The demo profile runs that configuration live: Demo
/// inputs price, readiness accepts Demo, and every output stays labelled demo.
/// Without `--demo` the same document fails validation on its Demo inputs.
#[cfg(target_os = "linux")]
/// Production composes the Live path above behind required access and the TLS proxy. No
/// HTTPS identity fixture runs in process (D24), so this shows a production configuration
/// clears every gate and stops only at the identity probe, before storage opens.
#[tokio::test]
async fn production_clears_every_gate_and_stops_only_at_the_identity_probe() {
    let issuer = fixture::IssuerFixture::start().await;
    let (directory, path) = storage();
    let policy = directory.path().join("access.json");
    std::fs::write(&policy, serde_json::to_vec(&fixture::policy()).unwrap()).unwrap();
    // Production introspection is HTTPS only; the fixture speaks plain HTTP, so the TLS
    // handshake fails before any request or credential is sent.
    let mut introspection = issuer.settings.clone();
    introspection.endpoint = introspection.endpoint.replacen("http://", "https://", 1);
    introspection.allow_http_loopback = false;
    let production = ServerConfig {
        profile: ServerProfile::Production,
        configuration_id: Some("SHOWCASE-USD".into()),
        access: AccessSettings {
            mode: AccessMode::Required,
            policy_file: Some(policy.to_string_lossy().into_owned()),
            introspection: Some(introspection),
            cors_origins: vec![],
            tls_boundary: TlsBoundary::Proxy,
        },
        ..config(&path)
    };
    production.validate_startup().unwrap();
    let error = compose(production).await.err().unwrap();
    assert!(
        matches!(error, StartupError::Access(AccessError::Unavailable)),
        "{error}"
    );
    assert_eq!(
        issuer.state.calls.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert!(
        !path.exists(),
        "storage opened before identity verification"
    );
    issuer.finish().await;
}

#[tokio::test]
async fn demo_profile_runs_the_showcase_configuration_live_on_labelled_demo_data() {
    let seed = seed();
    let (directory, path) = storage();
    let (instrument, _, _) = seeded_bullet(&seed);
    import_seed(&path);
    let showcase = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/showcase");
    let bin = Path::new(env!("CARGO_BIN_EXE_convex-config"))
        .parent()
        .unwrap();
    let work = directory.path().join("boot");
    let boot = std::process::Command::new("sh")
        .arg(showcase.join("live.sh"))
        .arg("activate")
        .args([
            path.as_os_str(),
            bin.as_os_str(),
            showcase.as_os_str(),
            work.as_os_str(),
        ])
        .arg(&instrument)
        .output()
        .unwrap();
    assert!(boot.status.success(), "{boot:?}");

    // Only the demo capability admits Demo inputs.
    let refused = std::process::Command::new(env!("CARGO_BIN_EXE_convex-config"))
        .arg(&path)
        .arg("validate")
        .args(["document.json", "fixtures.json", "fixture-events.jsonl"].map(|f| work.join(f)))
        .output()
        .unwrap();
    assert!(!refused.status.success(), "{refused:?}");
    let report: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(report["valid"], false, "{report}");

    let demo = ServerConfig {
        configuration_id: Some("SHOWCASE-USD".into()),
        ..config(&path)
    };
    let server = Running::start_config(demo).await;
    let runtime = runtime(&server).await;
    assert_eq!(runtime["mode"], "live", "{runtime}");
    assert_eq!(runtime["ready"], false);
    let now = chrono::Utc::now().timestamp_millis();
    post_image(&server, image(&instrument, 1, now, 0, "99.5")).await;
    let ready = wait_ready(&server).await;
    assert_eq!(ready["quality"], "Demo", "{ready}");
    let response = get(&server, "/api/v1/runtime/results").await;
    assert_eq!(response.headers()["x-convex-data-quality"], "demo");
    let results: Value = response.json().await.unwrap();
    let result = &results[0];
    assert_eq!(result["runtime_id"], "SHOWCASE-USD-v1", "{result}");
    assert_eq!(result["result"]["status"], "Complete", "{result}");
    assert_eq!(result["result"]["quality"], "Demo");
    assert_eq!(
        result["result"]["results"]["clean_price_mid"]["quality"],
        "Demo"
    );
    let measures = &result["result"]["results"];
    for (measure, unit) in [
        ("modified_duration_mid", "years"),
        ("convexity_mid", "years_squared"),
        ("dv01_mid", "price_per_100"),
        ("z_spread_mid", "bp"),
    ] {
        assert_eq!(
            measures[measure]["value"]["unit"], unit,
            "{measure}: {result}"
        );
        assert!(measures[measure]["value"]["value"].is_f64(), "{result}");
    }

    // Analytics by book over the Demo results. One position of 1,000,000 face: each
    // weighted measure is the bond's own, and market value is dirty price times face.
    let response = server
        .client
        .post(server.url("/api/v1/portfolio/analytics"))
        .json(&json!({"book_id": "SHOWCASE-BOOK"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-convex-data-quality"], "demo");
    let analytics: Value = response.json().await.unwrap();
    let measure = |name: &str| number(&measures[name]["value"]["value"]);
    let close = |name: &str, actual: f64, expected: f64| {
        assert!(
            (actual - expected).abs() <= expected.abs() * 1e-9,
            "{name}: {actual} vs {expected}: {analytics}"
        );
    };
    assert_eq!(analytics["num_positions"], 1, "{analytics}");
    let dirty = measure("clean_price_mid") + measure("accrued_interest");
    close(
        "market_value",
        number(&analytics["market_value"]),
        dirty * 10_000.0,
    );
    close(
        "duration",
        number(&analytics["duration"]),
        measure("modified_duration_mid"),
    );
    close(
        "convexity",
        number(&analytics["convexity"]),
        measure("convexity_mid"),
    );
    close(
        "yield",
        number(&analytics["yield_value"]),
        measure("ytm_mid"),
    );
    close(
        "spread",
        number(&analytics["spread"]),
        measure("z_spread_mid"),
    );
    close(
        "dv01",
        number(&analytics["dv01"]),
        measure("dv01_mid") * 10_000.0,
    );

    // A position held in units has no defined conversion to face.
    let units = server
        .client
        .put(server.url("/api/v1/books/UNITS-BOOK"))
        .header("idempotency-key", "units-book")
        .json(&json!({"currency": "USD", "positions": [
            {"instrument_id": instrument, "quantity": "10", "unit": "units"}
        ]}))
        .send()
        .await
        .unwrap();
    assert!(units.status().is_success(), "{}", units.status());
    let (status, body) = book_analytics(&server, "UNITS-BOOK").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "UNSUPPORTED_QUANTITY_UNIT", "{body}");
    server.stop().await;
}

/// A JSON number, or a decimal string.
#[cfg(target_os = "linux")]
fn number(value: &Value) -> f64 {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or_else(|| panic!("not a number: {value}"))
}

async fn book_analytics(server: &Running, book: &str) -> (StatusCode, Value) {
    let response = server
        .client
        .post(server.url("/api/v1/portfolio/analytics"))
        .json(&json!({ "book_id": book }))
        .send()
        .await
        .unwrap();
    (response.status(), response.json().await.unwrap())
}

/// Latest instant this process has stamped or seen the server stamp.
static FLOOR: std::sync::Mutex<Option<chrono::DateTime<chrono::Utc>>> = std::sync::Mutex::new(None);

/// The current instant in the canonical timestamp form, never before an instant
/// already used: the WSL wall clock steps back by up to about a second.
fn timestamp() -> Value {
    let mut floor = FLOOR.lock().unwrap();
    let now = floor.map_or(chrono::Utc::now(), |f| f.max(chrono::Utc::now()));
    *floor = Some(now);
    serde_json::to_value(convex_core::contracts::UtcTimestamp::from_datetime(now).unwrap()).unwrap()
}

/// Keep later test timestamps at or after an instant the server recorded.
fn not_before(recorded: &str) {
    let recorded = convex_core::contracts::UtcTimestamp::parse(recorded)
        .unwrap()
        .datetime();
    let mut floor = FLOOR.lock().unwrap();
    *floor = Some(floor.map_or(recorded, |f| f.max(recorded)));
}

/// Run the offline configuration operator on the stopped store and parse its report.
fn config_cli(storage: &Path, args: &[&std::ffi::OsStr]) -> Value {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_convex-config"))
        .arg(storage)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

/// One canonical mid image for `instrument` from the showcase feed.
fn image(instrument: &str, sequence: u64, source_ms: i64, receive_ms: i64, mid: &str) -> Value {
    json!({
        "schema_version": 1,
        "key": {"instrument_id": instrument, "source": "showcase-feed"},
        "session": "showcase-1",
        "sequence": sequence,
        "source_time_ms": source_ms,
        "receive_time_ms": receive_ms,
        "quality": "live",
        "entitled": true,
        "entitlement_tags": [],
        "evidence_reference": null,
        "event": {"kind": "image", "fields": [{
            "field": "mid_price",
            "update": {"action": "set", "value": {"value": mid, "unit": "price_per100"}}
        }]}
    })
}

async fn post_image(server: &Running, event: Value) {
    let response = server
        .client
        .post(server.url("/api/v1/market/quotes"))
        .json(&json!({ "events": [event] }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

async fn runtime(server: &Running) -> Value {
    get(server, "/api/v1/runtime").await.json().await.unwrap()
}

async fn wait_ready(server: &Running) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let runtime = runtime(server).await;
            if runtime["ready"] == true {
                return runtime;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap()
}
