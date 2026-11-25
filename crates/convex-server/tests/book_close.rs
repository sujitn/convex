//! Desk closes and P&L since the close, over Live results: the `live` profile over the
//! real composition, fed by posted quote images. A close records each position's clean
//! price and accrued as the runtime's results hold them, at most once a UTC date; P&L since
//! it is zero when nothing moves, follows a price, and carries a coupon paid between the
//! close and a later settlement date as cash, not a loss. At most 64 books are watched.
//! `close_and_pnl_time_at_the_bounds` measures a close and P&L at a full book
//! (`--ignored --nocapture`).
#[path = "support/live_server.rs"]
mod live_server;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use chrono::{Datelike, Weekday};
use convex_engine::ports::quote_event::QuoteKey;
use convex_server::ServerConfig;
use live_server::{images, Running};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

/// A Live server valuing `width` bonds (`BOND-001`, then `BOND-W001`...), each quoted at
/// `price`.
async fn started(
    width: usize,
    price: &str,
) -> (tempfile::TempDir, ServerConfig, Vec<QuoteKey>, Running) {
    let (dir, _, config, keys) = live_server::wide_live_setup(width).await;
    let server = quoted(&config, &keys, 1, price).await;
    (dir, config, keys, server)
}

/// Start a server on `config` and post `price` for every key, then wait for readiness.
async fn quoted(config: &ServerConfig, keys: &[QuoteKey], sequence: u64, price: &str) -> Running {
    let server = Running::start(config.clone()).await;
    server.post_all_quotes(&images(keys, sequence, price)).await;
    server.wait_ready().await;
    server
}

async fn send(
    server: &Running,
    method: Method,
    path: &str,
    key: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    send_over(server, method, path, key, None, body).await
}

/// [`send`] over `version`, in `If-Match`.
async fn send_over(
    server: &Running,
    method: Method,
    path: &str,
    key: Option<&str>,
    version: Option<u64>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = server
        .client
        .request(method, format!("{}{path}", server.base));
    if let Some(key) = key {
        request = request.header("idempotency-key", key);
    }
    if let Some(version) = version {
        request = request.header("if-match", format!("\"{version}\""));
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn put_book(server: &Running, book: &str, key: &str, body: Value) {
    put_book_over(server, book, key, None, body).await;
}

/// A snapshot of `book` over its `version`.
async fn put_book_over(server: &Running, book: &str, key: &str, version: Option<u64>, body: Value) {
    let path = format!("/api/v1/books/{book}");
    let (status, reply) =
        send_over(server, Method::PUT, &path, Some(key), version, Some(body)).await;
    assert!(status.is_success(), "{path}: {status} {reply}");
}

/// Retire `book`, at its `version`.
async fn retire(server: &Running, book: &str, version: u64) {
    let path = format!("/api/v1/books/{book}/retire");
    let key = format!("retire-{book}-{version}");
    let (status, reply) =
        send_over(server, Method::POST, &path, Some(&key), Some(version), None).await;
    assert!(status.is_success(), "{path}: {status} {reply}");
}

async fn close(server: &Running, book: &str, key: &str) -> (StatusCode, Value) {
    send(
        server,
        Method::POST,
        &format!("/api/v1/books/{book}/close"),
        Some(key),
        None,
    )
    .await
}

async fn get(server: &Running, path: &str) -> (StatusCode, Value) {
    send(server, Method::GET, path, None, None).await
}

async fn pnl(server: &Running, book: &str) -> Value {
    let (status, reply) = get(server, &format!("/api/v1/books/{book}/pnl")).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    reply
}

fn number(value: &Value) -> f64 {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or_else(|| panic!("not a number: {value}"))
}

/// Each instrument's Live clean price mid and accrued per 100, and its record's revision.
async fn live_prices(server: &Running) -> BTreeMap<String, (f64, f64, u64)> {
    let (_, page) = get(server, "/api/v1/runtime/results").await;
    page["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|record| {
            let results = &record["result"]["results"];
            Some((
                record["result"]["provenance"]["subject"]["instrument_id"]
                    .as_str()?
                    .to_owned(),
                (
                    number(&results["clean_price_mid"]["value"]["value"]),
                    number(&results["accrued_interest"]["value"]["value"]),
                    record["revision"].as_u64()?,
                ),
            ))
        })
        .collect()
}

fn by_instrument(reply: &Value) -> BTreeMap<String, Value> {
    reply["positions"]
        .as_array()
        .unwrap_or_else(|| panic!("no positions: {reply}"))
        .iter()
        .map(|p| (p["instrument_id"].as_str().unwrap().to_owned(), p.clone()))
        .collect()
}

fn assert_near(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= 1e-6 * expected.abs().max(1.0),
        "{what}: {actual} vs {expected}"
    );
}

#[tokio::test]
async fn a_close_records_each_position_once_a_date() {
    let (_dir, _, _, server) = started(3, "99.25").await;
    put_book(
        &server,
        "DESK",
        "book-1",
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "1000000"},
            {"instrument_id": "BOND-W001", "quantity": "-400000"},
            {"instrument_id": "BOND-W002", "quantity": "25", "unit": "units"},
        ]}),
    )
    .await;
    let prices = live_prices(&server).await;

    let (status, taken) = close(&server, "DESK", "close-1").await;
    assert_eq!(status, StatusCode::CREATED, "{taken}");
    assert_eq!(taken["version"], 1);
    assert_eq!(taken["basis"], "desk_close");
    assert_eq!(
        taken["taken_by"],
        json!({"kind": "desk", "subject": "local-development"})
    );
    assert_eq!(
        taken["close_date"],
        chrono::Utc::now().date_naive().to_string()
    );
    assert_eq!(taken["settlement_date"], "2026-10-06");
    assert_eq!(taken["book_version"], 1);
    assert!(
        !taken["snapshot_ids"].as_array().unwrap().is_empty(),
        "{taken}"
    );
    let positions = by_instrument(&taken);
    for id in ["BOND-001", "BOND-W001"] {
        let value = &positions[id]["value"];
        assert_eq!(value["status"], "valued", "{value}");
        assert_eq!(value["instrument_version"], 1);
        let (clean, accrued, _) = prices[id];
        assert_eq!(number(&value["clean_price_mid"]), clean, "{id}");
        assert_eq!(number(&value["accrued_interest"]), accrued, "{id}");
    }
    assert_eq!(
        positions["BOND-W002"]["value"],
        json!({"status": "unvalued", "reason": {"code": "unsupported_quantity_unit"}})
    );

    // An exact retry returns the close it wrote; another close the same date is refused.
    let (status, retried) = close(&server, "DESK", "close-1").await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(retried, taken);
    let (status, again) = close(&server, "DESK", "close-2").await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    assert_eq!(again["code"], "BOOK_ALREADY_CLOSED");

    let (status, latest) = get(&server, "/api/v1/books/DESK/close").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(latest, taken);
    let (status, first) = get(&server, "/api/v1/books/DESK/close/versions/1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first, taken);
    // A close is read only through its book's routes.
    let (status, receipt) = get(&server, "/api/v1/reference/commits/close-1").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{receipt}");
    assert_eq!(receipt["code"], "BOOK_CLOSE_ROUTE_REQUIRED");
    let (status, _) = get(&server, "/api/v1/reference/masters/book_close").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Nor written elsewhere: a transaction naming a close and a book is refused for the
    // close.
    let book = json!({"kind": "book", "definition": {
        "book_id": "GENERIC",
        "owning_system": "elsewhere",
        "effective": {"from": "2020-01-01T00:00:00Z", "until": null},
        "positions": [],
        "provenance": {"source": "elsewhere", "evidence_reference": "elsewhere"},
    }});
    let close_record = json!({"kind": "book_close", "definition": {
        "book_id": "DESK",
        "close_date": "2026-10-05",
        "basis": "desk_close",
        "taken_by": {"kind": "schedule"},
        "book_version": 1,
        "currency": "USD",
        "settlement_date": "2026-10-06",
        "snapshot_ids": ["snapshot-1"],
        "positions": [{"instrument_id": "BOND-001", "quantity": "1000000", "unit": "face",
            "value": {"status": "valued", "instrument_version": 1,
                      "clean_price_mid": "99.25", "accrued_interest": "1.25"}}],
        "effective": {"from": "2026-10-05T21:30:00Z", "until": null},
        "provenance": {"source": "elsewhere", "evidence_reference": "elsewhere"},
    }});
    let command = json!({
        "idempotency_key": "generic-close",
        "changes": [],
        "master_changes": [{"definition": book}, {"definition": close_record}],
    });
    let (status, refused) = send(
        &server,
        Method::POST,
        "/api/v1/reference/transactions",
        Some("generic-close"),
        Some(command),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(refused["code"], "BOOK_CLOSE_ROUTE_REQUIRED");
    server.stop().await;
}

#[tokio::test]
async fn pnl_is_zero_when_nothing_moves_and_follows_a_price() {
    let (_dir, _, keys, server) = started(3, "99.25").await;
    put_book(
        &server,
        "DESK",
        "book-1",
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "1000000"},
            {"instrument_id": "BOND-W001", "quantity": "-400000"},
            {"instrument_id": "BOND-W002", "quantity": "25", "unit": "units"},
        ]}),
    )
    .await;
    let (status, taken) = close(&server, "DESK", "close-1").await;
    assert_eq!(status, StatusCode::CREATED, "{taken}");

    // The accrued carries an f64's digits; the close and now round it the same way.
    let still = pnl(&server, "DESK").await;
    assert_eq!(still["pnl"], "0", "{still}");
    assert_eq!(still["entitled_cash"], "0");
    assert_eq!(still["excluded_count"], 1);
    assert_eq!(still["close"]["version"], 1);
    assert_eq!(still["close"]["settlement_date"], still["settlement_date"]);
    let positions = by_instrument(&still);
    assert_eq!(positions["BOND-001"]["held_face"], "1000000");
    assert_eq!(positions["BOND-001"]["pnl"], "0");
    assert_eq!(positions["BOND-W001"]["held_face"], "-400000");
    assert_eq!(positions["BOND-W002"]["no_pnl"], "unvalued_at_close");

    // Grown, removed and added: P&L on the face held through, and none on the rest.
    put_book_over(
        &server,
        "DESK",
        "book-2",
        Some(1),
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "1500000"},
            {"instrument_id": "BOND-W002", "quantity": "25", "unit": "units"},
            {"instrument_id": "BOND-W001", "quantity": "0"},
        ]}),
    )
    .await;
    put_book_over(
        &server,
        "DESK",
        "book-3",
        Some(2),
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "1500000"},
            {"instrument_id": "BOND-W002", "quantity": "100000"},
        ]}),
    )
    .await;
    let before = live_prices(&server).await;
    server
        .post_all_quotes(&images(&keys[..1], 2, "100.25"))
        .await;
    let after = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let prices = live_prices(&server).await;
            if prices["BOND-001"].2 > before["BOND-001"].2 {
                return prices;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let moved = pnl(&server, "DESK").await;
    let positions = by_instrument(&moved);
    let close_dirty = number(&positions["BOND-001"]["close_dirty_price"]);
    let (clean, accrued, _) = after["BOND-001"];
    assert_near(
        number(&positions["BOND-001"]["dirty_price"]),
        clean + accrued,
        "dirty now",
    );
    assert_eq!(positions["BOND-001"]["held_face"], "1000000");
    let expected = 1_000_000.0 * (clean + accrued - close_dirty) / 100.0;
    assert_near(
        number(&positions["BOND-001"]["pnl"]),
        expected,
        "BOND-001 P&L",
    );
    assert_near(expected, 10_000.0, "a point on 1,000,000");
    assert_eq!(positions["BOND-W001"]["no_pnl"], "removed_since_close");
    assert_eq!(positions["BOND-W002"]["no_pnl"], "unvalued_at_close");
    assert_near(number(&moved["pnl"]), expected, "book P&L");
    assert_eq!(moved["excluded_count"], 2);
    assert_eq!(moved["book_version"], 3);
    server.stop().await;
}

#[tokio::test]
async fn a_close_and_pnl_refuse_what_they_cannot_value() {
    let (_dir, _, _, server) = started(1, "99.25").await;
    put_book(
        &server,
        "NO-CCY",
        "b-1",
        json!({"positions": [{"instrument_id": "BOND-001", "quantity": "100"}]}),
    )
    .await;
    let (status, reply) = close(&server, "NO-CCY", "c-1").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    assert_eq!(reply["code"], "BOOK_CURRENCY_REQUIRED");

    put_book(
        &server,
        "UNITS",
        "b-2",
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "100", "unit": "units"},
        ]}),
    )
    .await;
    let (status, reply) = close(&server, "UNITS", "c-2").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    assert_eq!(reply["code"], "BOOK_CLOSE_UNVALUED");

    let (status, reply) = get(&server, "/api/v1/books/UNITS/pnl").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{reply}");
    assert_eq!(reply["code"], "BOOK_CLOSE_NOT_FOUND");
    let (status, _) = close(&server, "MISSING", "c-3").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &server,
        Method::POST,
        "/api/v1/books/MISSING/close",
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an idempotency key is required"
    );

    put_book(
        &server,
        "GONE",
        "b-3",
        json!({"currency": "USD", "positions": [{"instrument_id": "BOND-001", "quantity": "100"}]}),
    )
    .await;
    let (status, reply) = close(&server, "GONE", "c-4").await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    retire(&server, "GONE", 1).await;
    let (status, reply) = get(&server, "/api/v1/books/GONE/pnl").await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "BOOK_RETIRED");
    server.stop().await;
}

/// The schedule closes each watched book at its time, Monday to Friday: a book watched by
/// its limits is closed by the schedule, one the desk closed today keeps the desk's close,
/// and one not watched is not closed. The composed server reads the wall clock, so the
/// close is set for 15 seconds after the test starts. On a weekend, or when setting the books
/// up outlasts that, nothing can be checked and the test says so.
#[tokio::test]
async fn the_schedule_closes_each_watched_book_at_its_time() {
    let due = chrono::Utc::now() + chrono::TimeDelta::seconds(15);
    if matches!(due.weekday(), Weekday::Sat | Weekday::Sun) {
        eprintln!("no close is due on a weekend");
        return;
    }
    let (_dir, _, mut config, keys) = live_server::wide_live_setup(1).await;
    config.books.close_time_utc = Some(due.format("%H:%M:%S").to_string());
    let server = quoted(&config, &keys, 1, "99.25").await;
    let held = json!({"currency": "USD", "positions": [
        {"instrument_id": "BOND-001", "quantity": "100"},
    ]});
    for book in ["DESK", "LIMITED", "UNWATCHED"] {
        put_book(&server, book, &format!("b-{book}"), held.clone()).await;
    }
    let (status, reply) = send(
        &server,
        Method::PUT,
        "/api/v1/books/LIMITED/limits",
        Some("limits-limited"),
        Some(json!({"limits": [{"limit_id": "net", "measure": {"kind": "market_value"}, "upper": "1"}]})),
    )
    .await;
    assert!(status.is_success(), "{reply}");
    let (status, desk) = close(&server, "DESK", "desk-close").await;
    assert_eq!(status, StatusCode::CREATED, "{desk}");
    if chrono::Utc::now() + chrono::TimeDelta::seconds(1) >= due {
        eprintln!("setting the books up outlasted the schedule's time; nothing to check");
        server.stop().await;
        return;
    }

    // The schedule takes the books in order, so DESK is done once LIMITED is closed.
    let scheduled = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let (status, reply) = get(&server, "/api/v1/books/LIMITED/close").await;
            if status == StatusCode::OK {
                return reply;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(scheduled["taken_by"], json!({"kind": "schedule"}));
    assert_eq!(scheduled["close_date"], due.date_naive().to_string());
    let (status, latest) = get(&server, "/api/v1/books/DESK/close").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(latest, desk);
    let (status, _) = get(&server, "/api/v1/books/UNWATCHED/close").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    server.stop().await;
}

#[tokio::test]
async fn at_most_64_books_are_watched_and_retiring_one_frees_its_place() {
    let (_dir, _, _, server) = started(1, "99.25").await;
    let held = json!({"currency": "USD", "positions": [
        {"instrument_id": "BOND-001", "quantity": "100"},
    ]});
    // One watched by its limits, the rest by their closes.
    put_book(&server, "B-00", "b-00", held.clone()).await;
    let (status, reply) = send(
        &server,
        Method::PUT,
        "/api/v1/books/B-00/limits",
        Some("limits-00"),
        Some(json!({"limits": [{"limit_id": "net", "measure": {"kind": "market_value"}, "upper": "1"}]})),
    )
    .await;
    assert!(status.is_success(), "{reply}");
    for index in 1..64 {
        let book = format!("B-{index:02}");
        put_book(&server, &book, &format!("b-{index:02}"), held.clone()).await;
        let (status, reply) = close(&server, &book, &format!("c-{index:02}")).await;
        assert_eq!(status, StatusCode::CREATED, "{book}: {reply}");
    }
    put_book(&server, "B-64", "b-64", held.clone()).await;
    let (status, reply) = close(&server, "B-64", "c-64").await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "WATCHED_BOOKS_EXCEEDED");
    let (status, reply) = send(
        &server,
        Method::PUT,
        "/api/v1/books/B-64/limits",
        Some("limits-64"),
        Some(json!({"limits": [{"limit_id": "net", "measure": {"kind": "market_value"}, "upper": "1"}]})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "WATCHED_BOOKS_EXCEEDED");
    // A closed book already watched takes limits without a new place.
    let (status, reply) = send(
        &server,
        Method::PUT,
        "/api/v1/books/B-01/limits",
        Some("limits-01"),
        Some(json!({"limits": [{"limit_id": "net", "measure": {"kind": "market_value"}, "upper": "1"}]})),
    )
    .await;
    assert!(status.is_success(), "{reply}");

    retire(&server, "B-02", 1).await;
    let (status, reply) = close(&server, "B-64", "c-64b").await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    // Reviving a closed book watches it again, so it needs a place too.
    let (status, reply) = send_over(
        &server,
        Method::PUT,
        "/api/v1/books/B-02",
        Some("b-02-revived"),
        Some(2),
        Some(held.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "WATCHED_BOOKS_EXCEEDED");
    retire(&server, "B-03", 1).await;
    put_book_over(&server, "B-02", "b-02-revived-2", Some(2), held.clone()).await;
    let (status, reply) = get(&server, "/api/v1/books/B-02/pnl").await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    // A retry of the revival gets its receipt, though the book has since been retired and
    // its place taken.
    retire(&server, "B-02", 3).await;
    put_book(&server, "B-65", "b-65", held.clone()).await;
    let (status, reply) = close(&server, "B-65", "c-65").await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    put_book_over(&server, "B-02", "b-02-revived-2", Some(2), held.clone()).await;
    // A revival names the version it replaces.
    let (status, reply) = send_over(
        &server,
        Method::PUT,
        "/api/v1/books/B-02",
        Some("b-02-revived-3"),
        Some(5),
        Some(held),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "VERSION_CONFLICT");
    server.stop().await;
}

/// Rewrite the plan beside `config`'s database to settle at `date`: a new runtime, as a
/// change of settlement date needs.
fn settle_at(config: &ServerConfig, date: &str) {
    let path = Path::new(config.runtime_plan.as_deref().unwrap());
    let mut plan: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    plan["settlement_date"] = json!(date);
    plan["runtime_id"] = json!(format!("{}-{date}", plan["runtime_id"].as_str().unwrap()));
    std::fs::write(path, serde_json::to_vec(&plan).unwrap()).unwrap();
}

#[tokio::test]
async fn a_coupon_paid_between_the_close_and_a_later_settlement_is_cash_not_a_loss() {
    // BOND-001 pays 5.375% on 15 January and 15 July, ACT/ACT: a holder settling on
    // 6 October 2026 is owed the 15 January 2027 coupon, on the period's 184 days of a
    // 365-day year, and one settling on 19 January 2027 is not.
    let (_dir, config, keys, server) = started(2, "99.25").await;
    put_book(
        &server,
        "DESK",
        "book-1",
        json!({"currency": "USD", "positions": [
            {"instrument_id": "BOND-001", "quantity": "1000000"},
            {"instrument_id": "BOND-W001", "quantity": "-500000"},
        ]}),
    )
    .await;
    let (status, taken) = close(&server, "DESK", "close-1").await;
    assert_eq!(status, StatusCode::CREATED, "{taken}");
    server.stop().await;

    settle_at(&config, "2027-01-19");
    let server = quoted(&config, &keys, 2, "99.25").await;
    let now = live_prices(&server).await;
    let (status, reply) = close(&server, "DESK", "close-2").await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "BOOK_ALREADY_CLOSED");

    const COUPON: f64 = 5.375 * 184.0 / 365.0;
    let since = pnl(&server, "DESK").await;
    assert_eq!(since["close"]["settlement_date"], "2026-10-06");
    assert_eq!(since["settlement_date"], "2027-01-19");
    let positions = by_instrument(&since);
    let closed = by_instrument(&taken);
    let mut total = 0.0;
    for (id, face) in [("BOND-001", 1_000_000.0), ("BOND-W001", -500_000.0)] {
        let value = &closed[id]["value"];
        let close_dirty = number(&value["clean_price_mid"]) + number(&value["accrued_interest"]);
        let (clean, accrued, _) = now[id];
        let cash = face * COUPON / 100.0;
        assert_near(number(&positions[id]["entitled_cash"]), cash, id);
        let expected = face * (clean + accrued - close_dirty + COUPON) / 100.0;
        assert_near(number(&positions[id]["pnl"]), expected, id);
        total += expected;
    }
    assert_near(
        number(&since["entitled_cash"]),
        500_000.0 * COUPON / 100.0,
        "book cash",
    );
    assert_near(number(&since["pnl"]), total, "book P&L");
    assert_eq!(since["excluded_count"], 0);
    server.stop().await;
}

fn summary(mut times: Vec<Duration>) -> String {
    times.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    format!(
        "median {:.1} ms, p95 {:.1} ms, max {:.1} ms",
        ms(times[times.len() / 2]),
        ms(times[times.len() * 95 / 100]),
        ms(times[times.len() - 1])
    )
}

/// A close of a 1,000-position book, and P&L since it across a settlement roll, at the
/// bounds: 1,000 valued positions, the cash of each projected.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "measurement"]
async fn close_and_pnl_time_at_the_bounds() {
    let (_dir, config, keys, server) = started(1_000, "99.25").await;
    let ids: Vec<String> = std::iter::once("BOND-001".to_owned())
        .chain((1..1_000).map(|index| format!("BOND-W{index:03}")))
        .collect();
    let positions: Vec<Value> = ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let face = if index % 3 == 0 { "-250000" } else { "1000000" };
            json!({"instrument_id": id, "quantity": face})
        })
        .collect();
    let mut closes = Vec::new();
    let mut bytes = 0;
    for book in 0..20 {
        let id = format!("BIG-{book:02}");
        put_book(
            &server,
            &id,
            &format!("big-{book:02}"),
            json!({"currency": "USD", "positions": positions}),
        )
        .await;
        let started = Instant::now();
        let (status, reply) = close(&server, &id, &format!("close-{book:02}")).await;
        closes.push(started.elapsed());
        assert_eq!(status, StatusCode::CREATED, "{reply}");
        bytes = serde_json::to_vec(&reply["positions"]).unwrap().len();
    }
    println!(
        "close of 1,000 positions: {}; its positions {} KiB as JSON",
        summary(closes),
        bytes / 1024
    );
    let mut same_date = Vec::new();
    for _ in 0..20 {
        let started = Instant::now();
        let reply = pnl(&server, "BIG-00").await;
        same_date.push(started.elapsed());
        assert_eq!(reply["excluded_count"], 0);
    }
    println!("P&L, same settlement date: {}", summary(same_date));
    server.stop().await;

    settle_at(&config, "2027-01-19");
    let server = quoted(&config, &keys, 2, "99.25").await;
    let mut rolled = Vec::new();
    for _ in 0..20 {
        let started = Instant::now();
        let reply = pnl(&server, "BIG-00").await;
        rolled.push(started.elapsed());
        assert_eq!(reply["excluded_count"], 0, "{}", reply["positions"][0]);
    }
    println!(
        "P&L across a roll, every position's cash projected: {}",
        summary(rolled)
    );
    server.stop().await;
}
