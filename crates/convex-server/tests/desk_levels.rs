//! The desk's levels over the real composition and HTTP: `POST /api/v1/market/levels`
//! marks a bond quoted by a spread on the desk's level source, which the route alone
//! writes, and the Live runtime prices it from the mark.
#[path = "support/live_server.rs"]
mod live_server;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use live_server::*;
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::time::Duration;

const DESK: &str = "desk";
const BOND: &str = "BOND-001";

/// A Live server whose two bonds read their spreads over the 10-year from the desk, with
/// every Treasury priced.
async fn desk_server() -> (
    tempfile::TempDir,
    convex_server::ServerConfig,
    Running,
    LevelKeys,
) {
    let (dir, _, mut config, keys) = level_live_setup_from(2, Some(DESK)).await;
    config.desk_level_source = Some(DESK.into());
    let server = Running::start(config.clone()).await;
    price_treasuries(&server, &keys, 1).await;
    (dir, config, server, keys)
}

async fn price_treasuries(server: &Running, keys: &LevelKeys, sequence: u64) {
    let prices = keys
        .treasuries
        .iter()
        .map(|key| {
            let mut event = image(sequence, "99.25");
            event.key = key.clone();
            event
        })
        .collect();
    let response = server.post_quotes(prices).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

async fn mark(server: &Running, body: Value) -> (StatusCode, Value) {
    let response = server
        .client
        .post(format!("{}/api/v1/market/levels", server.base))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

fn spread(instrument_id: &str, level: &str) -> Value {
    json!({"instrument_id": instrument_id, "field": "mid_spread", "level": level})
}

async fn marks(server: &Running) -> Value {
    server
        .client
        .get(format!("{}/api/v1/market/levels", server.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// The bond's Live clean price mid once its record is newer than `after`, with that
/// record's revision.
async fn clean_price_after(server: &Running, instrument_id: &str, after: u64) -> (u64, String) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let page: Value = server
                .client
                .get(format!("{}/api/v1/runtime/results", server.base))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            let record = page["results"].as_array().unwrap().iter().find(|record| {
                record["result"]["provenance"]["subject"]["instrument_id"] == instrument_id
            });
            if let Some(record) = record {
                let revision = record["revision"].as_u64().unwrap();
                let clean = &record["result"]["results"]["clean_price_mid"]["value"]["value"];
                if revision > after {
                    if let Some(clean) = clean.as_str() {
                        return (revision, clean.to_owned());
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn a_desk_mark_prices_its_bond_and_a_nudge_reprices_it() {
    let (dir, config, server, keys) = desk_server().await;
    let before = marks(&server).await;
    assert_eq!(before["source"], DESK);
    let levels = before["levels"].as_array().unwrap();
    assert_eq!(levels.len(), 2, "{before}");
    assert!(levels
        .iter()
        .all(|level| level["field"] == "mid_spread" && level["mark"].is_null()));

    let (status, receipt) = mark(
        &server,
        json!({"source": DESK, "levels": [spread(BOND, "125")]}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{receipt}");
    assert_eq!(receipt["accepted"], 1);
    let (revision, at_125) = clean_price_after(&server, BOND, 0).await;

    // A level is absolute: a wider spread prices lower, and the mark reads back as sent.
    let (status, _) = mark(
        &server,
        json!({"source": DESK, "levels": [spread(BOND, "126.5")]}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (_, at_126) = clean_price_after(&server, BOND, revision).await;
    let price = |text: &str| text.parse::<f64>().unwrap();
    assert!(price(&at_126) < price(&at_125), "{at_126} after {at_125}");
    let after = marks(&server).await;
    let marked = after["levels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|level| level["instrument_id"] == BOND)
        .unwrap();
    assert_eq!(marked["mark"]["level"], "126.5", "{after}");
    assert!(marked["mark"]["marked_at"].is_string());

    // A restart holds no level, and the first mark after it prices the bond again.
    server.stop().await;
    let server = Running::start(config).await;
    assert!(marks(&server).await["levels"]
        .as_array()
        .unwrap()
        .iter()
        .all(|level| level["mark"].is_null()));
    price_treasuries(&server, &keys, 2).await;
    let (status, _) = mark(
        &server,
        json!({"source": DESK, "levels": [spread(BOND, "126.5")]}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (_, restarted) = clean_price_after(&server, BOND, 0).await;
    assert_eq!(restarted, at_126);
    server.stop().await;
    drop(dir);
}

#[tokio::test]
async fn a_mark_the_desk_does_not_quote_refuses_the_whole_batch() {
    let (dir, _, server, keys) = desk_server().await;
    let treasury = keys.treasuries[0].instrument_id.as_str().to_owned();
    let refusals = [
        (spread(&treasury, "10"), "NOT_DESK_QUOTED"),
        (
            json!({"instrument_id": BOND, "field": "mid_yield", "level": "0.05"}),
            "WRONG_LEVEL_FIELD",
        ),
        (spread(BOND, "20001"), "LEVEL_OUT_OF_RANGE"),
    ];
    for (refused, code) in refusals {
        let body = json!({"source": DESK, "levels": [spread("BOND-W001", "125"), refused]});
        let (status, reply) = mark(&server, body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["rows"][0]["index"], 1);
        assert_eq!(reply["rows"][0]["code"], code);
    }
    for (body, why) in [
        (
            json!({"source": "feed-b", "levels": [spread(BOND, "125")]}),
            "another source",
        ),
        (json!({"source": DESK, "levels": []}), "no level"),
        (
            json!({"source": DESK, "levels": [spread(BOND, "125"), spread(BOND, "126")]}),
            "a bond twice",
        ),
        (
            json!({"source": DESK, "levels": [{"instrument_id": BOND, "field": "mid_spread", "level": 125}]}),
            "a number, not an exact decimal",
        ),
    ] {
        let (status, reply) = mark(&server, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{why}: {reply}");
    }
    // Nothing was marked.
    assert!(marks(&server).await["levels"]
        .as_array()
        .unwrap()
        .iter()
        .all(|level| level["mark"].is_null()));

    // A feed cannot write the desk's source.
    let mut feed = image(1, "125");
    feed.key = keys.bonds[0].clone();
    let refused = server.post_quotes(vec![feed]).await;
    assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let reply: Value = refused.json().await.unwrap();
    assert_eq!(reply["rows"][0]["code"], "DESK_LEVEL_SOURCE");
    server.stop().await;
    drop(dir);
}

#[tokio::test]
async fn startup_refuses_a_desk_source_no_bond_reads_a_level_from() {
    // The feed's source carries prices, and a source the plan never names carries nothing.
    for source in ["feed-a", "nobody"] {
        let (_dir, _, mut config, _) = level_live_setup_from(1, Some(DESK)).await;
        config.desk_level_source = Some(source.into());
        let refused = convex_server::compose(config).await;
        assert!(
            matches!(
                refused,
                Err(convex_server::StartupError::Composition {
                    stage: "desk_level_source",
                    ..
                })
            ),
            "{source}"
        );
    }
    // A server without a Live runtime takes no desk levels.
    let (_dir, database, _) = live_setup().await;
    let mut config = live_server::config(convex_server::ServerProfile::Demo, &database, None);
    config.desk_level_source = Some(DESK.into());
    assert!(matches!(
        convex_server::compose(config).await,
        Err(convex_server::StartupError::Configuration(
            convex_server::ConfigError::Unsupported {
                field: "desk_level_source",
                ..
            }
        ))
    ));
}
