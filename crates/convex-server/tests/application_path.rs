//! Acceptance test for the one application path: instrument/book create or import →
//! market inputs → pricing → publication → client stream, over the real composition
//! (`convex_server::compose`) and real HTTP.
//!
//! Inputs are the committed showcase seed (`deploy/showcase/seed.json`), so the
//! fixture the Fly container imports is exercised by CI. Steps that need a later
//! phase of `.ai/server-plan/IMPLEMENTATION_PLAN.md` are `#[ignore]`d with that phase.
use convex_server::access::identity::{AccessMode, AccessSettings};
use convex_server::{compose, ServerConfig, ServerProfile, StartupError};
use reqwest::{Client, StatusCode};
use serde_json::Value;
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
        let composition = compose(config(storage)).await.unwrap();
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

/// Steps 4–8 (P2, P3): compile and activate a plan from the repository, POST live
/// quotes, receive a WebSocket `valuation`, read `/api/v1/runtime/results`, and after
/// a restart recover live values from fresh ticks rather than a stored tick history.
#[tokio::test]
#[ignore = "P2: plan compiled from the repository and POST /api/v1/market/quotes"]
async fn live_quotes_price_publish_and_recover_after_restart() {
    let seed = seed();
    let (_directory, path) = storage();
    let server = Running::start(&path).await;
    commit_references(&server, &seed).await;
    let runtime: Value = server
        .client
        .get(server.url("/api/v1/runtime"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(runtime["mode"], "live", "{runtime}");
    let (instrument, _, _) = seeded_bullet(&seed);
    let response = server
        .client
        .post(server.url("/api/v1/market/quotes"))
        .json(&serde_json::json!({
            "quotes": [{"instrument_id": instrument, "clean_price_mid": "99.5"}]
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "{}", response.status());
    let results: Value = server
        .client
        .get(server.url("/api/v1/runtime/results"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!results.as_array().unwrap().is_empty());
    server.stop().await;
}
