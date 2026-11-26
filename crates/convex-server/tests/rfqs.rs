//! Clients' requests for quote over the real composition and HTTP: a Live server whose
//! bond is quoted by a spread the desk marks, priced at receipt at its rule's width for the
//! trade's size scaled by the client tier's factor, checked against its book's limits, and
//! moved by the trader and the source until it is final. `rfq_pricing_time_at_the_bounds`
//! is the measurement (`--ignored --nocapture`).
#[path = "support/live_server.rs"]
mod live_server;
#[path = "../../convex-engine/tests/support/products.rs"]
mod products;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use convex_core::contracts::RequestId;
use convex_engine::ports::reference_master::MasterChange;
use convex_engine::ports::reference_repository::{ReferenceChange, ReferenceCommand};
use live_server::*;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const DESK: &str = "desk";
const BOND: &str = "BOND-001";
/// A stored bond the Live runtime does not value.
const OUTSIDE: &str = "BOND-X";
const BOOK: &str = "BOOK-IG";
const SOURCE: &str = "venue-sim";

/// Store, before the server starts, a width on the bond's rule (2, 4 and 8 bp by size), a
/// bond outside the plan, and an active configuration with `tiers`.
async fn prepare(database: &std::path::Path, tiers: Value) {
    let storage = convex_ext_redb::open_redb_storage(database)
        .unwrap()
        .storage;
    let master = |definition: Value| -> MasterChange {
        serde_json::from_value(json!({
            "expected_version": 1,
            "definition": {"kind": "pricing_rule", "definition": definition},
        }))
        .unwrap()
    };
    let rule = master(json!({
        "instrument_id": BOND,
        "method": {"kind": "benchmark_spread", "tenor": "10Y"},
        "benchmark_set_id": "UST-OTR",
        "width": {"unit": "basis_points", "buckets": [
            {"max_face": "1000000", "width": "2"},
            {"max_face": "10000000", "width": "4"},
            {"width": "8"},
        ]},
        "effective": {"from": "2025-01-01T00:00:00Z", "until": null},
        "provenance": {"source": "desk", "evidence_reference": "rfq-width"},
    }));
    let mut outside = support::fixture::bond();
    outside.instrument_id = convex_core::ids::InstrumentId::new(OUTSIDE);
    outside.isin = Some("fixture-isin-x".into());
    outside.cusip = Some("fixture-cusip-x".into());
    storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("rfq-desk").unwrap(),
                changes: vec![ReferenceChange::Create {
                    bond: Box::new(outside),
                }],
                master_changes: vec![rule],
            },
            support::fixture::time("2025-06-02T00:00:00Z"),
        )
        .await
        .unwrap();
    drop(storage);
    activate_client_tiers(database, tiers).await;
}

/// A Live desk: Treasuries priced, the bond marked 125 bp over the 10-year, and a book
/// holding 1,000,000 of it with a face limit of 3,000,000 and a market value limit.
struct Desk {
    _dir: tempfile::TempDir,
    config: convex_server::ServerConfig,
    server: Running,
}

async fn desk() -> Desk {
    let (dir, database, mut config, keys) = level_live_setup_from(2, Some(DESK)).await;
    config.desk_level_source = Some(DESK.into());
    prepare(
        &database,
        json!([
            {"client_tier": "1", "width_factor": "0.5"},
            {"client_tier": "2", "width_factor": "1"},
        ]),
    )
    .await;
    let server = Running::start(config.clone()).await;
    let prices = keys
        .treasuries
        .iter()
        .map(|key| {
            let mut event = image(1, "99.25");
            event.key = key.clone();
            event
        })
        .collect();
    assert_eq!(
        server.post_quotes(prices).await.status(),
        StatusCode::ACCEPTED
    );
    let (status, receipt) = server
        .send(
            Method::POST,
            "/api/v1/market/levels",
            Some(&json!({"source": DESK, "levels": [
                {"instrument_id": BOND, "field": "mid_spread", "level": "125"},
            ]})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{receipt}");
    priced_in_live(&server, BOND).await;
    let desk = Desk {
        _dir: dir,
        config,
        server,
    };
    desk.server
        .put(
            &format!("/api/v1/books/{BOOK}"),
            &json!({"currency": "USD", "positions": [
                {"instrument_id": BOND, "quantity": "1000000"},
            ]}),
        )
        .await;
    desk.server
        .put(
            &format!("/api/v1/books/{BOOK}/limits"),
            &json!({"limits": [
                {"limit_id": "face", "measure": {"kind": "face", "instrument": "each"}, "upper": "3000000"},
                {"limit_id": "net", "measure": {"kind": "market_value"}, "upper": "1000000000"},
            ]}),
        )
        .await;
    desk
}

/// Wait until `instrument_id`'s Live record carries a clean price.
async fn priced_in_live(server: &Running, instrument_id: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (_, page) = server
                .send(Method::GET, "/api/v1/runtime/results", None)
                .await;
            let priced = page["results"].as_array().is_some_and(|records| {
                records.iter().any(|record| {
                    record["result"]["provenance"]["subject"]["instrument_id"] == instrument_id
                        && record["result"]["results"]["clean_price_mid"].is_object()
                })
            });
            if priced {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
}

fn stamp(after: Duration) -> String {
    (chrono::Utc::now() + after).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// A request from tier `tier` for the client to `side` `face` of the bond, open for
/// `open`.
fn rfq(rfq_id: &str, side: &str, face: &str, tier: &str, open: Duration) -> Value {
    json!({
        "source_id": SOURCE,
        "rfq_id": rfq_id,
        "client_id": "CLIENT-17",
        "client_tier": tier,
        "instrument_id": BOND,
        "side": side,
        "face": face,
        "settlement_date": (chrono::Utc::now() + chrono::Duration::days(2)).date_naive().to_string(),
        "book_id": BOOK,
        "deadline": stamp(open),
    })
}

const MINUTE: Duration = Duration::from_secs(60);

async fn ingest(server: &Running, rfqs: Vec<Value>) -> (StatusCode, Vec<Value>) {
    let (status, reply) = server
        .send(Method::POST, "/api/v1/rfqs", Some(&json!({ "rfqs": rfqs })))
        .await;
    let answers = reply["rfqs"]
        .as_array()
        .unwrap_or_else(|| panic!("{status} {reply}"))
        .clone();
    (status, answers)
}

/// Ingest one request, which must be accepted; its key.
async fn accepted(server: &Running, request: Value) -> String {
    let (status, answers) = ingest(server, vec![request]).await;
    assert_eq!(status, StatusCode::OK, "{answers:?}");
    assert_eq!(answers[0]["result"], "accepted", "{answers:?}");
    assert_eq!(answers[0]["state"], "priced");
    answers[0]["rfq_key"].as_str().unwrap().to_owned()
}

async fn open_rfqs(server: &Running) -> Vec<Value> {
    let (status, reply) = server.send(Method::GET, "/api/v1/rfqs", None).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["withheld"], 0);
    reply["rfqs"].as_array().unwrap().clone()
}

async fn open_rfq(server: &Running, key: &str) -> Value {
    open_rfqs(server)
        .await
        .into_iter()
        .find(|rfq| rfq["rfq_key"] == key)
        .unwrap_or_else(|| panic!("{key} is not open"))
}

/// The request's `priced` entry: its configuration, auto price and check.
async fn priced(server: &Running, key: &str) -> Value {
    let recorded = entries(server, key).await;
    assert_eq!(recorded[1]["event"], "priced", "{recorded:?}");
    recorded[1].clone()
}

async fn entries(server: &Running, key: &str) -> Vec<Value> {
    let (status, reply) = server
        .send(Method::GET, &format!("/api/v1/rfqs/{key}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    reply["records"].as_array().unwrap().clone()
}

fn states(entries: &[Value]) -> Vec<&str> {
    entries
        .iter()
        .map(|entry| entry["state"].as_str().unwrap())
        .collect()
}

fn number(value: &Value) -> f64 {
    value
        .as_str()
        .and_then(|text| text.parse().ok())
        .or_else(|| value.as_f64())
        .unwrap_or_else(|| panic!("not a number: {value}"))
}

fn limit<'a>(check: &'a Value, id: &str) -> &'a Value {
    check["status"]["limits"]
        .as_array()
        .unwrap_or_else(|| panic!("no limits: {check}"))
        .iter()
        .find(|limit| limit["limit_id"] == id)
        .unwrap_or_else(|| panic!("no {id}: {check}"))
}

async fn answer(
    server: &Running,
    key: &str,
    action: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    server
        .send(
            Method::POST,
            &format!("/api/v1/rfqs/{key}/{action}"),
            body.as_ref(),
        )
        .await
}

#[tokio::test]
async fn a_request_is_priced_at_its_size_and_tier_on_the_desk_side_and_checked_against_its_book() {
    let desk = desk().await;
    let server = &desk.server;
    // A client sell of 5,000,000 hits the desk's bid: the 4 bp bucket, half each side.
    let full = accepted(server, rfq("RFQ-1", "sell", "5000000", "2", MINUTE)).await;
    let rfq_1 = priced(server, &full).await;
    let price = &rfq_1["auto_price"];
    assert_eq!(price["kind"], "priced", "{rfq_1}");
    assert_eq!(price["desk_side"], "bid");
    assert_eq!(number(&price["level"]["value"]), 127.0, "{price}");
    assert_eq!(price["level"]["unit"], "spread_basis_points", "{price}");
    assert!(price.get("mid_only").is_none(), "{price}");
    assert!(number(&price["clean_price"]) < number(&price["clean_price_mid"]));
    assert_eq!(
        number(&price["pricing_rule"]["two_way"]["width"]),
        4.0,
        "{price}"
    );
    assert_eq!(price["quote_input"]["field"], "mid_spread");
    assert_eq!(
        price["quote_input"]["observation"]["value"]["value"], "125",
        "{price}"
    );
    assert_eq!(
        price["curves"],
        json!([]),
        "a benchmark spread reads no curve"
    );
    let configuration = &rfq_1["configuration"];
    assert_eq!(configuration["configuration_id"], "CONFIG-RESTORE");
    assert_eq!(configuration["width_factor"], "1");
    // 1,000,000 held and 5,000,000 bought from the client pass the face limit.
    let check = &rfq_1["check"];
    assert_eq!(check["kind"], "evaluated", "{check}");
    assert_eq!(limit(check, "face")["status"], "breach", "{check}");
    assert_eq!(number(&limit(check, "face")["value"]), 6_000_000.0);
    assert_eq!(limit(check, "net")["status"], "within", "{check}");

    // A better tier halves the width; a client buy lifts the ask; a small size takes the
    // first bucket.
    let halved = accepted(server, rfq("RFQ-2", "sell", "5000000", "1", MINUTE)).await;
    let halved = priced(server, &halved).await;
    assert_eq!(number(&halved["auto_price"]["level"]["value"]), 126.0);
    let bought = accepted(server, rfq("RFQ-3", "buy", "500000", "2", MINUTE)).await;
    let bought = priced(server, &bought).await;
    let price = &bought["auto_price"];
    assert_eq!(price["desk_side"], "ask");
    assert_eq!(number(&price["level"]["value"]), 124.0, "{price}");
    assert!(number(&price["clean_price"]) > number(&price["clean_price_mid"]));
    // Selling the client 500,000 shortens the book to 500,000: within.
    assert_eq!(limit(&bought["check"], "face")["status"], "within");
    // Past the schedule's 10,000,000 bucket, the last takes it.
    let large = accepted(server, rfq("RFQ-4", "buy", "25000000", "2", MINUTE)).await;
    let large = priced(server, &large).await;
    assert_eq!(number(&large["auto_price"]["level"]["value"]), 121.0);

    // The list shows each open request's desk side price and how many limits the check
    // put in each status; its entries carry what each was made from.
    let row = open_rfq(server, &full).await;
    assert_eq!(row["state"], "priced");
    assert_eq!(number(&row["auto_price"]["level"]["value"]), 127.0, "{row}");
    assert!(row["auto_price"].get("pricing_rule").is_none(), "{row}");
    assert_eq!(
        row["check"],
        json!({"kind": "evaluated", "statuses": {"breach": 1, "within": 1}})
    );
    // The open requests come by deadline, and only in the state asked for.
    assert_eq!(open_rfqs(server).await.len(), 4);
    let (status, reply) = server
        .send(Method::GET, "/api/v1/rfqs?state=quoted", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["rfqs"], json!([]));
    desk.server.stop().await;
}

#[tokio::test]
async fn a_trader_answers_and_the_source_reports_the_outcome_which_changes_no_book() {
    let desk = desk().await;
    let server = &desk.server;
    let book_path = format!("/api/v1/books/{BOOK}");
    let (_, book) = server.send(Method::GET, &book_path, None).await;

    let key = accepted(server, rfq("RFQ-1", "sell", "5000000", "2", MINUTE)).await;
    // The source may not report an outcome before a quote.
    let (status, reply) = answer(server, &key, "outcome", Some(json!({"outcome": "done"}))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "RFQ_TRANSITION");
    let (status, reply) = answer(
        server,
        &key,
        "quote",
        Some(json!({"clean_price": "99.125"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["state"], "quoted");
    // One quote: a second is refused.
    let (status, reply) =
        answer(server, &key, "quote", Some(json!({"clean_price": "99.25"}))).await;
    assert_eq!(
        (status, reply["code"].as_str()),
        (StatusCode::CONFLICT, Some("RFQ_TRANSITION"))
    );
    let (status, reply) = answer(server, &key, "outcome", Some(json!({"outcome": "done"}))).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["state"], "done");
    // Final: nothing moves it again, and it is no longer open.
    let (status, reply) = answer(server, &key, "pass", None).await;
    assert_eq!(
        (status, reply["code"].as_str()),
        (StatusCode::CONFLICT, Some("RFQ_FINAL"))
    );
    assert!(open_rfqs(server).await.is_empty());

    let recorded = entries(server, &key).await;
    assert_eq!(states(&recorded), ["received", "priced", "quoted", "done"]);
    let actors: Vec<&str> = recorded
        .iter()
        .map(|entry| entry["actor"].as_str().unwrap())
        .collect();
    assert_eq!(actors, ["server", "server", "trader", "source"]);
    assert_eq!(recorded[0]["request"]["rfq_id"], "RFQ-1");
    assert_eq!(recorded[0]["instrument_id"], BOND);
    assert_eq!(recorded[1]["auto_price"]["kind"], "priced");
    assert_eq!(recorded[2]["clean_price"], "99.125");
    assert!(recorded
        .windows(2)
        .all(|pair| pair[0]["record_id"].as_u64() < pair[1]["record_id"].as_u64()));

    // Done is recorded only: the book's owning system sends the new position.
    let (_, after) = server.send(Method::GET, &book_path, None).await;
    assert_eq!(after, book);

    // A pass, and a cancellation of an open request.
    let passed = accepted(server, rfq("RFQ-2", "buy", "1000000", "2", MINUTE)).await;
    let (status, reply) = answer(server, &passed, "pass", None).await;
    assert_eq!(
        (status, reply["state"].as_str()),
        (StatusCode::OK, Some("passed"))
    );
    let cancelled = accepted(server, rfq("RFQ-3", "buy", "1000000", "2", MINUTE)).await;
    let (status, reply) = answer(
        server,
        &cancelled,
        "outcome",
        Some(json!({"outcome": "cancelled"})),
    )
    .await;
    assert_eq!(
        (status, reply["state"].as_str()),
        (StatusCode::OK, Some("cancelled"))
    );
    // A source cannot send the server's or the trader's events.
    let open = accepted(server, rfq("RFQ-4", "buy", "1000000", "2", MINUTE)).await;
    for outcome in ["priced", "quoted", "passed", "expired"] {
        let (status, _) = answer(server, &open, "outcome", Some(json!({"outcome": outcome}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{outcome}");
    }
    let (status, reply) = answer(server, &"0".repeat(64), "pass", None).await;
    assert_eq!(
        (status, reply["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("RFQ_UNKNOWN"))
    );
    desk.server.stop().await;
}

#[tokio::test]
async fn ingest_is_idempotent_by_source_and_rfq_id_and_answers_each_request() {
    let desk = desk().await;
    let server = &desk.server;
    let first = rfq("RFQ-1", "sell", "5000000", "2", MINUTE);
    let key = accepted(server, first.clone()).await;
    let mut changed = first.clone();
    changed["face"] = json!("6000000");
    let mut long = rfq("RFQ-2", "sell", "5000000", "2", MINUTE);
    long["deadline"] = json!(stamp(Duration::from_secs(301)));
    let mut unknown_book = rfq("RFQ-3", "sell", "5000000", "2", MINUTE);
    unknown_book["book_id"] = json!("BOOK-NONE");
    let mut by_isin = rfq("RFQ-4", "sell", "5000000", "2", MINUTE);
    by_isin.as_object_mut().unwrap().remove("instrument_id");
    by_isin["identifier"] = json!({"kind": "isin", "value": "US0000000001"});
    let (status, answers) = ingest(
        server,
        vec![
            first.clone(),
            rfq("RFQ-5", "sell", "5000000", "9", MINUTE),
            long,
            unknown_book,
            by_isin,
            rfq("RFQ-6", "sell", "5000000", "2", MINUTE),
            rfq("RFQ-6", "sell", "5000000", "2", MINUTE),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answers:?}");
    let results: Vec<(&str, Option<&str>)> = answers
        .iter()
        .map(|answer| (answer["result"].as_str().unwrap(), answer["code"].as_str()))
        .collect();
    assert_eq!(
        results,
        [
            ("retry", None),
            ("refused", Some("RFQ_CLIENT_TIER")),
            ("refused", Some("RFQ_DEADLINE")),
            ("refused", Some("RFQ_BOOK")),
            ("refused", Some("UNKNOWN_IDENTIFIER")),
            ("accepted", None),
            ("refused", Some("RFQ_DUPLICATE")),
        ],
        "{answers:?}"
    );
    assert_eq!(answers[0]["rfq_key"], key);
    assert_eq!(answers[0]["state"], "priced");
    // The same id with other content, in a later batch.
    let (status, answers) = ingest(server, vec![changed]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(answers[0]["code"], "RFQ_CONFLICT", "{answers:?}");
    // A batch with nothing taken is refused whole.
    let (status, _) = ingest(server, vec![rfq("RFQ-7", "sell", "5000000", "9", MINUTE)]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // After it is final, the source's id is still taken, from its entries.
    let (status, _) = answer(server, &key, "pass", None).await;
    assert_eq!(status, StatusCode::OK);
    let (_, answers) = ingest(server, vec![first]).await;
    assert_eq!(
        (answers[0]["result"].as_str(), answers[0]["state"].as_str()),
        (Some("retry"), Some("passed"))
    );
    desk.server.stop().await;
}

#[tokio::test]
async fn a_bond_outside_the_live_universe_is_priced_for_the_trader_with_unknown_market_limits() {
    let desk = desk().await;
    let server = &desk.server;
    let mut request = rfq("RFQ-X", "sell", "4000000", "2", MINUTE);
    request["instrument_id"] = json!(OUTSIDE);
    let key = accepted(server, request).await;
    let rfq = priced(server, &key).await;
    let price = &rfq["auto_price"];
    assert_eq!(price["kind"], "unpriced", "{price}");
    assert!(
        price["reason"]
            .as_str()
            .unwrap()
            .contains("the Live runtime does not value it"),
        "{price}"
    );
    let check = &rfq["check"];
    // A face limit reads no market data: 4,000,000 of the bond passes 3,000,000.
    assert_eq!(limit(check, "face")["status"], "breach", "{check}");
    assert_eq!(limit(check, "face")["member"], OUTSIDE, "{check}");
    assert_eq!(limit(check, "net")["status"], "unknown", "{check}");
    assert!(limit(check, "net")["reason"].is_object(), "{check}");
    // The trader sets the price.
    let (status, reply) = answer(server, &key, "quote", Some(json!({"clean_price": "98.5"}))).await;
    assert_eq!(
        (status, reply["state"].as_str()),
        (StatusCode::OK, Some("quoted"))
    );
    desk.server.stop().await;
}

/// A sinking-fund bond quoted by its price per 100 current face is valued in Live per 100
/// original face at its factor. Its request is priced from that Live quote and sent to
/// the client per 100 current face, mid only, with its factor at the request's
/// settlement: the Live quote's price back.
#[tokio::test]
async fn a_sinkers_request_is_quoted_per_current_face_at_its_factor() {
    use convex_engine::{ports::configuration::InstrumentPin, RuntimePlan};
    const SINKER: &str = "SINKER-001";
    let (_dir, database, config, keys) = level_live_setup_from(2, Some(DESK)).await;
    let mut sinker = support::fixture::bond();
    sinker.instrument_id = convex_core::ids::InstrumentId::new(SINKER);
    sinker.isin = Some("fixture-isin-sinker".into());
    sinker.cusip = Some("fixture-cusip-sinker".into());
    let sinker = products::sinker(sinker);
    let storage = convex_ext_redb::open_redb_storage(&database)
        .unwrap()
        .storage;
    storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("rfq-sinker").unwrap(),
                changes: vec![ReferenceChange::Create {
                    bond: Box::new(sinker.clone()),
                }],
                master_changes: vec![products::convention(&sinker)],
            },
            support::fixture::time("2025-06-02T00:00:00Z"),
        )
        .await
        .unwrap();
    drop(storage);
    // Live values it from its price, on the Treasuries' feed.
    let path = database.with_file_name("runtime.json");
    let mut plan: RuntimePlan = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    plan.assembly.instruments.push(InstrumentPin {
        instrument_id: convex_core::contracts::InstrumentId::new(SINKER).unwrap(),
        version: support::fixture::version(1),
    });
    let mut quote = plan
        .assembly
        .quotes
        .iter()
        .find(|quote| quote.key == keys.treasuries[0])
        .unwrap()
        .clone();
    quote.key.instrument_id = sinker.instrument_id.clone();
    let sinker_key = quote.key.clone();
    plan.assembly.quotes.push(quote);
    // The staged configuration's runtime asks for the factor, which only a sinker publishes.
    plan.measures
        .push(convex_core::contracts::MeasureId::new("factor").unwrap());
    std::fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
    prepare(
        &database,
        json!([{"client_tier": "2", "width_factor": "1"}]),
    )
    .await;
    let server = Running::start(config).await;
    let mut prices = images(&keys.treasuries, 1, "99.25");
    let mut price = image(1, "99.5");
    price.key = sinker_key;
    prices.push(price);
    assert_eq!(
        server.post_quotes(prices).await.status(),
        StatusCode::ACCEPTED
    );
    priced_in_live(&server, SINKER).await;
    server
        .put(
            &format!("/api/v1/books/{BOOK}"),
            &json!({"currency": "USD", "positions": [
                {"instrument_id": SINKER, "quantity": "1000000"},
            ]}),
        )
        .await;
    server
        .put(
            &format!("/api/v1/books/{BOOK}/limits"),
            &json!({"limits": [
                {"limit_id": "net", "measure": {"kind": "market_value"}, "upper": "1000000000"},
            ]}),
        )
        .await;
    let mut request = rfq("RFQ-S", "sell", "2000000", "2", MINUTE);
    request["instrument_id"] = json!(SINKER);
    let settlement = request["settlement_date"].as_str().unwrap().to_owned();
    let key = accepted(&server, request).await;
    let rfq = priced(&server, &key).await;
    let price = &rfq["auto_price"];
    assert_eq!(price["kind"], "priced", "{price}");
    assert_eq!(number(&price["clean_price"]), 99.5, "{price}");
    assert_eq!(number(&price["clean_price_mid"]), 99.5, "{price}");
    assert_eq!(price["mid_only"], "a sinking-fund bond is quoted mid only");
    let settles = convex_core::Date::parse(&settlement).unwrap();
    let retired: rust_decimal::Decimal = sinker
        .contractual_terms
        .as_ref()
        .unwrap()
        .sinking_schedule
        .iter()
        .filter(|sink| sink.date.date() <= settles)
        .map(|sink| sink.amount.decimal())
        .sum();
    let factor =
        (rust_decimal::Decimal::ONE_HUNDRED - retired) / rust_decimal::Decimal::ONE_HUNDRED;
    assert_eq!(
        price["factor"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        factor,
        "{price}"
    );
    // Its book's limits are checked as if done, on the face held, original face, at its
    // Live dirty price per original face: at the Live settlement, after two sinks, 99.5
    // times 0.8 and 0.28 accrued, the reference's.
    let check = &rfq["check"];
    assert_eq!(check["kind"], "evaluated", "{rfq}");
    assert_eq!(
        number(&limit(check, "net")["value"]),
        3_000_000.0 * 79.88 / 100.0,
        "{check}"
    );
    let (_, page) = server
        .send(Method::GET, "/api/v1/runtime/results", None)
        .await;
    let live = page["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["result"]["provenance"]["subject"]["instrument_id"] == SINKER)
        .unwrap();
    assert_eq!(
        number(&live["result"]["results"]["clean_price_mid"]["value"]["value"]),
        79.6,
        "{live}"
    );
    assert_eq!(
        number(&live["result"]["results"]["factor"]["value"]["value"]),
        0.8,
        "{live}"
    );
    server.stop().await;
}

#[tokio::test]
async fn a_deadline_expires_a_request_once_and_a_restart_expires_every_open_one() {
    let desk = desk().await;
    let server = &desk.server;
    let late = rfq("RFQ-1", "sell", "1000000", "2", Duration::from_millis(1500));
    let soon = accepted(server, late.clone()).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while states(&entries(server, &soon).await).last() != Some(&"expired") {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let (status, reply) = answer(server, &soon, "quote", Some(json!({"clean_price": "99"}))).await;
    assert_eq!(
        (status, reply["code"].as_str()),
        (StatusCode::CONFLICT, Some("RFQ_FINAL"))
    );
    let recorded = entries(server, &soon).await;
    assert_eq!(states(&recorded), ["received", "priced", "expired"]);
    assert_eq!(recorded[2]["reason"], "deadline");

    let left = [
        accepted(server, rfq("RFQ-2", "sell", "1000000", "2", MINUTE)).await,
        accepted(server, rfq("RFQ-3", "buy", "1000000", "2", MINUTE)).await,
    ];
    let (status, _) = answer(
        server,
        &left[1],
        "quote",
        Some(json!({"clean_price": "99"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let Desk {
        _dir: _kept,
        config,
        server,
    } = desk;
    server.stop().await;
    let server = Running::start(config.clone()).await;
    for key in &left {
        tokio::time::timeout(Duration::from_secs(10), async {
            while states(&entries(&server, key).await).last() != Some(&"expired") {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        let recorded = entries(&server, key).await;
        assert_eq!(recorded.last().unwrap()["reason"], "restart");
        assert_eq!(
            states(&recorded)
                .iter()
                .filter(|state| **state == "expired")
                .count(),
            1
        );
    }
    assert!(open_rfqs(&server).await.is_empty());
    // The source's retry, past its deadline, is recognised, not refused.
    let (status, answers) = ingest(&server, vec![late.clone()]).await;
    assert_eq!(status, StatusCode::OK, "{answers:?}");
    assert_eq!(
        (answers[0]["result"].as_str(), answers[0]["state"].as_str()),
        (Some("retry"), Some("expired"))
    );
    // A second restart finds nothing open, and records nothing more.
    server.stop().await;
    let server = Running::start(config).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(states(&entries(&server, &left[0]).await).len(), 3);
    server.stop().await;
}

#[tokio::test]
async fn the_desk_holds_256_open_requests() {
    let desk = desk().await;
    let server = &desk.server;
    for batch in 0..4 {
        let rfqs = (0..64)
            .map(|index| {
                rfq(
                    &format!("RFQ-{batch}-{index}"),
                    "buy",
                    "1000000",
                    "2",
                    MINUTE,
                )
            })
            .collect();
        let (status, answers) = ingest(server, rfqs).await;
        assert_eq!(status, StatusCode::OK);
        assert!(answers.iter().all(|answer| answer["result"] == "accepted"));
    }
    let (status, answers) =
        ingest(server, vec![rfq("RFQ-257", "buy", "1000000", "2", MINUTE)]).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{answers:?}");
    assert_eq!(answers[0]["code"], "RFQ_CAPACITY");
    assert_eq!(open_rfqs(server).await.len(), 256);
    desk.server.stop().await;
}

#[tokio::test]
async fn the_rfq_topic_sends_each_open_request_then_every_transition_on_its_books() {
    let desk = desk().await;
    let server = &desk.server;
    let other = "BOOK-OTHER";
    server
        .put(
            &format!("/api/v1/books/{other}"),
            &json!({"currency": "USD", "positions": [
                {"instrument_id": BOND, "quantity": "1000000"},
            ]}),
        )
        .await;
    let later = accepted(server, rfq("RFQ-1", "sell", "5000000", "2", 2 * MINUTE)).await;
    let sooner = accepted(server, rfq("RFQ-2", "buy", "1000000", "1", MINUTE)).await;

    // The seed is every open request on the books, by deadline, each as the list shows it.
    let mut socket = server.session(None).await;
    let seed = subscribe_rfqs(&mut socket, &[BOOK]).await;
    let keys: Vec<&str> = seed
        .iter()
        .map(|frame| frame["rfq_key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, [sooner.as_str(), later.as_str()]);
    let mut listed = open_rfq(server, &sooner).await;
    listed["type"] = json!("rfq");
    assert_eq!(seed[0], listed);
    assert_eq!(seed[1]["state"], "priced");
    assert_eq!(seed[1]["auto_price"]["kind"], "priced");
    assert_eq!(seed[1]["check"]["kind"], "evaluated");
    let priced_id = seed[1]["record_id"].as_u64().unwrap();

    // Then each transition, once recorded, with its entry's ID.
    let (status, _) = answer(
        server,
        &later,
        "quote",
        Some(json!({"clean_price": "99.125"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let quoted = next_frame(&mut socket).await;
    assert_eq!(
        (quoted["type"].as_str(), quoted["rfq_key"].as_str()),
        (Some("rfq"), Some(later.as_str())),
        "{quoted}"
    );
    assert_eq!(quoted["state"], "quoted");
    assert_eq!(quoted["quote"]["clean_price"], "99.125");
    let quoted_id = quoted["record_id"].as_u64().unwrap();
    assert!(quoted_id > priced_id);
    let (status, _) = answer(server, &later, "outcome", Some(json!({"outcome": "done"}))).await;
    assert_eq!(status, StatusCode::OK);
    let done = next_frame(&mut socket).await;
    assert_eq!(done["state"], "done", "{done}");
    assert!(done["record_id"].as_u64().unwrap() > quoted_id);
    let recorded = entries(server, &later).await;
    assert_eq!(done["record_id"], recorded.last().unwrap()["record_id"]);

    // A request on a book the session does not follow is not sent; a new one on its book
    // is, and so is its expiry.
    let (status, answers) = ingest(
        server,
        vec![{
            let mut request = rfq("RFQ-3", "buy", "1000000", "2", MINUTE);
            request["book_id"] = json!(other);
            request
        }],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answers:?}");
    let brief = accepted(
        server,
        rfq("RFQ-4", "buy", "1000000", "2", Duration::from_millis(1500)),
    )
    .await;
    let received = next_frame(&mut socket).await;
    assert_eq!(
        (received["rfq_key"].as_str(), received["state"].as_str()),
        (Some(brief.as_str()), Some("priced")),
        "{received}"
    );
    let expired = next_frame(&mut socket).await;
    assert_eq!(
        (expired["rfq_key"].as_str(), expired["state"].as_str()),
        (Some(brief.as_str()), Some("expired")),
        "{expired}"
    );

    // Subscribing again resends the seed; after unsubscribing nothing more is sent.
    let again = subscribe_rfqs(&mut socket, &[BOOK]).await;
    assert_eq!(again.len(), 1);
    assert_eq!(again[0]["rfq_key"], sooner.as_str());
    send_frame(
        &mut socket,
        json!({"type": "unsubscribe_rfqs", "book_ids": [BOOK]}),
    )
    .await;
    assert_eq!(next_frame(&mut socket).await["type"], "unsubscribed");
    let (status, _) = answer(server, &sooner, "pass", None).await;
    assert_eq!(status, StatusCode::OK);
    send_frame(&mut socket, json!({"type": "ping", "timestamp": 1})).await;
    assert_eq!(next_frame(&mut socket).await["type"], "pong");

    // A message lists at most 64 books.
    let many: Vec<String> = (0..65).map(|index| format!("BOOK-{index}")).collect();
    send_frame(
        &mut socket,
        json!({"type": "subscribe_rfqs", "book_ids": many}),
    )
    .await;
    let refused = next_frame(&mut socket).await;
    assert_eq!(refused["type"], "error", "{refused}");
    desk.server.stop().await;
}

/// A simulator's binary, run against `server` with `args`.
struct Simulator {
    child: tokio::process::Child,
    lines: tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>,
}

impl Simulator {
    fn start(binary: &str, server: &Running, args: &[&str]) -> Self {
        let mut child = tokio::process::Command::new(binary)
            .arg("--server")
            .arg(&server.base)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        Self {
            child,
            lines: tokio::io::AsyncBufReadExt::lines(tokio::io::BufReader::new(stdout)),
        }
    }

    /// The next event it prints.
    async fn event(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(30), self.lines.next_line())
            .await
            .expect("no event")
            .unwrap()
            .expect("the simulator ended");
        serde_json::from_str(&line).unwrap()
    }

    /// Every event it prints until it exits, which it must do successfully.
    async fn events_to_exit(mut self) -> Vec<Value> {
        let mut events = Vec::new();
        while let Some(line) = tokio::time::timeout(Duration::from_secs(60), self.lines.next_line())
            .await
            .expect("the simulator did not exit")
            .unwrap()
        {
            events.push(serde_json::from_str(&line).unwrap());
        }
        assert!(self.child.wait().await.unwrap().success());
        events
    }
}

const RFQ_SIM: &str = env!("CARGO_BIN_EXE_convex-rfq-sim");
const BMS_SIM: &str = env!("CARGO_BIN_EXE_convex-bms-sim");

/// The face the book holds of the bond.
async fn held(server: &Running) -> f64 {
    let (status, book) = server
        .send(Method::GET, &format!("/api/v1/books/{BOOK}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{book}");
    book["positions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|position| position["instrument_id"] == BOND)
        .map_or(0.0, |position| number(&position["quantity"]))
}

/// The BMS simulator applies a done trade to the book, and the book's limit status is then
/// what the request's check said at receipt: the check is the trade as if done.
#[tokio::test]
async fn the_bms_books_a_done_trade_and_the_status_is_then_the_check_at_receipt() {
    let desk = desk().await;
    let server = &desk.server;
    let mut bms = Simulator::start(BMS_SIM, server, &["--books", BOOK, "--trades", "1"]);
    assert_eq!(bms.event().await["event"], "following");

    // The desk buys 1,000,000 from the client: the book then holds 2,000,000.
    let key = accepted(server, rfq("RFQ-1", "sell", "1000000", "2", MINUTE)).await;
    let check = priced(server, &key).await["check"].clone();
    let (status, _) = answer(server, &key, "quote", Some(json!({"clean_price": "99.5"}))).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = answer(server, &key, "outcome", Some(json!({"outcome": "done"}))).await;
    assert_eq!(status, StatusCode::OK);
    let events = bms.events_to_exit().await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        (events[0]["event"].as_str(), events[0]["rfq_key"].as_str()),
        (Some("applied"), Some(key.as_str())),
        "{events:?}"
    );
    assert_eq!(events[0]["position_change"], "1000000");
    assert_eq!(held(server).await, 2_000_000.0);

    let (status, booked) = server
        .send(
            Method::GET,
            &format!("/api/v1/books/{BOOK}/limits/status"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{booked}");
    let booked = json!({ "status": booked });
    for limit_id in ["face", "net"] {
        let (before, after) = (limit(&check, limit_id), limit(&booked, limit_id));
        assert_eq!(
            before["status"], after["status"],
            "{limit_id}: {before} {after}"
        );
        assert_eq!(
            number(&before["value"]),
            number(&after["value"]),
            "{limit_id}"
        );
    }
    desk.server.stop().await;
}

/// The trader: quote every request on the book at its auto price, or 99 when it has none.
fn quote_every_request(server: &Running) -> tokio::task::JoinHandle<()> {
    let (client, base) = (server.client.clone(), server.base.clone());
    let ws_url = server.ws_url();
    tokio::spawn(async move {
        let (mut socket, _) = tokio_tungstenite::connect_async(ws_url).await.unwrap();
        send_frame(
            &mut socket,
            json!({"type": "subscribe_rfqs", "book_ids": [BOOK]}),
        )
        .await;
        loop {
            let frame = next_frame(&mut socket).await;
            if frame["type"] != "rfq" || frame["state"] != "priced" {
                continue;
            }
            let price = match frame["auto_price"]["kind"].as_str() {
                Some("priced") => frame["auto_price"]["clean_price"].clone(),
                _ => json!("99"),
            };
            let key = frame["rfq_key"].as_str().unwrap();
            client
                .post(format!("{base}/api/v1/rfqs/{key}/quote"))
                .json(&json!({"clean_price": price}))
                .send()
                .await
                .unwrap();
        }
    })
}

/// The source simulator's requests are quoted by the desk and answered by its clients, and
/// the BMS simulator books each done trade once.
#[tokio::test]
async fn the_simulators_trade_with_a_quoting_desk_and_each_done_trade_is_booked_once() {
    let desk = desk().await;
    let server = &desk.server;
    let mut bms = Simulator::start(BMS_SIM, server, &["--books", BOOK]);
    assert_eq!(bms.event().await["event"], "following");
    let trader = quote_every_request(server);
    let source = Simulator::start(
        RFQ_SIM,
        server,
        &[
            "--source-id",
            SOURCE,
            "--books",
            BOOK,
            "--bonds",
            BOND,
            "--tiers",
            "1,2",
            "--rate",
            "20",
            "--count",
            "12",
            "--seed",
            "7",
        ],
    );
    let events = source.events_to_exit().await;
    trader.abort();

    let of = |kind: &str| -> Vec<&Value> {
        events
            .iter()
            .filter(|event| event["event"] == kind)
            .collect()
    };
    let sent = of("sent");
    let finals = of("final");
    assert_eq!(sent.len(), 12, "{events:?}");
    assert_eq!(finals.len(), 12, "{events:?}");
    let summary = of("summary");
    assert_eq!(summary[0]["taken"], 12);
    let done: std::collections::BTreeSet<&str> = finals
        .iter()
        .filter(|event| event["state"] == "done")
        .map(|event| event["rfq_key"].as_str().unwrap())
        .collect();
    assert!(!done.is_empty(), "{events:?}");
    // Each request is final as its client answered: none expired.
    assert!(
        finals.iter().all(|event| event["state"] != "expired"),
        "{events:?}"
    );
    let expected = 1_000_000.0
        + sent
            .iter()
            .filter(|event| done.contains(event["rfq_key"].as_str().unwrap()))
            .map(|event| {
                let face = number(&event["request"]["face"]);
                if event["request"]["side"] == "buy" {
                    -face
                } else {
                    face
                }
            })
            .sum::<f64>();
    let mut applied = std::collections::BTreeSet::new();
    while applied.len() < done.len() {
        let event = bms.event().await;
        assert_eq!(event["event"], "applied", "{event}");
        assert!(applied.insert(event["rfq_key"].as_str().unwrap().to_owned()));
    }
    assert_eq!(
        applied
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        done
    );
    assert_eq!(held(server).await, expected);
    drop(bms);
    desk.server.stop().await;
}

/// Receipt to `priced` for one request with the desk empty and with 255 open.
#[tokio::test]
#[ignore]
async fn rfq_pricing_time_at_the_bounds() {
    let desk = desk().await;
    let server = &desk.server;
    let mut alone = Vec::new();
    for round in 0..20 {
        assert_eq!(open_rfqs(server).await.len(), 0);
        let started = Instant::now();
        let key = accepted(
            server,
            rfq(&format!("ALONE-{round}"), "buy", "1000000", "2", MINUTE),
        )
        .await;
        alone.push(started.elapsed());
        let (status, _) = answer(server, &key, "pass", None).await;
        assert_eq!(status, StatusCode::OK);
    }
    for batch in 0..4 {
        let rfqs = (0..if batch == 3 { 63 } else { 64 })
            .map(|index| {
                rfq(
                    &format!("FILL-{batch}-{index}"),
                    "buy",
                    "1000000",
                    "2",
                    MINUTE,
                )
            })
            .collect();
        let (status, answers) = ingest(server, rfqs).await;
        assert_eq!(status, StatusCode::OK);
        assert!(answers.iter().all(|answer| answer["result"] == "accepted"));
    }
    let mut full = Vec::new();
    for round in 0..20 {
        assert_eq!(open_rfqs(server).await.len(), 255);
        let started = Instant::now();
        let key = accepted(
            server,
            rfq(&format!("FULL-{round}"), "buy", "1000000", "2", MINUTE),
        )
        .await;
        full.push(started.elapsed());
        let (status, _) = answer(server, &key, "pass", None).await;
        assert_eq!(status, StatusCode::OK);
    }
    let median = |mut times: Vec<Duration>| {
        times.sort();
        times[times.len() / 2]
    };
    println!(
        "receipt to priced, median of 20: {:?} with none open, {:?} with 255 open",
        median(alone),
        median(full)
    );
    desk.server.stop().await;
}
