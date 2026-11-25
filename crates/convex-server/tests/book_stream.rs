//! The `book` topic over Live results: the `live` profile over the real composition, fed
//! by posted quote images. A watched book's summary is what limit status, analytics by book
//! and P&L since the close give over the same results; it follows a price; a burst of
//! ticks sends at most four a second and always its last; a book watched only by its close
//! has a summary, one watched by neither has none, and a retired one none until revived.
#[path = "support/live_server.rs"]
mod live_server;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use convex_engine::ports::quote_event::QuoteKey;
use live_server::{images, next_frame, subscribe_books, Running, Socket};
use reqwest::{Method, StatusCode};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;
use std::time::{Duration, Instant};

/// What the runtime computes: every measure a summary reads.
const MEASURES: [&str; 11] = [
    "clean_price_mid",
    "dirty_price_mid",
    "accrued_interest",
    "ytm_mid",
    "modified_duration_mid",
    "convexity_mid",
    "dv01_mid",
    "z_spread_mid",
    "cs01_mid",
    "krd_2y_mid",
    "krd_5y_mid",
];

/// A Live server valuing `BOND-001` and `BOND-W001`, each quoted at 99.25.
async fn started() -> (tempfile::TempDir, Vec<QuoteKey>, Running) {
    let (dir, _, config, keys) = live_server::wide_live_setup_measuring(2, &MEASURES).await;
    let server = Running::start(config).await;
    server.post_all_quotes(&images(&keys, 1, "99.25")).await;
    server.wait_ready().await;
    (dir, keys, server)
}

async fn get(server: &Running, path: &str) -> Value {
    let (status, reply) = server.send(Method::GET, path, None).await;
    assert_eq!(status, StatusCode::OK, "{path}: {reply}");
    reply
}

/// A long in `BOND-001` and a short in `BOND-W001`.
fn positions() -> Value {
    json!({"currency": "USD", "positions": [
        {"instrument_id": "BOND-001", "quantity": "1000000"},
        {"instrument_id": "BOND-W001", "quantity": "-500000"},
    ]})
}

/// A limit on each market measure, wide enough to hold.
fn every_measure() -> Value {
    let wide = |id: &str, measure: Value| json!({"limit_id": id, "measure": measure, "lower": "-100000000", "upper": "100000000"});
    json!({"limits": [
        wide("mv", json!({"kind": "market_value"})),
        wide("gross", json!({"kind": "gross_market_value"})),
        wide("dv01", json!({"kind": "dv01"})),
        wide("cs01", json!({"kind": "cs01"})),
        wide("duration", json!({"kind": "modified_duration"})),
        wide("krd-5y", json!({"kind": "key_rate_dv01", "tenor": "5Y"})),
    ]})
}

/// Close `book` once the runtime values a position of it.
async fn close(server: &Running, book: &str) {
    let path = format!("/api/v1/books/{book}/close");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (status, reply) = server
                .send_with(Method::POST, &path, &format!("close-{book}"), None, None)
                .await;
            if status == StatusCode::CREATED {
                return;
            }
            assert_eq!(reply["code"], "BOOK_CLOSE_UNVALUED", "{status} {reply}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the runtime values the book");
}

/// Post `price` for `BOND-001` at `sequence`.
async fn quote(server: &Running, keys: &[QuoteKey], sequence: u64, price: &str) {
    server
        .post_all_quotes(&images(&keys[..1], sequence, price))
        .await;
}

/// `book`'s summary in `seed`, or else the next sent.
async fn first_summary(socket: &mut Socket, seed: &[Value], book: &str) -> Value {
    match seed.iter().find(|frame| frame["book_id"] == book) {
        Some(frame) => frame.clone(),
        None => summary_after(socket, book, 0).await,
    }
}

/// The next summary of `book` later than `after`, skipping any other frame.
async fn summary_after(socket: &mut Socket, book: &str, after: u64) -> Value {
    loop {
        let frame = next_frame(socket).await;
        if frame["type"] == "book_summary"
            && frame["book_id"] == book
            && frame["sequence"].as_u64().unwrap() > after
        {
            return frame;
        }
    }
}

fn decimal(value: &Value) -> Decimal {
    Decimal::from_str(value.as_str().unwrap_or_else(|| panic!("{value}"))).unwrap()
}

/// Each risk measure is the value a limit on it reports, or both are unknown.
fn assert_measure(summary: &Value, measure: &Value, limit: &Value) {
    if limit["status"] == "unknown" {
        assert_eq!(measure["status"], "unknown", "{summary}");
    } else {
        assert_eq!(measure["status"], "known", "{summary}");
        assert_eq!(
            decimal(&measure["value"]),
            decimal(&limit["value"]),
            "{summary}"
        );
    }
}

/// A summary is what the status route, analytics by book and P&L give over the same
/// results, and it follows a price.
#[tokio::test]
async fn a_summary_is_what_status_analytics_and_pnl_give_and_follows_a_price() {
    let (_dir, keys, server) = started().await;
    server.put("/api/v1/books/DESK", &positions()).await;
    server
        .put("/api/v1/books/DESK/limits", &every_measure())
        .await;
    close(&server, "DESK").await;
    let mut socket = server.session(None).await;
    let seed = subscribe_books(&mut socket, &["DESK"]).await;
    assert!(
        seed.iter().all(|frame| frame["type"] == "book_summary"),
        "{seed:?}"
    );
    let mut summary = first_summary(&mut socket, &seed, "DESK").await;
    // The monitor rereads the book once its close commits.
    while summary["pnl"]["status"] != "measured" {
        let sequence = summary["sequence"].as_u64().unwrap();
        summary = summary_after(&mut socket, "DESK", sequence).await;
    }
    // The summary of the close's own results: no movement since.
    for moved in [false, true] {
        let status = get(&server, "/api/v1/books/DESK/limits/status").await;
        let pnl = get(&server, "/api/v1/books/DESK/pnl").await;
        let (code, analytics) = server
            .send(
                Method::POST,
                "/api/v1/portfolio/analytics",
                Some(&json!({"book_id": "DESK"})),
            )
            .await;
        assert_eq!(code, StatusCode::OK, "{analytics}");
        let limit = |id: &str| {
            status["limits"]
                .as_array()
                .unwrap()
                .iter()
                .find(|limit| limit["limit_id"] == id)
                .unwrap()
                .clone()
        };
        let risk = &summary["risk"];
        assert_eq!(risk["status"], "measured", "{summary}");
        assert_eq!(summary["currency"], "USD", "{summary}");
        assert_measure(&summary, &risk["market_value"], &limit("mv"));
        assert_measure(&summary, &risk["gross_market_value"], &limit("gross"));
        assert_measure(&summary, &risk["dv01"], &limit("dv01"));
        assert_measure(&summary, &risk["cs01"], &limit("cs01"));
        assert_measure(&summary, &risk["modified_duration"], &limit("duration"));
        // The tenors the runtime computes.
        let tenors: Vec<&Value> = risk["key_rate_dv01"]
            .as_array()
            .unwrap()
            .iter()
            .map(|krd| &krd["tenor"])
            .collect();
        assert_eq!(tenors, [&json!("2Y"), &json!("5Y")], "{summary}");
        let krd_5y = &risk["key_rate_dv01"][1];
        assert_measure(&summary, krd_5y, &limit("krd-5y"));
        assert_eq!(
            decimal(&risk["market_value"]["value"]),
            decimal(&analytics["market_value"]),
            "{analytics}"
        );
        assert_eq!(
            decimal(&risk["dv01"]["value"]),
            decimal(&analytics["dv01"]),
            "{analytics}"
        );
        // Each limit's status, value and utilisation, as the status route gives them.
        let limits: Vec<Value> = status["limits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|limit| {
                let mut now = json!({"limit_id": limit["limit_id"], "status": limit["status"]});
                for field in ["value", "utilisation"] {
                    if !limit[field].is_null() {
                        now[field] = limit[field].clone();
                    }
                }
                now
            })
            .collect();
        assert_eq!(summary["limits"], json!(limits));
        assert_eq!(summary["limit_set_version"], status["limit_set_version"]);
        // P&L since the close, as the route totals it.
        assert_eq!(summary["pnl"]["status"], "measured", "{summary}");
        assert_eq!(decimal(&summary["pnl"]["pnl"]), decimal(&pnl["pnl"]));
        assert_eq!(
            decimal(&summary["pnl"]["entitled_cash"]),
            decimal(&pnl["entitled_cash"])
        );
        assert_eq!(summary["pnl"]["excluded_count"], pnl["excluded_count"]);
        assert_eq!(summary["pnl"]["close_version"], pnl["close"]["version"]);
        assert_eq!(decimal(&summary["pnl"]["pnl"]).is_zero(), !moved);
        if !moved {
            quote(&server, &keys, 2, "99.75").await;
            let before = decimal(&summary["risk"]["market_value"]["value"]);
            loop {
                let sequence = summary["sequence"].as_u64().unwrap();
                summary = summary_after(&mut socket, "DESK", sequence).await;
                if decimal(&summary["risk"]["market_value"]["value"]) != before {
                    break;
                }
            }
        }
    }
    server.stop().await;
}

/// A book watched only by its close has a summary; one watched only by limits has one
/// without P&L; one watched by neither has none. A burst of ticks sends at most four
/// summaries a second a book and always its last; a retired book has none until revived.
#[tokio::test]
async fn summaries_coalesce_and_follow_what_is_watched() {
    let (_dir, keys, server) = started().await;
    for book in ["CLOSED", "LIMITED", "PLAIN"] {
        server
            .put(&format!("/api/v1/books/{book}"), &positions())
            .await;
    }
    server
        .put("/api/v1/books/LIMITED/limits", &every_measure())
        .await;
    close(&server, "CLOSED").await;
    let mut socket = server.session(None).await;
    let seed = subscribe_books(&mut socket, &["CLOSED", "LIMITED", "PLAIN"]).await;
    assert!(
        seed.iter().all(|frame| frame["book_id"] != "PLAIN"),
        "{seed:?}"
    );
    let limited = first_summary(&mut socket, &seed, "LIMITED").await;
    assert_eq!(
        limited["pnl"],
        json!({"status": "unknown", "reason": "no_close"})
    );
    let closed = first_summary(&mut socket, &seed, "CLOSED").await;
    assert!(closed.get("limit_set_version").is_none(), "{closed}");
    assert_eq!(closed["limits"], json!([]));

    // Twenty ticks 25 ms apart.
    let started_at = Instant::now();
    // Each at its own price, so only the last tick's summary has the last P&L.
    for i in 0..20 {
        let price = format!("99.{}", 300 + 5 * i);
        quote(&server, &keys, 3 + i, &price).await;
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let mut summaries = Vec::new();
    let quiet = Instant::now() + Duration::from_secs(2);
    while let Ok(frame) = tokio::time::timeout_at(quiet.into(), next_frame(&mut socket)).await {
        assert_ne!(frame["book_id"], "PLAIN", "{frame}");
        if frame["type"] == "book_summary" && frame["book_id"] == "CLOSED" {
            summaries.push(frame);
        }
    }
    let elapsed = started_at.elapsed().as_millis();
    let most = elapsed / 250 + 2;
    assert!(
        summaries.len() as u128 <= most && summaries.len() >= 2,
        "{} summaries in {elapsed} ms",
        summaries.len()
    );
    let sequences: Vec<u64> = summaries
        .iter()
        .map(|summary| summary["sequence"].as_u64().unwrap())
        .collect();
    assert!(
        sequences.windows(2).all(|pair| pair[0] < pair[1]),
        "{sequences:?}"
    );
    // The last of the burst was sent: the last summary is the P&L now.
    let last = summaries.last().unwrap();
    let pnl = get(&server, "/api/v1/books/CLOSED/pnl").await;
    assert_eq!(decimal(&last["pnl"]["pnl"]), decimal(&pnl["pnl"]), "{last}");

    // Retired, the book has no summary to start from; revived, it has one again.
    let book = get(&server, "/api/v1/books/CLOSED").await;
    let version = book["version"].as_u64().unwrap();
    let (status, reply) = server
        .send_with(
            Method::POST,
            "/api/v1/books/CLOSED/retire",
            "retire-closed",
            Some(version),
            None,
        )
        .await;
    assert!(status.is_success(), "{status} {reply}");
    let mut fresh = server.session(None).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let seed = subscribe_books(&mut fresh, &["CLOSED"]).await;
        if seed.is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "a retired book's summary is still kept"
        );
        send_unsubscribe(&mut fresh, "CLOSED").await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (status, reply) = server
        .send_with(
            Method::PUT,
            "/api/v1/books/CLOSED",
            "revive-closed",
            Some(version + 1),
            Some(&positions()),
        )
        .await;
    assert!(status.is_success(), "{status} {reply}");
    // A tick revalues every book; the session follows CLOSED alone.
    quote(&server, &keys, 30, "99.125").await;
    let revived = loop {
        let frame = next_frame(&mut fresh).await;
        if frame["type"] == "book_summary" {
            break frame;
        }
    };
    assert_eq!(revived["book_id"], "CLOSED", "{revived}");
    assert_eq!(revived["pnl"]["status"], "measured", "{revived}");
    let quiet = Instant::now() + Duration::from_millis(600);
    while let Ok(frame) = tokio::time::timeout_at(quiet.into(), next_frame(&mut fresh)).await {
        assert!(
            frame["book_id"].is_null() || frame["book_id"] == "CLOSED",
            "{frame}"
        );
    }
    server.stop().await;
}

async fn send_unsubscribe(socket: &mut Socket, book: &str) {
    live_server::send_frame(
        socket,
        json!({"type": "unsubscribe_books", "book_ids": [book]}),
    )
    .await;
    assert_eq!(next_frame(socket).await["type"], "unsubscribed");
}
