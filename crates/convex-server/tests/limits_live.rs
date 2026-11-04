//! Book limit status over Live results (P14b): the `live` profile over the real
//! composition, fed by posted quote images. Status reads the same accepted results that
//! analytics by book read, names the snapshots it read, and reports each reason a result
//! leaves a limit unknown. `status_route_time_at_the_bounds` is the slice's measurement
//! (`--ignored --nocapture`).
#[path = "support/live_server.rs"]
mod live_server;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use convex_core::contracts::{MeasureId, ModelId, RequestId, UtcTimestamp};
use convex_engine::ports::quote_event::QuoteEvent;
use convex_engine::ports::reference_repository::{
    ReferenceChange, ReferenceCommand, MAX_REFERENCE_CHANGES,
};
use convex_engine::RuntimeMode;
use convex_server::{ServerConfig, ServerProfile};
use live_server::{config, image, Running};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// A Live configuration valuing `priced` bonds (the fixture bond and copies `BOND-W001`...)
/// for `measures`, with `unpriced` further copies stored but outside the plan.
async fn live(
    priced: usize,
    unpriced: usize,
    measures: &[&str],
) -> (tempfile::TempDir, ServerConfig, Vec<String>) {
    let s = support::setup().await;
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
    if !measures.is_empty() {
        plan.measures = measures
            .iter()
            .map(|m| MeasureId::new(*m).unwrap())
            .collect();
    }
    support::widen(&s, &mut plan, priced).await;
    let extra: Vec<_> = (priced..priced + unpriced)
        .map(|index| {
            let mut bond = support::fixture::bond();
            bond.instrument_id = convex_core::ids::InstrumentId::new(format!("BOND-W{index:03}"));
            bond.isin = Some(format!("fixture-isin-w{index:03}"));
            bond.cusip = Some(format!("fixture-cusip-w{index:03}"));
            bond
        })
        .collect();
    for (chunk, bonds) in extra.chunks(MAX_REFERENCE_CHANGES).enumerate() {
        s.storage
            .bonds
            .commit(
                &ReferenceCommand {
                    idempotency_key: RequestId::new(format!("unpriced-{chunk}")).unwrap(),
                    changes: bonds
                        .iter()
                        .map(|bond| ReferenceChange::Create {
                            bond: Box::new(bond.clone()),
                        })
                        .collect(),
                    master_changes: vec![],
                },
                support::fixture::time("2025-01-01T00:00:00Z"),
            )
            .await
            .unwrap();
    }
    let instruments = plan
        .assembly
        .instruments
        .iter()
        .map(|pin| pin.instrument_id.as_str().to_owned())
        .chain(extra.iter().map(|b| b.instrument_id.as_str().to_owned()))
        .collect();
    let events: Vec<QuoteEvent> = plan
        .assembly
        .quotes
        .iter()
        .map(|quote| {
            let mut event = image(1, "99.25");
            event.key = quote.key.clone();
            event
        })
        .collect();
    let plan_path = s.dir.path().join("runtime.json");
    std::fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
    std::fs::write(
        s.dir.path().join("images.json"),
        serde_json::to_vec(&events).unwrap(),
    )
    .unwrap();
    let database = s.dir.path().join("inputs.redb");
    let dir = s.dir;
    drop(s.storage);
    drop(s.store);
    drop(s.snapshots);
    drop(s.results);
    drop(s.fixings);
    let live = config(ServerProfile::Live, &database, Some(&plan_path));
    (dir, live, instruments)
}

/// Start the server and post one image per quoted key, then wait for readiness.
async fn started(dir: &tempfile::TempDir, live: ServerConfig) -> Running {
    let server = Running::start(live).await;
    let events: Vec<QuoteEvent> =
        serde_json::from_slice(&std::fs::read(dir.path().join("images.json")).unwrap()).unwrap();
    let posted = server.post_quotes(events).await;
    assert!(posted.status().is_success(), "{}", posted.status());
    server.wait_ready().await;
    server
}

async fn send(
    server: &Running,
    method: reqwest::Method,
    path: &str,
    key: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = server
        .client
        .request(method, format!("{}{path}", server.base));
    if let Some(key) = key {
        request = request.header("idempotency-key", key);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn put(server: &Running, path: &str, key: &str, body: Value) {
    let (status, response) = send(server, reqwest::Method::PUT, path, Some(key), Some(body)).await;
    assert!(status.is_success(), "{path}: {status} {response}");
}

fn number(value: &Value) -> f64 {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or_else(|| panic!("not a number: {value}"))
}

fn by_id(status: &Value) -> BTreeMap<String, Value> {
    status["limits"]
        .as_array()
        .unwrap_or_else(|| panic!("no limits: {status}"))
        .iter()
        .map(|item| (item["limit_id"].as_str().unwrap().to_owned(), item.clone()))
        .collect()
}

fn limit(id: &str, measure: Value) -> Value {
    json!({"limit_id": id, "measure": measure, "upper": "1000000000"})
}

#[tokio::test]
async fn status_reads_live_results_and_names_the_snapshots_it_read() {
    let (dir, live, _) = live(2, 0, &[]).await;
    let server = started(&dir, live).await;
    let (_, results) = send(
        &server,
        reqwest::Method::GET,
        "/api/v1/runtime/results",
        None,
        None,
    )
    .await;
    let result = &results[0]["result"]["results"];
    let dirty = number(&result["clean_price_mid"]["value"]["value"])
        + number(&result["accrued_interest"]["value"]["value"]);

    put(
        &server,
        "/api/v1/books/LIVE",
        "book-live",
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "1000000", "rating": "A"},
            {"instrument_id": "BOND-W001", "quantity": "-400000"},
        ]}),
    )
    .await;
    put(
        &server,
        "/api/v1/books/EUR",
        "book-eur",
        json!({"currency": "EUR", "positions": [
            {"instrument_id": "BOND-001", "quantity": "1000000"},
        ]}),
    )
    .await;
    let limits = json!({"limits": [
        limit("net", json!({"kind": "market_value"})),
        limit("gross", json!({"kind": "gross_market_value"})),
        limit("dv01", json!({"kind": "dv01"})),
        limit("issuer", json!({"kind": "group_market_value", "by": "issuer", "group": "each"})),
        limit("rating", json!({"kind": "group_market_value", "by": "rating", "group": "each"})),
        limit("face", json!({"kind": "face", "instrument": "each"})),
    ]});
    put(
        &server,
        "/api/v1/books/LIVE/limits",
        "limits-live",
        limits.clone(),
    )
    .await;
    put(&server, "/api/v1/books/EUR/limits", "limits-eur", limits).await;

    let (code, status) = send(
        &server,
        reqwest::Method::GET,
        "/api/v1/books/LIVE/limits/status",
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{status}");
    let snapshots = status["snapshot_ids"].as_array().unwrap();
    assert!(
        !snapshots.is_empty() && snapshots.iter().all(Value::is_string),
        "{status}"
    );
    let limits = by_id(&status);
    let close = |id: &str, expected: f64| {
        let actual = number(&limits[id]["value"]);
        assert!(
            (actual - expected).abs() <= 1e-6,
            "{id}: {actual} vs {expected}: {}",
            limits[id]
        );
    };
    close("net", dirty / 100.0 * 600_000.0);
    close("gross", dirty / 100.0 * 1_400_000.0);
    close("issuer", dirty / 100.0 * 600_000.0);
    assert_eq!(
        limits["issuer"]["member"], "ISSUER-001",
        "{}",
        limits["issuer"]
    );
    // The fixture runtime computes prices and yield only.
    assert_eq!(
        limits["dv01"]["reason"],
        json!({"code": "measure_not_computed", "measure": "dv01_mid",
               "instruments": {"count": 2, "first": ["BOND-001", "BOND-W001"]}}),
        "{}",
        limits["dv01"]
    );
    assert_eq!(
        limits["rating"]["reason"],
        json!({"code": "missing_classification", "by": "rating",
               "instruments": {"count": 1, "first": ["BOND-W001"]}}),
        "{}",
        limits["rating"]
    );
    assert_eq!(limits["face"]["status"], "within");
    assert_eq!(limits["face"]["member"], "BOND-001");

    let (code, status) = send(
        &server,
        reqwest::Method::GET,
        "/api/v1/books/EUR/limits/status",
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{status}");
    let limits = by_id(&status);
    assert_eq!(
        limits["net"]["reason"],
        json!({"code": "foreign_currency", "instruments": {"count": 1, "first": ["BOND-001"]}})
    );
    assert_eq!(limits["face"]["status"], "within");
    server.stop().await;
}

/// The measures analytics by book need, and those limits read.
const MEASURES: [&str; 14] = [
    "clean_price_mid",
    "dirty_price_mid",
    "accrued_interest",
    "ytm_mid",
    "modified_duration_mid",
    "convexity_mid",
    "dv01_mid",
    "z_spread_mid",
    "cs01_mid",
    "krd_1y_mid",
    "krd_2y_mid",
    "krd_5y_mid",
    "krd_10y_mid",
    "krd_30y_mid",
];
const TENORS: [&str; 5] = ["1Y", "2Y", "5Y", "10Y", "30Y"];
const RATINGS: [&str; 5] = ["AAA", "AA", "A", "BBB", "BB"];

/// 64 limits over every measure, as the P14a bench sets them.
fn every_measure() -> Value {
    let mut measures = vec![json!({"kind": "face", "instrument": "each"})];
    for i in 0..7 {
        measures.push(
            json!({"kind": "face", "instrument": {"one": format!("BOND-W{:03}", i * 31 + 1)}}),
        );
    }
    for kind in ["market_value", "gross_market_value"] {
        measures.extend([json!({ "kind": kind }), json!({ "kind": kind })]);
    }
    for kind in ["dv01", "cs01"] {
        measures.extend((0..4).map(|_| json!({ "kind": kind })));
    }
    measures.extend((0..10).map(|i| json!({"kind": "key_rate_dv01", "tenor": TENORS[i % 5]})));
    measures.extend([
        json!({"kind": "modified_duration"}),
        json!({"kind": "modified_duration"}),
    ]);
    for kind in ["group_market_value", "group_share"] {
        for by in ["issuer", "country", "sector", "rating"] {
            measures.push(json!({"kind": kind, "by": by, "group": "each"}));
        }
        for rating in &RATINGS[..3] {
            measures.push(json!({"kind": kind, "by": "issuer", "group": {"one": "ISSUER-001"}}));
            measures.push(json!({"kind": kind, "by": "country", "group": {"one": "US"}}));
            measures.push(json!({"kind": kind, "by": "sector", "group": {"one": "Technology"}}));
            measures.push(json!({"kind": kind, "by": "rating", "group": {"one": rating}}));
        }
    }
    assert_eq!(measures.len(), 64);
    let limits: Vec<Value> = measures
        .into_iter()
        .enumerate()
        .map(|(i, measure)| {
            json!({"limit_id": format!("limit-{i:02}"), "measure": measure,
            "lower": "-100000000000", "upper": "100000000000", "warning_fraction": "0.8"})
        })
        .collect();
    json!({ "limits": limits })
}

fn book(instruments: &[String]) -> Value {
    let positions: Vec<Value> = instruments
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let sign = if i % 3 == 2 { "-" } else { "" };
            json!({"instrument_id": id, "quantity": format!("{sign}{}", 100_000 + i * 1_000),
                   "rating": RATINGS[i % 5]})
        })
        .collect();
    json!({"currency": "USD", "positions": positions})
}

fn summary(mut times: Vec<Duration>) -> String {
    times.sort();
    let at = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize].as_secs_f64() * 1e3;
    format!(
        "median {:.2} ms, p90 {:.2} ms, min {:.2} ms (n={})",
        at(0.5),
        at(0.9),
        at(0.0),
        times.len()
    )
}

async fn timed(
    server: &Running,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Duration {
    let start = Instant::now();
    let (status, response) = send(server, method, path, None, body).await;
    let elapsed = start.elapsed();
    assert_eq!(status, StatusCode::OK, "{path}: {response}");
    elapsed
}

/// P14b's measurement: `GET .../limits/status` for a book with 64 limits over every
/// measure, against `POST /api/v1/portfolio/analytics` for the same book. A runtime values
/// at most 256 instruments, so the largest book with a result for every position holds
/// 256; a 1,000-position book has results for 256 of them.
#[tokio::test]
#[ignore = "measurement; run with --ignored --nocapture under the ci profile"]
async fn status_route_time_at_the_bounds() {
    const ROUNDS: usize = 40;
    let (dir, live, instruments) = live(256, 744, &MEASURES).await;
    let server = started(&dir, live).await;
    put(
        &server,
        "/api/v1/books/PRICED",
        "book-priced",
        book(&instruments[..256]),
    )
    .await;
    put(
        &server,
        "/api/v1/books/BOUND",
        "book-bound",
        book(&instruments),
    )
    .await;
    put(
        &server,
        "/api/v1/books/PRICED/limits",
        "limits-priced",
        every_measure(),
    )
    .await;
    put(
        &server,
        "/api/v1/books/BOUND/limits",
        "limits-bound",
        every_measure(),
    )
    .await;

    let (code, status) = send(
        &server,
        reqwest::Method::GET,
        "/api/v1/books/PRICED/limits/status",
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{status}");
    let unknown: Vec<_> = by_id(&status)
        .into_values()
        .filter(|l| l["status"] == "unknown")
        .collect();
    assert!(
        unknown.is_empty(),
        "every limit is evaluated on a fully priced book: {unknown:?}"
    );
    let analytics = json!({"book_id": "PRICED"});
    let (code, body) = send(
        &server,
        reqwest::Method::POST,
        "/api/v1/portfolio/analytics",
        None,
        Some(analytics.clone()),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let net = by_id(&status)["limit-08"]["value"].clone();
    assert!(
        (number(&net) - number(&body["market_value"])).abs() <= 1e-6,
        "status and analytics read the same results: {net} vs {}",
        body["market_value"]
    );

    let (mut priced, mut bound, mut book_analytics) = (Vec::new(), Vec::new(), Vec::new());
    for round in 0..ROUNDS + 5 {
        let a = timed(
            &server,
            reqwest::Method::GET,
            "/api/v1/books/PRICED/limits/status",
            None,
        )
        .await;
        let b = timed(
            &server,
            reqwest::Method::POST,
            "/api/v1/portfolio/analytics",
            Some(analytics.clone()),
        )
        .await;
        let c = timed(
            &server,
            reqwest::Method::GET,
            "/api/v1/books/BOUND/limits/status",
            None,
        )
        .await;
        if round >= 5 {
            priced.push(a);
            book_analytics.push(b);
            bound.push(c);
        }
    }
    println!(
        "limits/status, 256 positions all priced, 64 limits: {}",
        summary(priced)
    );
    println!(
        "portfolio/analytics by book, same 256 positions:   {}",
        summary(book_analytics)
    );
    println!(
        "limits/status, 1,000 positions (256 priced), 64 limits: {}",
        summary(bound)
    );
    server.stop().await;
}
