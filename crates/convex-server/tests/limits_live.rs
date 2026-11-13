//! Book limit status over Live results (P14b): the `live` profile over the real
//! composition, fed by posted quote images. Status reads the same accepted results that
//! analytics by book read, names the snapshots it read, and reports each reason a result
//! leaves a limit unknown. `status_route_time_at_the_bounds` is the slice's measurement
//! (`--ignored --nocapture`). A pre-trade check (P14d) equals the status of the book with
//! its changes committed.
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
    let result = &results["results"][0]["result"]["results"];
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

/// P15c's measurement: `GET .../limits/status` for a book with 64 limits over every
/// measure, each in breach and acknowledged with a note of 1,024 bytes, against the same
/// book's positions under limits that are all within and none acknowledged, in the same
/// rounds. Bounds of 10^15 to 2·10^15 put every value, zero included, in breach.
#[tokio::test]
#[ignore = "measurement; run with --ignored --nocapture under the ci profile"]
async fn status_route_time_with_acknowledgements_at_the_bounds() {
    const ROUNDS: usize = 40;
    let (dir, live, instruments) = live(256, 0, &MEASURES).await;
    let server = started(&dir, live).await;
    let mut breaching = every_measure();
    for limit in breaching["limits"].as_array_mut().unwrap() {
        let limit = limit.as_object_mut().unwrap();
        limit.remove("warning_fraction");
        limit.insert("lower".into(), json!("1000000000000000"));
        limit.insert("upper".into(), json!("2000000000000000"));
    }
    for (id, limits) in [("PRICED", every_measure()), ("ACKED", breaching)] {
        put(
            &server,
            &format!("/api/v1/books/{id}"),
            &format!("book-{id}"),
            book(&instruments),
        )
        .await;
        put(
            &server,
            &format!("/api/v1/books/{id}/limits"),
            &format!("limits-{id}"),
            limits,
        )
        .await;
    }
    let expires_at = (chrono::Utc::now() + chrono::Duration::hours(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    for i in 0..64 {
        let body = json!({"limit_id": format!("limit-{i:02}"), "expires_at": expires_at,
                          "note": format!("{i:02}{}", "n".repeat(1022))});
        let (code, response) = send(
            &server,
            reqwest::Method::POST,
            "/api/v1/books/ACKED/limits/acknowledgements",
            Some(&format!("ack-{i}")),
            Some(body),
        )
        .await;
        assert_eq!(code, StatusCode::CREATED, "limit-{i:02}: {response}");
    }
    let (code, status) = send(
        &server,
        reqwest::Method::GET,
        "/api/v1/books/ACKED/limits/status",
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{status}");
    let carried = by_id(&status)
        .into_values()
        .filter(|l| l["status"] == "breach" && l["acknowledgement"]["note"].is_string())
        .count();
    assert_eq!(
        carried, 64,
        "every limit breaches and carries its acknowledgement"
    );
    let (code, priced) = send(
        &server,
        reqwest::Method::GET,
        "/api/v1/books/PRICED/limits/status",
        None,
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{priced}");
    println!(
        "response bytes: {} with 64 acknowledgements, {} without",
        status.to_string().len(),
        priced.to_string().len()
    );

    let (mut plain, mut acked) = (Vec::new(), Vec::new());
    for round in 0..ROUNDS + 5 {
        let paths = [
            "/api/v1/books/PRICED/limits/status",
            "/api/v1/books/ACKED/limits/status",
        ];
        // Alternate which goes first, so neither always follows the other.
        let order = if round % 2 == 0 { [0, 1] } else { [1, 0] };
        let mut times = [Duration::ZERO; 2];
        for index in order {
            times[index] = timed(&server, reqwest::Method::GET, paths[index], None).await;
        }
        if round >= 5 {
            plain.push(times[0]);
            acked.push(times[1]);
        }
    }
    println!(
        "limits/status, 256 positions priced, 64 limits, none acknowledged: {}",
        summary(plain)
    );
    println!(
        "limits/status, 256 positions priced, 64 limits in breach, 64 acknowledged: {}",
        summary(acked)
    );
    server.stop().await;
}

// =============================================================================
// THE MONITOR (P14c)
// =============================================================================

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The next frame of type `kind`, skipping heartbeats and any other frame.
async fn frame(socket: &mut Socket, kind: &str) -> Value {
    use futures_util::StreamExt;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match socket.next().await {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    if value["type"] == kind {
                        return value;
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended: {other:?}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no {kind} frame"))
}

/// A session subscribed to the limit status changes of `books`.
async fn limit_session(server: &Running, books: &[&str]) -> Socket {
    use futures_util::SinkExt;
    let (mut socket, _) = tokio_tungstenite::connect_async(server.ws_url())
        .await
        .unwrap();
    frame(&mut socket, "connected").await;
    let subscribe = json!({"type": "subscribe_limits", "book_ids": books});
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            subscribe.to_string().into(),
        ))
        .await
        .unwrap();
    let ack = frame(&mut socket, "subscribed").await;
    assert_eq!(ack["subscription_type"], "limits", "{ack}");
    socket
}

/// `PUT` over the version `if_match` names; local development commits a limit set at once.
async fn put_over(server: &Running, path: &str, key: &str, if_match: &str, body: Value) {
    let response = server
        .client
        .put(format!("{}{path}", server.base))
        .header("idempotency-key", key)
        .header("if-match", if_match)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(status, StatusCode::OK, "{}", response.text().await.unwrap());
}

/// Post one image of the fixture bond at `price`.
async fn tick(server: &Running, dir: &tempfile::TempDir, sequence: u64, price: &str) {
    let images: Vec<QuoteEvent> =
        serde_json::from_slice(&std::fs::read(dir.path().join("images.json")).unwrap()).unwrap();
    let mut event = image(sequence, price);
    event.key = images[0].key.clone();
    let posted = server.post_quotes(vec![event]).await;
    assert!(posted.status().is_success(), "{}", posted.status());
}

/// Every recorded status change, oldest first, read from the stopped server's database.
async fn records(live: &ServerConfig) -> Vec<(u64, Value)> {
    use convex_engine::ports::storage::{AuditFilter, Pagination};
    let storage = convex_ext_redb::open_redb_storage(&live.storage_path).unwrap();
    let filter = AuditFilter {
        event_type: Some("limit_status".into()),
        ..Default::default()
    };
    let mut items = Vec::new();
    loop {
        let page = storage
            .storage
            .audit
            .query(&filter, &Pagination::new(items.len(), 1000))
            .await
            .unwrap();
        let done = page.items.len() < 1000;
        items.extend(page.items);
        if done {
            break;
        }
    }
    items
        .into_iter()
        .map(|entry| {
            assert_eq!(entry.entity_type, "book");
            let details: Value = serde_json::from_str(entry.details.as_deref().unwrap()).unwrap();
            assert_eq!(details["record_id"], entry.id);
            assert_eq!(details["book_id"], entry.entity_id);
            assert_eq!(details["status"], entry.action);
            (entry.id, details)
        })
        .collect()
}

/// Wait until `/metrics` shows `line`: the gauges follow each book's records.
async fn metric(server: &Running, line: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let text = server
                .client
                .get(format!("{}/metrics", server.base))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap();
            if text.lines().any(|l| l == line) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("/metrics never showed {line}"))
}

/// The acknowledgement of the last request, failing on an error frame.
async fn acknowledged(socket: &mut Socket) -> Value {
    use futures_util::StreamExt;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match socket.next().await {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    match value["type"].as_str() {
                        Some("subscribed") => return value,
                        Some("error") => panic!("refused: {value}"),
                        _ => {}
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended: {other:?}"),
            }
        }
    })
    .await
    .expect("no acknowledgement")
}

/// D100: one session can follow every instrument a runtime values and every book the monitor
/// watches. One quota of 256 topics used to cover both, so a session following 256
/// instruments could follow no book.
#[tokio::test]
async fn one_session_follows_every_instrument_and_every_limited_book() {
    use futures_util::SinkExt;
    let (dir, live, _) = live(1, 0, &[]).await;
    let server = started(&dir, live.clone()).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(server.ws_url())
        .await
        .unwrap();
    frame(&mut socket, "connected").await;
    let mut instruments = vec!["BOND-001".to_owned()];
    instruments.extend((1..convex_engine::MAX_RUNTIME_INSTRUMENTS).map(|i| format!("ANY-{i:03}")));
    let books: Vec<String> = (0..64).map(|i| format!("BOOK-{i:02}")).collect();
    for (request, kind, count) in [
        (
            json!({"type": "subscribe_valuations", "instrument_ids": instruments}),
            "valuations",
            256,
        ),
        (
            json!({"type": "subscribe_limits", "book_ids": books}),
            "limits",
            64,
        ),
        // Listing books already followed, one of them twice, takes no new place (Excel
        // GX25): the session still follows 64 books, and the count is of distinct books.
        (
            json!({"type": "subscribe_limits", "book_ids": [&books[0], &books[63], &books[0]]}),
            "limits",
            2,
        ),
    ] {
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                request.to_string().into(),
            ))
            .await
            .unwrap();
        let ack = acknowledged(&mut socket).await;
        assert_eq!(ack["subscription_type"], kind, "{ack}");
        assert_eq!(ack["count"], count, "{ack}");
    }
}

/// The monitor records a change when a limit set commits, when a tick moves a value past
/// a bound, and when the book changes; it publishes each one after recording it; and a
/// restart records each status afresh, from `unevaluated`, naming its start. A cleared
/// set is no longer watched.
#[tokio::test]
async fn the_monitor_records_and_publishes_each_status_change_and_restates_them_at_a_restart() {
    let (dir, live, _) = live(1, 0, &[]).await;
    let server = started(&dir, live.clone()).await;
    put(
        &server,
        "/api/v1/books/WATCHED",
        "book-watched",
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "1000000"},
        ]}),
    )
    .await;
    let mut socket = limit_session(&server, &["WATCHED"]).await;

    // The first set: both limits are recorded from `unevaluated`.
    let first = json!({"limits": [
        limit("net", json!({"kind": "market_value"})),
        json!({"limit_id": "face", "measure": {"kind": "face", "instrument": {"one": "BOND-001"}},
               "upper": "2000000"}),
    ]});
    put(&server, "/api/v1/books/WATCHED/limits", "set-1", first).await;
    let mut seen = BTreeMap::new();
    let mut published = Vec::new();
    for _ in 0..2 {
        let change = frame(&mut socket, "limit_status").await;
        published.push(change.clone());
        assert_eq!(change["previous"], "unevaluated", "{change}");
        assert_eq!(change["status"], "within", "{change}");
        assert_eq!(change["book_id"], "WATCHED");
        assert_eq!(change["limit_set_version"], 1);
        seen.insert(change["limit_id"].as_str().unwrap().to_owned(), change);
    }
    assert_eq!(
        seen.keys().cloned().collect::<Vec<_>>(),
        ["face", "net"],
        "{seen:?}"
    );
    let net = number(&seen["net"]["value"]);
    assert!(!seen["net"]["snapshot_ids"].as_array().unwrap().is_empty());

    // A tighter bound that the current value is within: no change, nothing recorded.
    let upper = format!("{:.2}", net * 1.005);
    let second = json!({"limits": [
        {"limit_id": "net", "measure": {"kind": "market_value"}, "upper": upper},
        {"limit_id": "face", "measure": {"kind": "face", "instrument": {"one": "BOND-001"}},
         "upper": "2000000"},
    ]});
    put_over(
        &server,
        "/api/v1/books/WATCHED/limits",
        "set-2",
        "1",
        second,
    )
    .await;

    // A tick 1.5 points up moves the value past it.
    tick(&server, &dir, 2, "100.75").await;
    let change = frame(&mut socket, "limit_status").await;
    assert_eq!(
        (&change["limit_id"], &change["previous"], &change["status"]),
        (&json!("net"), &json!("within"), &json!("breach")),
        "{change}"
    );
    assert_eq!(change["limit_set_version"], 2);
    assert!(number(&change["value"]) > net, "{change}");
    published.push(change.clone());
    metric(&server, "convex_limit_breaches{book_id=\"WATCHED\"} 1").await;

    // A book commit is evaluated too: a larger position breaches the face limit.
    put_over(
        &server,
        "/api/v1/books/WATCHED",
        "book-watched-2",
        "1",
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "3000000"},
        ]}),
    )
    .await;
    let change = frame(&mut socket, "limit_status").await;
    assert_eq!(
        (&change["limit_id"], &change["previous"], &change["status"]),
        (&json!("face"), &json!("within"), &json!("breach")),
        "{change}"
    );
    assert_eq!(change["book_version"], 2);
    let started_first = change["monitor_started_at"].clone();
    published.push(change);

    // The records route returns each change as its frame carried it, oldest first (P15b).
    let path = "/api/v1/books/WATCHED/limits/records";
    let (status, page) = send(&server, reqwest::Method::GET, path, None, None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    published.sort_by_key(|frame| frame["record_id"].as_u64());
    for frame in &mut published {
        frame.as_object_mut().unwrap().remove("type");
    }
    assert_eq!(page["records"], Value::Array(published), "{page}");
    assert_eq!(page["more"], false);
    drop(socket);
    server.stop().await;

    let before = records(&live).await;
    assert_eq!(before.len(), 4, "{before:?}");
    assert!(before.windows(2).all(|w| w[0].0 < w[1].0), "{before:?}");
    let last_id = before.last().unwrap().0;

    // A restart records every status again, from `unevaluated`, naming the new start.
    // The session subscribes before any result exists. Both limits are in breach: the
    // position is three times the face the net bound was set for.
    let server = Running::start(live.clone()).await;
    let mut socket = limit_session(&server, &["WATCHED"]).await;
    let images: Vec<QuoteEvent> =
        serde_json::from_slice(&std::fs::read(dir.path().join("images.json")).unwrap()).unwrap();
    // Ingress answers 503 across a backward clock step (WSL2 steps under load); retry.
    let mut attempts = 0;
    loop {
        let posted = server.post_quotes(images.clone()).await;
        let status = posted.status();
        if status.is_success() {
            break;
        }
        attempts += 1;
        assert!(
            status == StatusCode::SERVICE_UNAVAILABLE && attempts < 10,
            "{status}: {}",
            posted.text().await.unwrap()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    server.wait_ready().await;
    // The net limit reaches `breach` from `unevaluated`, or through `unknown` if the
    // monitor's first evaluation found no result yet.
    loop {
        let change = frame(&mut socket, "limit_status").await;
        if change["limit_id"] == "net" && change["status"] == "breach" {
            assert_ne!(change["monitor_started_at"], started_first, "{change}");
            break;
        }
    }
    metric(&server, "convex_limit_breaches{book_id=\"WATCHED\"} 2").await;

    // Cleared, the set is no longer watched: the book returning within both bounds
    // records nothing.
    put_over(
        &server,
        "/api/v1/books/WATCHED/limits",
        "set-3",
        "2",
        json!({"limits": []}),
    )
    .await;
    metric(&server, "convex_limit_breaches{book_id=\"WATCHED\"} 0").await;
    put_over(
        &server,
        "/api/v1/books/WATCHED",
        "book-watched-3",
        "2",
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "1000000"},
        ]}),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (status, history) = send(&server, reqwest::Method::GET, path, None, None).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    drop(socket);
    server.stop().await;

    let after = records(&live).await;
    // The route reads every record of a book whose set is now empty, across the restart.
    let stored: Vec<Value> = after.iter().map(|(_, details)| details.clone()).collect();
    assert_eq!(history["records"], Value::Array(stored));
    let restated: Vec<&Value> = after
        .iter()
        .filter(|(id, _)| *id > last_id)
        .map(|(_, details)| details)
        .collect();
    assert!(restated
        .iter()
        .all(|d| d["monitor_started_at"] != started_first));
    for limit in ["net", "face"] {
        let of: Vec<&&Value> = restated.iter().filter(|d| d["limit_id"] == limit).collect();
        assert_eq!(of[0]["previous"], "unevaluated", "{limit}: {of:?}");
    }
    let face: Vec<&&Value> = restated
        .iter()
        .filter(|d| d["limit_id"] == "face")
        .collect();
    assert_eq!(face.len(), 1, "{face:?}");
    assert_eq!(face[0]["status"], "breach");
    let net: Vec<&&Value> = restated.iter().filter(|d| d["limit_id"] == "net").collect();
    assert_eq!(net.last().unwrap()["status"], "breach", "{net:?}");
    assert!(
        restated.iter().all(|d| d["status"] != "within"),
        "a cleared set is not watched: {restated:?}"
    );
}

/// An image stamped no earlier than any before it. WSL2's clock can step back a second or
/// more under load; ingestion refuses a quote whose times go backwards, which would value
/// that bond `unknown` and record changes this measurement does not intend.
fn steady_image(sequence: u64, price: &str) -> QuoteEvent {
    use std::sync::atomic::{AtomicI64, Ordering};
    static LAST: AtomicI64 = AtomicI64::new(i64::MIN);
    let now = chrono::Utc::now().timestamp_millis();
    let stamp = LAST.fetch_max(now, Ordering::SeqCst).max(now);
    support::fixture::event(sequence, stamp, 0, price)
}

/// Post one quote for `key`'s instrument and time it to its `valuation` frame on
/// `socket`, a session subscribed to that instrument.
async fn valuation_hop(
    server: &Running,
    socket: &mut Socket,
    key: &QuoteEvent,
    sequence: u64,
    price: &str,
) -> Duration {
    let mut event = steady_image(sequence, price);
    event.key = key.key.clone();
    let started = Instant::now();
    let posted = server.post_quotes(vec![event]).await;
    assert!(posted.status().is_success(), "{}", posted.status());
    frame(socket, "valuation").await;
    started.elapsed()
}

/// Valuation hops paced `pace` apart, prices alternating; their summary.
async fn hops(
    server: &Running,
    socket: &mut Socket,
    key: &QuoteEvent,
    sequence: &mut u64,
    pace: Duration,
) -> String {
    let mut times = Vec::new();
    for round in 0..40u64 {
        *sequence += 1;
        let price = if round % 2 == 0 { "99.30" } else { "99.25" };
        let time = valuation_hop(server, socket, key, *sequence, price).await;
        if round >= 5 {
            times.push(time);
        }
        tokio::time::sleep(pace).await;
    }
    summary(times)
}

/// The monitor's counts so far: seconds evaluating, passes, and changes recorded.
async fn monitor_counts(server: &Running) -> (f64, u64, u64) {
    let text = server
        .client
        .get(format!("{}/metrics", server.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let read = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name))
            .map(|value| value.trim().parse::<f64>().unwrap())
            .unwrap_or(0.0)
    };
    (
        read("convex_limit_monitor_seconds_sum "),
        read("convex_limit_monitor_seconds_count ") as u64,
        read("convex_limit_status_changes_total ") as u64,
    )
}

/// Wait until the monitor has finished more than `after` passes and neither its passes nor
/// its records have moved for 300 ms. A pass recording many changes is long, so passes
/// alone can look still while it runs.
async fn monitor_settled(server: &Running, after: u64) -> (f64, u64, u64) {
    tokio::time::timeout(Duration::from_secs(120), async {
        let mut last = monitor_counts(server).await;
        loop {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let now = monitor_counts(server).await;
            if now.1 > after && (now.1, now.2) == (last.1, last.2) {
                return now;
            }
            last = now;
        }
    })
    .await
    .expect("the monitor settles")
}

/// P14c's measurement, at the bounds: 64 watched books, each holding all 256 priced
/// instruments with 64 limits over every measure.
/// - The first evaluation of every set records 4,096 changes, as a restart does: one
///   commit a book since P15a (D92). The records' sizes are read once the server stops.
/// - One instrument's valuation hop: before any book is watched, while the monitor
///   records those first changes, and while it evaluates every book after each quote.
/// - The monitor's time per tick with 1, 64 and 256 instruments revalued. Every book
///   holds every instrument, so even a tick of one evaluates all 64 books. Prices move
///   within every bound, so nothing is recorded: this is evaluation time alone.
// Multi-threaded, as the server runs.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "measurement; run with --ignored --nocapture under the ci profile"]
async fn monitor_time_per_tick_at_the_bounds() {
    const ROUNDS: u64 = 20;
    let (dir, live, instruments) = live(256, 0, &MEASURES).await;
    let server = started(&dir, live.clone()).await;
    let images: Vec<QuoteEvent> =
        serde_json::from_slice(&std::fs::read(dir.path().join("images.json")).unwrap()).unwrap();
    let mut sequence = 1;

    let (mut session, _) = tokio_tungstenite::connect_async(server.ws_url())
        .await
        .unwrap();
    {
        use futures_util::SinkExt;
        frame(&mut session, "connected").await;
        let subscribe = json!({"type": "subscribe_valuations",
                               "instrument_ids": [images[0].key.instrument_id.as_str()]});
        session
            .send(tokio_tungstenite::tungstenite::Message::Text(
                subscribe.to_string().into(),
            ))
            .await
            .unwrap();
        frame(&mut session, "subscribed").await;
    }
    let pace = Duration::from_millis(50);
    let idle = hops(&server, &mut session, &images[0], &mut sequence, pace).await;

    let (_, before, recorded_before) = monitor_counts(&server).await;
    let positions = book(&instruments[..256]);
    let mut committed = Instant::now();
    for i in 0..64 {
        let id = format!("MON-{i:02}");
        put(
            &server,
            &format!("/api/v1/books/{id}"),
            &format!("book-{id}"),
            positions.clone(),
        )
        .await;
        put(
            &server,
            &format!("/api/v1/books/{id}/limits"),
            &format!("limits-{id}"),
            every_measure(),
        )
        .await;
        committed = Instant::now();
    }
    // The same hop while the monitor records those first statuses: each book's records
    // are one database commit, and the runtime's checkpoints share the database's one
    // writer.
    let recording = hops(&server, &mut session, &images[0], &mut sequence, pace).await;
    // Every limit's first status is a change from `unevaluated`.
    let behind = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let (_, _, recorded) = monitor_counts(&server).await;
            if recorded - recorded_before >= 64 * 64 {
                return committed.elapsed();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("every first status is recorded");
    let (_, mut passes, recorded) = monitor_settled(&server, before).await;
    assert_eq!(recorded - recorded_before, 64 * 64);

    let busy = hops(&server, &mut session, &images[0], &mut sequence, pace).await;
    drop(session);
    passes = monitor_settled(&server, passes).await.1;
    println!(
        "first evaluation of 64 sets x 64 limits: 4,096 changes recorded, the last {:.2} s after the last set committed",
        behind.as_secs_f64()
    );
    println!("valuation hop, 1 instrument, quotes 50 ms apart, no watched book: {idle}");
    println!("valuation hop, same, while the monitor records those 4,096 changes: {recording}");
    println!("valuation hop, same, monitor evaluating 64 books after each quote: {busy}");

    for revalued in [1usize, 64, 256] {
        let (before_sum, before_count, _) = monitor_counts(&server).await;
        for round in 0..ROUNDS {
            sequence += 1;
            let price = if round % 2 == 0 { "99.30" } else { "99.25" };
            let events: Vec<QuoteEvent> = images[..revalued]
                .iter()
                .map(|image_event| {
                    let mut event = steady_image(sequence, price);
                    event.key = image_event.key.clone();
                    event
                })
                .collect();
            let posted = server.post_quotes(events).await;
            assert!(posted.status().is_success(), "{}", posted.status());
            passes = monitor_settled(&server, passes).await.1;
        }
        let (sum, count, _) = monitor_counts(&server).await;
        let per_pass = (sum - before_sum) / (count - before_count) as f64 * 1e3;
        println!(
            "monitor, {revalued:>3} revalued, 64 books x 256 positions x 64 limits: {per_pass:.2} ms a pass, {} passes for {ROUNDS} ticks",
            count - before_count
        );
    }
    server.stop().await;
    let mut sizes: Vec<usize> = records(&live)
        .await
        .iter()
        .map(|(_, details)| details.to_string().len())
        .collect();
    sizes.sort_unstable();
    println!(
        "record details: {} records, median {} bytes, largest {} bytes",
        sizes.len(),
        sizes[sizes.len() / 2],
        sizes[sizes.len() - 1]
    );
}

// =============================================================================
// THE PRE-TRADE CHECK (P14d)
// =============================================================================

/// splitmix64: a small deterministic generator, so a failing case can be replayed.
struct Draws(u64);

impl Draws {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// A nonzero amount, a multiple of 25,000, sometimes with cents, either sign.
    fn amount(&mut self) -> rust_decimal::Decimal {
        let whole = rust_decimal::Decimal::from(25_000 * (1 + self.below(40) as i64));
        let cents = if self.below(4) == 0 {
            rust_decimal::Decimal::new(25, 2)
        } else {
            rust_decimal::Decimal::ZERO
        };
        let amount = whole + cents;
        if self.below(2) == 0 {
            -amount
        } else {
            amount
        }
    }
}

/// One position as the test's own reading of D89 keeps it.
#[derive(Clone, Debug)]
struct Held {
    id: String,
    quantity: rust_decimal::Decimal,
    pin: Option<u64>,
    rating: Option<&'static str>,
}

/// One proposed change: the instrument, the signed amount, the rating a new position takes.
type Change = (String, rust_decimal::Decimal, Option<&'static str>);

/// `held` with `changes` applied as D89 says, read independently of the server: each change
/// is added to the held position, a position a change brings to exactly zero is dropped,
/// and a new instrument follows the held positions in change order.
fn apply(held: &[Held], changes: &[Change]) -> Vec<Held> {
    let mut book: Vec<(Held, bool)> = held.iter().cloned().map(|p| (p, false)).collect();
    let mut added = Vec::new();
    for (id, amount, rating) in changes {
        match book.iter_mut().find(|(p, _)| &p.id == id) {
            Some((p, changed)) => {
                p.quantity += amount;
                *changed = true;
            }
            None => added.push(Held {
                id: id.clone(),
                quantity: *amount,
                pin: None,
                rating: *rating,
            }),
        }
    }
    book.into_iter()
        .filter(|(p, changed)| !(*changed && p.quantity.is_zero()))
        .map(|(p, _)| p)
        .chain(added.into_iter().filter(|p| !p.quantity.is_zero()))
        .collect()
}

fn snapshot(positions: &[Held]) -> Value {
    let positions: Vec<Value> = positions
        .iter()
        .map(|p| {
            let mut row = json!({"instrument_id": p.id, "quantity": p.quantity.to_string()});
            if let Some(pin) = p.pin {
                row["reference_version"] = json!(pin);
            }
            if let Some(rating) = p.rating {
                row["rating"] = json!(rating);
            }
            row
        })
        .collect();
    json!({"currency": "USD", "positions": positions})
}

/// Replace the book `id` at `version`; returns the version committed.
async fn replace_book(server: &Running, id: &str, key: &str, version: u64, body: Value) -> u64 {
    let response = server
        .client
        .put(format!("{}/api/v1/books/{id}", server.base))
        .header("idempotency-key", key)
        .header("if-match", version.to_string())
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    body["version"].as_u64().unwrap()
}

/// Map ISIN `value` to `instrument`.
async fn map_isin(server: &Running, value: &str, instrument: &str) {
    let key = format!("map-{value}");
    let (status, body) = send(
        server,
        reqwest::Method::POST,
        "/api/v1/reference/transactions",
        Some(&key),
        Some(json!({
            "idempotency_key": key,
            "changes": [],
            "master_changes": [{
                "expected_version": null,
                "definition": {"kind": "identifier", "definition": {
                    "mapping_id": format!("isin-{value}"),
                    "identifier": {"kind": "isin", "value": value},
                    "target": {"kind": "instrument", "instrument_id": instrument},
                    "effective": {"from": "2000-01-01T00:00:00Z", "until": null},
                    "provenance": {"source": "fictional-fixture", "evidence_reference": "test"}
                }}
            }]
        })),
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
}

/// D88's property: a check of changes equals the status of the book with those changes
/// committed. Each case draws up to 16 changes among adding to a held position, selling it
/// to exactly zero, flipping its sign, a new priced instrument and a new one outside the
/// plan, some named by ISIN. The held `BOND-W002` has no rating, so a check that kept it
/// at zero would leave the rating limits unknown where the committed book's are not. It checks, commits the book as `apply` reads D89, reads
/// status, compares, and restores the book. No quote is posted meanwhile, so both read the
/// same results.
#[tokio::test]
async fn a_check_equals_the_status_of_the_book_with_its_changes_committed() {
    const CASES: u64 = 64;
    const PRICED: usize = 12;
    let (dir, live, instruments) = live(PRICED, 6, &MEASURES).await;
    let server = started(&dir, live).await;
    let isins = BTreeMap::from([
        ("BOND-W002", "CHECK-W002"),
        ("BOND-W005", "CHECK-W005"),
        ("BOND-W014", "CHECK-W014"),
    ]);
    for (instrument, isin) in &isins {
        map_isin(&server, isin, instrument).await;
    }
    let original = vec![
        Held {
            id: "BOND-001".into(),
            quantity: "1000000".parse().unwrap(),
            pin: None,
            rating: Some("A"),
        },
        Held {
            id: "BOND-W001".into(),
            quantity: "-400000".parse().unwrap(),
            pin: None,
            rating: Some("BBB"),
        },
        Held {
            id: "BOND-W002".into(),
            quantity: "250000".parse().unwrap(),
            pin: Some(1),
            rating: None,
        },
        Held {
            id: "BOND-W003".into(),
            quantity: "600000".parse().unwrap(),
            pin: None,
            rating: Some("AA"),
        },
        Held {
            id: "BOND-W004".into(),
            quantity: "150000.50".parse().unwrap(),
            pin: None,
            rating: Some("A"),
        },
    ];
    put(
        &server,
        "/api/v1/books/CHECK",
        "book-check",
        snapshot(&original),
    )
    .await;
    let bounded = |id: &str, measure: Value, lower: Option<&str>, upper: &str| {
        let mut limit = json!({"limit_id": id, "measure": measure, "upper": upper,
                               "warning_fraction": "0.8"});
        if let Some(lower) = lower {
            limit["lower"] = json!(lower);
        }
        limit
    };
    // P15d: some limits hold an excess not yet expired, so both read its bounds and report it.
    let expires_at = (chrono::Utc::now() + chrono::Duration::days(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let widened = |mut limit: Value, lower: Option<&str>, upper: &str| {
        limit["excess"] = json!({"upper": upper, "expires_at": expires_at, "reason": "mandate"});
        if let Some(lower) = lower {
            limit["excess"]["lower"] = json!(lower);
        }
        limit
    };
    let limits = json!({"limits": [
        widened(bounded("net", json!({"kind": "market_value"}), Some("-500000"), "1500000"), Some("-600000"), "1600000"),
        widened(bounded("gross", json!({"kind": "gross_market_value"}), None, "3000000"), None, "3200000"),
        widened(bounded("dv01", json!({"kind": "dv01"}), Some("-1500"), "1500"), None, "1700"),
        bounded("cs01", json!({"kind": "cs01"}), Some("-1500"), "1500"),
        bounded("krd5", json!({"kind": "key_rate_dv01", "tenor": "5Y"}), Some("-800"), "800"),
        bounded("duration", json!({"kind": "modified_duration"}), Some("-10"), "10"),
        bounded("face-each", json!({"kind": "face", "instrument": "each"}), Some("-1000000"), "1200000"),
        widened(bounded("face-001", json!({"kind": "face", "instrument": {"one": "BOND-001"}}), Some("-500000"), "1500000"), Some("-750000"), "1750000"),
        bounded("issuer", json!({"kind": "group_market_value", "by": "issuer", "group": "each"}), None, "2000000"),
        bounded("rating", json!({"kind": "group_share", "by": "rating", "group": "each"}), None, "0.6"),
        bounded("sector", json!({"kind": "group_market_value", "by": "sector", "group": "each"}), None, "2500000"),
        bounded("us", json!({"kind": "group_share", "by": "country", "group": {"one": "US"}}), None, "0.9"),
    ]});
    put(
        &server,
        "/api/v1/books/CHECK/limits",
        "limits-check",
        limits,
    )
    .await;

    let mut version = 1;
    let mut statuses = BTreeMap::<String, usize>::new();
    let mut with_excess = BTreeMap::<String, usize>::new();
    for case in 0..CASES {
        let mut draws = Draws(0x5EED_0000 + case);
        // An instrument outside the plan makes every market limit unknown, which would hide
        // the rest of the comparison, so only one case in four may draw one.
        let reach = if draws.below(4) == 0 {
            instruments.len()
        } else {
            PRICED
        };
        let mut pool: Vec<&String> = instruments[..reach].iter().collect();
        let count = 1 + draws.below(16.min(reach));
        let mut changes: Vec<Change> = Vec::new();
        let mut rows = Vec::new();
        for _ in 0..count {
            let id = pool.swap_remove(draws.below(pool.len())).clone();
            let held = original.iter().find(|p| p.id == id);
            let (amount, rating) = match held {
                Some(p) => match draws.below(4) {
                    0 => (-p.quantity, None),
                    1 => (-p.quantity - p.quantity, None),
                    _ => (draws.amount(), p.rating.filter(|_| draws.below(2) == 0)),
                },
                None if draws.below(8) == 0 => (rust_decimal::Decimal::ZERO, None),
                None => {
                    let rating = RATINGS[draws.below(RATINGS.len())];
                    (draws.amount(), (draws.below(8) > 0).then_some(rating))
                }
            };
            let mut row = match isins.get(id.as_str()) {
                Some(isin) if draws.below(2) == 0 => {
                    json!({"identifier": {"kind": "isin", "value": isin}, "quantity": amount.to_string()})
                }
                _ => json!({"instrument_id": id, "quantity": amount.to_string()}),
            };
            if let Some(rating) = rating {
                row["rating"] = json!(rating);
            }
            rows.push(row);
            let new_rating = if held.is_none() { rating } else { None };
            changes.push((id, amount, new_rating));
        }
        let rows = json!(rows);
        let (code, checked) = send(
            &server,
            reqwest::Method::POST,
            "/api/v1/books/CHECK/limits/check",
            None,
            Some(json!({ "changes": rows })),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "case {case}: {rows}: {checked}");
        assert_eq!(checked["book_version"], version, "case {case}: {checked}");

        let applied = apply(&original, &changes);
        version = replace_book(
            &server,
            "CHECK",
            &format!("case-{case}-apply"),
            version,
            snapshot(&applied),
        )
        .await;
        let (code, committed) = send(
            &server,
            reqwest::Method::GET,
            "/api/v1/books/CHECK/limits/status",
            None,
            None,
        )
        .await;
        assert_eq!(code, StatusCode::OK, "case {case}: {committed}");
        assert_eq!(
            committed["book_version"], version,
            "case {case}: {committed}"
        );
        assert_eq!(
            checked["limits"], committed["limits"],
            "case {case}: rows {rows}\napplied {applied:?}"
        );
        assert_eq!(
            checked["snapshot_ids"], committed["snapshot_ids"],
            "case {case}: rows {rows}"
        );
        assert_eq!(checked["limit_set_version"], committed["limit_set_version"]);
        for limit in checked["limits"].as_array().unwrap() {
            *statuses
                .entry(limit["status"].as_str().unwrap().to_owned())
                .or_default() += 1;
            if limit["excess"].is_object() {
                *with_excess
                    .entry(limit["status"].as_str().unwrap().to_owned())
                    .or_default() += 1;
            }
        }
        version = replace_book(
            &server,
            "CHECK",
            &format!("case-{case}-restore"),
            version,
            snapshot(&original),
        )
        .await;
    }
    // The cases reach every status, so the comparison is not of one kind of answer, and
    // so do limits an excess applies to.
    for status in ["within", "warning", "breach", "unknown"] {
        assert!(
            statuses.contains_key(status),
            "no case reached {status}: {statuses:?}"
        );
        assert!(
            with_excess.contains_key(status),
            "no limit with an excess reached {status}: {with_excess:?}"
        );
    }
    server.stop().await;
}

/// P14d's measurement: `POST .../limits/check` with 256 rows into a 1,000-position book
/// (256 priced) with 64 limits over every measure, beside status for the same book.
#[tokio::test]
#[ignore = "measurement; run with --ignored --nocapture under the ci profile"]
async fn check_route_time_at_the_bounds() {
    const ROUNDS: usize = 40;
    let (dir, live, instruments) = live(256, 744, &MEASURES).await;
    let server = started(&dir, live).await;
    put(
        &server,
        "/api/v1/books/BOUND",
        "book-bound",
        book(&instruments),
    )
    .await;
    put(
        &server,
        "/api/v1/books/BOUND/limits",
        "limits-bound",
        every_measure(),
    )
    .await;
    let rows: Vec<Value> = instruments[..256]
        .iter()
        .enumerate()
        .map(|(i, id)| json!({"instrument_id": id, "quantity": if i % 2 == 0 { "25000" } else { "-25000" }}))
        .collect();
    let body = json!({ "changes": rows });
    let (mut checks, mut statuses) = (Vec::new(), Vec::new());
    for round in 0..ROUNDS + 5 {
        let a = timed(
            &server,
            reqwest::Method::POST,
            "/api/v1/books/BOUND/limits/check",
            Some(body.clone()),
        )
        .await;
        let b = timed(
            &server,
            reqwest::Method::GET,
            "/api/v1/books/BOUND/limits/status",
            None,
        )
        .await;
        if round >= 5 {
            checks.push(a);
            statuses.push(b);
        }
    }
    println!(
        "limits/check, 256 rows into 1,000 positions (256 priced), 64 limits: {}",
        summary(checks)
    );
    println!(
        "limits/status, the same book:                                 {}",
        summary(statuses)
    );
    server.stop().await;
}

/// A record as the monitor writes one: about 600 bytes of `details`, as P15a measured.
fn fixture_record(id: u64, book: &str) -> convex_engine::ports::storage::AuditEntry {
    let details = json!({
        "record_id": id,
        "book_id": book,
        "book_version": 3,
        "limit_set_version": 2,
        "evaluated_at": "2026-11-07T12:00:00.123456Z",
        "monitor_started_at": "2026-11-07T08:00:00.000001Z",
        "previous": "within",
        "measure": {"kind": "group_market_value", "by": "sector", "group": {"one": "financials"}},
        "lower": "-25000000",
        "upper": "25000000",
        "warning_fraction": "0.9",
        "limit_id": format!("sector-financials-{:02}", id % 64),
        "status": "warning",
        "value": "23456789.123456",
        "utilisation": "0.93827156",
        "snapshot_ids": [
            "live-2026-11-07T12:00:00.123456Z-000123",
            "live-2026-11-07T11:59:59.987654Z-000122",
            "live-2026-11-07T11:59:58.876543Z-000121",
        ],
    });
    convex_engine::ports::storage::AuditEntry {
        id,
        timestamp: 1_794_484_800_123,
        event_type: "limit_status".into(),
        entity_type: "book".into(),
        entity_id: book.into(),
        user: None,
        action: "warning".into(),
        details: Some(details.to_string()),
    }
}

/// P15b's measurement, with 1,000,000 records in the store (`MAX_HISTORY`), 64 books'
/// worth written as the monitor writes them, a book's 64 a commit through `append_all`:
/// - a page of 100 and of 1,000 of one book's records from the middle of its 15,616, by
///   the index (`entity_page`), against `query` with the same filter and offset;
/// - the store's reopen, with nothing to index;
/// - the route, through the real composition, for the same pages.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "measurement; run with --ignored --nocapture under the ci profile"]
async fn records_page_time_at_the_cap() {
    use convex_engine::ports::storage::{AuditFilter, Pagination};
    const ENTRIES: u64 = 1_000_000;
    let (dir, live, _) = live(1, 0, &[]).await;
    let book = "BOOK-17";
    let mut pages = Vec::new();
    {
        let storage = convex_ext_redb::open_redb_storage(&live.storage_path).unwrap();
        let audit = storage.storage.audit.clone();
        let started = Instant::now();
        let mut next = 1;
        let mut sizes = Vec::new();
        while next <= ENTRIES {
            let owner = format!("BOOK-{:02}", (next / 64) % 64);
            let batch: Vec<_> = (next..(next + 64).min(ENTRIES + 1))
                .map(|id| fixture_record(id, &owner))
                .collect();
            if sizes.is_empty() {
                sizes.push(batch[0].details.as_ref().unwrap().len());
            }
            audit.append_all(&batch).await.unwrap();
            next += batch.len() as u64;
        }
        println!(
            "fixture: {ENTRIES} records in {} commits of 64, {:.1} s; details {} bytes",
            ENTRIES.div_ceil(64),
            started.elapsed().as_secs_f64(),
            sizes[0]
        );
        assert_eq!(audit.entry_count().await.unwrap(), ENTRIES);

        let filter = AuditFilter {
            entity_type: Some("book".into()),
            entity_id: Some(book.into()),
            ..AuditFilter::default()
        };
        for size in [100usize, 1000] {
            let mut scans = Vec::new();
            let mut scanned = None;
            for _ in 0..3 {
                let start = Instant::now();
                let page = audit
                    .query(&filter, &Pagination::new(7_812, size))
                    .await
                    .unwrap();
                scans.push(start.elapsed());
                scanned = Some(page);
            }
            let scanned = scanned.unwrap();
            // Batch `b` belongs to book `b % 64`: 244 of the 15,625 are book 17's.
            let share = (0..ENTRIES / 64).filter(|b| b % 64 == 17).count() as u64 * 64;
            assert_eq!(scanned.total, share, "a book's share");
            let after = scanned.items[0].id - 1;
            let mut indexed = Vec::new();
            for _ in 0..20 {
                let start = Instant::now();
                let page = audit.entity_page("book", book, after, size).await.unwrap();
                indexed.push(start.elapsed());
                assert_eq!(
                    page.entries, scanned.items,
                    "the index reads what the scan reads"
                );
                assert!(page.more);
            }
            println!(
                "page of {size:>4}, entity_page:                            {}",
                summary(indexed)
            );
            println!(
                "page of {size:>4}, query with the same filter:             {}",
                summary(scans)
            );
            pages.push((size, after));
        }
    }
    let start = Instant::now();
    drop(convex_ext_redb::open_redb_storage(&live.storage_path).unwrap());
    println!(
        "reopen with nothing to index:                              {:.1} ms",
        start.elapsed().as_secs_f64() * 1e3
    );

    let server = started(&dir, live.clone()).await;
    put(
        &server,
        &format!("/api/v1/books/{book}"),
        "book-17",
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "1000000"},
        ]}),
    )
    .await;
    for (size, after) in pages {
        let path = format!("/api/v1/books/{book}/limits/records?after={after}&limit={size}");
        let (status, page) = send(&server, reqwest::Method::GET, &path, None, None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(page["records"].as_array().unwrap().len(), size);
        let mut times = Vec::new();
        for _ in 0..20 {
            times.push(timed(&server, reqwest::Method::GET, &path, None).await);
        }
        println!(
            "page of {size:>4}, GET .../limits/records (local development): {}",
            summary(times)
        );
    }
    server.stop().await;
}

/// P15c (D95, D97): a value hovering at a bound needs one acknowledgement. The monitor
/// rereads a book's acknowledgements when they commit, so a breach a tick causes carries
/// one made since; making it records and publishes nothing.
#[tokio::test]
async fn a_breach_a_tick_causes_carries_the_acknowledgement_made_since() {
    let (dir, live, _) = live(1, 0, &[]).await;
    let server = started(&dir, live).await;
    put(
        &server,
        "/api/v1/books/WATCHED",
        "book-watched",
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "1000000"},
        ]}),
    )
    .await;
    let mut socket = limit_session(&server, &["WATCHED"]).await;
    let first = json!({"limits": [limit("net", json!({"kind": "market_value"}))]});
    put(&server, "/api/v1/books/WATCHED/limits", "set-1", first).await;
    let change = frame(&mut socket, "limit_status").await;
    let net = number(&change["value"]);
    let upper = format!("{:.2}", net * 1.005);
    let second = json!({"limits": [
        {"limit_id": "net", "measure": {"kind": "market_value"}, "upper": upper},
    ]});
    put_over(
        &server,
        "/api/v1/books/WATCHED/limits",
        "set-2",
        "1",
        second,
    )
    .await;

    // A tick breaches; the breach is acknowledged, which publishes nothing.
    tick(&server, &dir, 2, "100.75").await;
    let breach = frame(&mut socket, "limit_status").await;
    assert_eq!(
        (&breach["previous"], &breach["status"]),
        (&json!("within"), &json!("breach")),
        "{breach}"
    );
    assert!(breach.get("acknowledgement").is_none(), "{breach}");
    let expires_at = (chrono::Utc::now() + chrono::Duration::hours(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let body = json!({"limit_id": "net", "expires_at": expires_at, "note": "hovering at the bound",
                      "record_id": breach["record_id"]});
    let (code, made) = send(
        &server,
        reqwest::Method::POST,
        "/api/v1/books/WATCHED/limits/acknowledgements",
        Some("ack-net"),
        Some(body),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{made}");

    // Back within, then a breach again: the next frame is the return, not the
    // acknowledgement, and the second breach carries it.
    tick(&server, &dir, 3, "99.25").await;
    let within = frame(&mut socket, "limit_status").await;
    assert_eq!(
        (&within["previous"], &within["status"]),
        (&json!("breach"), &json!("within")),
        "{within}"
    );
    assert!(within.get("acknowledgement").is_none(), "{within}");
    tick(&server, &dir, 4, "100.75").await;
    let again = frame(&mut socket, "limit_status").await;
    assert_eq!(
        (&again["previous"], &again["status"]),
        (&json!("within"), &json!("breach")),
        "{again}"
    );
    assert_eq!(
        (
            &again["acknowledgement"]["note"],
            &again["acknowledgement"]["record_id"],
            &again["acknowledgement"]["expires_at"]
        ),
        (
            &json!("hovering at the bound"),
            &breach["record_id"],
            &json!(expires_at)
        ),
        "{again}"
    );
    drop(socket);
    server.stop().await;
}

/// P15d (D96): approving an excess on a breached limit is a status change, recorded and
/// published with the excess as evidence. Its expiry is the change back, with no tick or
/// commit to wake the monitor: the expiry does.
#[tokio::test]
async fn an_excess_is_recorded_when_approved_and_its_expiry_wakes_the_monitor() {
    let (dir, live, _) = live(1, 0, &[]).await;
    let server = started(&dir, live.clone()).await;
    put(
        &server,
        "/api/v1/books/WATCHED",
        "book-watched",
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "1000000"},
        ]}),
    )
    .await;
    let mut socket = limit_session(&server, &["WATCHED"]).await;
    let first = json!({"limits": [limit("net", json!({"kind": "market_value"}))]});
    put(&server, "/api/v1/books/WATCHED/limits", "set-1", first).await;
    let change = frame(&mut socket, "limit_status").await;
    let net = number(&change["value"]);
    let base = format!("{:.2}", net * 0.5);
    let wider = format!("{:.2}", net * 2.0);
    let tight = |excess: Option<Value>| {
        let mut limit =
            json!({"limit_id": "net", "measure": {"kind": "market_value"}, "upper": base});
        if let Some(excess) = excess {
            limit["excess"] = excess;
        }
        json!({"limits": [limit]})
    };
    put_over(
        &server,
        "/api/v1/books/WATCHED/limits",
        "set-2",
        "1",
        tight(None),
    )
    .await;
    let breach = frame(&mut socket, "limit_status").await;
    assert_eq!(
        (&breach["previous"], &breach["status"]),
        (&json!("within"), &json!("breach")),
        "{breach}"
    );

    let expires_at = (chrono::Utc::now() + chrono::Duration::seconds(4))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let excess = json!({"upper": wider, "expires_at": expires_at, "reason": "mandate change"});
    put_over(
        &server,
        "/api/v1/books/WATCHED/limits",
        "set-3",
        "2",
        tight(Some(excess)),
    )
    .await;
    let granted = frame(&mut socket, "limit_status").await;
    assert_eq!(
        (&granted["previous"], &granted["status"]),
        (&json!("breach"), &json!("within")),
        "{granted}"
    );
    assert_eq!(
        number(&granted["upper"]),
        wider.parse::<f64>().unwrap(),
        "{granted}"
    );
    assert_eq!(
        number(&granted["excess"]["base_upper"]),
        base.parse::<f64>().unwrap()
    );
    assert_eq!(granted["excess"]["expires_at"], json!(expires_at));
    assert_eq!(granted["excess"]["reason"], "mandate change");

    // No tick and no commit from here: only the expiry can wake the monitor.
    let back = frame(&mut socket, "limit_status").await;
    assert_eq!(
        (&back["previous"], &back["status"]),
        (&json!("within"), &json!("breach")),
        "{back}"
    );
    assert_eq!(
        number(&back["upper"]),
        base.parse::<f64>().unwrap(),
        "{back}"
    );
    assert!(back.get("excess").is_none(), "{back}");
    let read: chrono::DateTime<chrono::Utc> =
        back["evaluated_at"].as_str().unwrap().parse().unwrap();
    let expiry: chrono::DateTime<chrono::Utc> = expires_at.parse().unwrap();
    assert!(
        read >= expiry,
        "evaluated {read} before the expiry {expiry}"
    );

    drop(socket);
    server.stop().await;
    let stored = records(&live).await;
    let changes: Vec<(&str, &str)> = stored
        .iter()
        .map(|(_, record)| {
            (
                record["previous"].as_str().unwrap(),
                record["status"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        changes,
        [
            ("unevaluated", "within"),
            ("within", "breach"),
            ("breach", "within"),
            ("within", "breach"),
        ]
    );
    assert!(stored[2].1["excess"].is_object(), "{}", stored[2].1);
    assert!(stored[3].1.get("excess").is_none(), "{}", stored[3].1);
}

/// Wait until status on request shows the book's limit `net` as `want`.
async fn until_status(server: &Running, book: &str, want: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (code, status) = send(
                server,
                reqwest::Method::GET,
                &format!("/api/v1/books/{book}/limits/status"),
                None,
                None,
            )
            .await;
            if code == StatusCode::OK && by_id(&status)["net"]["status"] == want {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("status never showed {want}"))
}

/// Fail on any `limit_status` frame for `wait`.
async fn quiet(socket: &mut Socket, wait: Duration) {
    use futures_util::StreamExt;
    let heard = tokio::time::timeout(wait, async {
        loop {
            match socket.next().await {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    if value["type"] == "limit_status" {
                        return value;
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended: {other:?}"),
            }
        }
    })
    .await;
    if let Ok(frame) = heard {
        panic!("recorded during the hold: {frame}");
    }
}

/// A book holding the fixture bond, watched by a session, under a limit `net` whose upper
/// bound is `upper(value)` for the value its first evaluation records.
async fn watched_net(server: &Running, upper: impl Fn(f64) -> String) -> (Socket, f64) {
    put(
        server,
        "/api/v1/books/WATCHED",
        "book-watched",
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "1000000"},
        ]}),
    )
    .await;
    let mut socket = limit_session(server, &["WATCHED"]).await;
    let first = json!({"limits": [limit("net", json!({"kind": "market_value"}))]});
    put(server, "/api/v1/books/WATCHED/limits", "set-1", first).await;
    let change = frame(&mut socket, "limit_status").await;
    let net = number(&change["value"]);
    let second = json!({"limits": [
        {"limit_id": "net", "measure": {"kind": "market_value"}, "upper": upper(net)},
    ]});
    put_over(server, "/api/v1/books/WATCHED/limits", "set-2", "1", second).await;
    (socket, net)
}

/// The recorded changes, as (previous, status).
async fn changes(live: &ServerConfig) -> Vec<(String, String)> {
    records(live)
        .await
        .into_iter()
        .map(|(_, record)| {
            (
                record["previous"].as_str().unwrap().to_owned(),
                record["status"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// P18a (D106): under a clear hold, a breach is recorded at once; a return within the hold
/// records nothing, and the hold it ended records nothing when its time comes; a clearing is
/// recorded once it has held, while status on request shows it at once.
#[tokio::test]
async fn a_clearing_is_recorded_once_it_has_held_and_a_flap_within_the_hold_records_nothing() {
    const HOLD: Duration = Duration::from_secs(3);
    let (dir, mut live, _) = live(1, 0, &[]).await;
    live.limits.clear_hold_seconds = HOLD.as_secs();
    let server = started(&dir, live.clone()).await;
    let (mut socket, _) = watched_net(&server, |net| format!("{:.2}", net * 1.005)).await;

    tick(&server, &dir, 2, "100.75").await;
    let breach = frame(&mut socket, "limit_status").await;
    assert_eq!(
        (&breach["previous"], &breach["status"]),
        (&json!("within"), &json!("breach")),
        "{breach}"
    );

    // Within, then a breach again, inside the hold.
    let flap = Instant::now();
    tick(&server, &dir, 3, "99.25").await;
    until_status(&server, "WATCHED", "within").await;
    // The monitor reads the same results; let it evaluate them.
    tokio::time::sleep(Duration::from_millis(300)).await;
    tick(&server, &dir, 4, "100.75").await;
    until_status(&server, "WATCHED", "breach").await;
    assert!(flap.elapsed() < HOLD, "the flap outlasted the hold");
    quiet(
        &mut socket,
        (flap + HOLD + Duration::from_secs(1)).saturating_duration_since(Instant::now()),
    )
    .await;

    // A clearing: status shows it at once, the stream once it has held.
    let cleared = Instant::now();
    tick(&server, &dir, 5, "99.25").await;
    until_status(&server, "WATCHED", "within").await;
    let shown = cleared.elapsed();
    let within = frame(&mut socket, "limit_status").await;
    let recorded = cleared.elapsed();
    assert_eq!(
        (&within["previous"], &within["status"]),
        (&json!("breach"), &json!("within")),
        "{within}"
    );
    assert!(shown < HOLD, "status showed the clearing after {shown:?}");
    assert!(
        recorded >= HOLD,
        "recorded after {recorded:?}, inside the {HOLD:?} hold"
    );

    drop(socket);
    server.stop().await;
    assert_eq!(
        changes(&live).await,
        [
            ("unevaluated".into(), "within".into()),
            ("within".into(), "breach".into()),
            ("breach".into(), "within".into()),
        ]
    );
}

/// P18a (D106): approving an excess on a breached limit is a clearing, recorded once it
/// has held; its expiry is a breach, recorded at once.
#[tokio::test]
async fn an_excess_approval_waits_out_the_hold_and_its_expiry_does_not() {
    const HOLD: Duration = Duration::from_secs(2);
    let (dir, mut live, _) = live(1, 0, &[]).await;
    live.limits.clear_hold_seconds = HOLD.as_secs();
    let server = started(&dir, live.clone()).await;
    let (mut socket, net) = watched_net(&server, |net| format!("{:.2}", net * 0.5)).await;
    let breach = frame(&mut socket, "limit_status").await;
    assert_eq!(
        (&breach["previous"], &breach["status"]),
        (&json!("within"), &json!("breach")),
        "{breach}"
    );

    let expires_at = (chrono::Utc::now() + chrono::Duration::seconds(6))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let excess = json!({"limits": [{
        "limit_id": "net", "measure": {"kind": "market_value"},
        "upper": format!("{:.2}", net * 0.5),
        "excess": {"upper": format!("{:.2}", net * 2.0), "expires_at": expires_at,
                   "reason": "mandate change"},
    }]});
    let approved = Instant::now();
    put_over(
        &server,
        "/api/v1/books/WATCHED/limits",
        "set-3",
        "2",
        excess,
    )
    .await;
    let granted = frame(&mut socket, "limit_status").await;
    let waited = approved.elapsed();
    assert_eq!(
        (&granted["previous"], &granted["status"]),
        (&json!("breach"), &json!("within")),
        "{granted}"
    );
    assert!(
        waited >= HOLD,
        "recorded after {waited:?}, inside the {HOLD:?} hold"
    );

    let back = frame(&mut socket, "limit_status").await;
    assert_eq!(
        (&back["previous"], &back["status"]),
        (&json!("within"), &json!("breach")),
        "{back}"
    );
    let read: chrono::DateTime<chrono::Utc> =
        back["evaluated_at"].as_str().unwrap().parse().unwrap();
    let expiry: chrono::DateTime<chrono::Utc> = expires_at.parse().unwrap();
    assert!(
        read >= expiry,
        "evaluated {read} before the expiry {expiry}"
    );
    assert!(
        (read - expiry).to_std().unwrap() < HOLD,
        "the breach at {read} waited on the hold after the expiry {expiry}"
    );

    drop(socket);
    server.stop().await;
    assert_eq!(
        changes(&live).await,
        [
            ("unevaluated".into(), "within".into()),
            ("within".into(), "breach".into()),
            ("breach".into(), "within".into()),
            ("within".into(), "breach".into()),
        ]
    );
}

/// P18a's measurement (D106): a value hovering at a bound, crossing it about three times a
/// second for 60 s, with no hold and with the default of 5 s. Each return lasts about a
/// third of a second, so no clearing outlasts the hold.
#[tokio::test]
#[ignore = "measurement; run with --ignored --nocapture under the ci profile"]
async fn records_while_a_value_hovers_at_a_bound() {
    const RUN: Duration = Duration::from_secs(60);
    const PACE: Duration = Duration::from_millis(333);
    for hold in [0, 5] {
        let (dir, mut live, _) = live(1, 0, &[]).await;
        live.limits.clear_hold_seconds = hold;
        let server = started(&dir, live.clone()).await;
        let (socket, _) = watched_net(&server, |net| format!("{:.2}", net * 1.005)).await;
        // Only the stored records are read.
        drop(socket);
        let images: Vec<QuoteEvent> =
            serde_json::from_slice(&std::fs::read(dir.path().join("images.json")).unwrap())
                .unwrap();
        let run = Instant::now();
        let (mut ticks, mut refused) = (0u64, 0u64);
        while run.elapsed() < RUN {
            ticks += 1;
            let price = if ticks % 2 == 1 { "100.75" } else { "99.25" };
            let mut event = steady_image(ticks + 1, price);
            event.key = images[0].key.clone();
            // A WSL2 clock that steps back leaves these stamps ahead of the server's clock
            // for a moment; such a quote is refused, and counted rather than fatal.
            if !server.post_quotes(vec![event]).await.status().is_success() {
                refused += 1;
            }
            tokio::time::sleep(PACE).await;
        }
        server.stop().await;
        let changes = changes(&live).await;
        let breaches = changes.iter().filter(|(_, s)| s == "breach").count();
        println!(
            "hold {hold} s: {ticks} ticks ({refused} refused) in {:.1} s, {} records ({breaches} breaches), first {:?}",
            run.elapsed().as_secs_f64(),
            changes.len(),
            &changes[..changes.len().min(4)]
        );
    }
}
