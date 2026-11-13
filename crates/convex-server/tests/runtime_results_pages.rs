//! `GET /api/v1/runtime/results` pages by instrument (P17a, D103): at most `limit`
//! results (256 unless asked, at most 1,024) in instrument ID order after `after`, with
//! `next_after` and `more`. A page read while the runtime ticks holds each instrument
//! once, and no record older than one read before it. Outside local development a full
//! page passes the access layer, which does not inspect its output (D103a).
#[allow(dead_code)] // Only the issuer, the policy and the request helper.
#[path = "support/access_fixture.rs"]
mod identity;
#[path = "support/live_server.rs"]
mod live_server;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use convex_engine::ports::quote_event::{QuoteEvent, QuoteKey};
use convex_server::access::{AccessControl, ResourceKind};
use live_server::{image, wide_live_setup, Running};
use reqwest::StatusCode;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

/// An image of `key` at `sequence`.
fn image_of(key: &QuoteKey, sequence: u64, price: &str) -> QuoteEvent {
    let mut event = image(sequence, price);
    event.key = key.clone();
    event
}

async fn page(server: &Running, query: &str) -> (StatusCode, Value) {
    let response = server
        .client
        .get(format!("{}/api/v1/runtime/results{query}", server.base))
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

fn instruments(page: &Value) -> Vec<String> {
    page["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            r["result"]["provenance"]["subject"]["instrument_id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

/// A Live server of three bonds, each valued once.
async fn three_bonds() -> (tempfile::TempDir, Running, Vec<QuoteKey>) {
    let (dir, _, live, keys) = wide_live_setup(3).await;
    let server = Running::start(live).await;
    let images = keys.iter().map(|key| image_of(key, 1, "99.25")).collect();
    assert!(server.post_quotes(images).await.status().is_success());
    tokio::time::timeout(Duration::from_secs(10), async {
        while page(&server, "").await.1["results"]
            .as_array()
            .is_none_or(|results| {
                results.len() < 3 || results.iter().any(|r| r["result"]["status"] != "Complete")
            })
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    (dir, server, keys)
}

#[tokio::test]
async fn results_are_read_in_pages_by_instrument() {
    let (_dir, server, _) = three_bonds().await;
    let (status, all) = page(&server, "").await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert_eq!(instruments(&all), ["BOND-001", "BOND-W001", "BOND-W002"]);
    assert_eq!(
        (&all["next_after"], &all["more"]),
        (&Value::Null, &Value::Bool(false))
    );

    let (_, first) = page(&server, "?limit=2").await;
    assert_eq!(instruments(&first), ["BOND-001", "BOND-W001"]);
    assert_eq!(first["next_after"], "BOND-W001");
    assert_eq!(first["more"], true);
    let (_, last) = page(&server, "?after=BOND-W001&limit=2").await;
    assert_eq!(instruments(&last), ["BOND-W002"]);
    assert_eq!(
        (&last["next_after"], &last["more"]),
        (&Value::Null, &Value::Bool(false))
    );
    // The cursor need not be an instrument the runtime values.
    let (_, between) = page(&server, "?after=BOND-0&limit=1").await;
    assert_eq!(instruments(&between), ["BOND-001"]);
    let (_, past) = page(&server, "?after=ZZZ").await;
    assert_eq!(instruments(&past), Vec::<String>::new());
    assert_eq!(page(&server, "?limit=1024").await.0, StatusCode::OK);
    for refused in [
        "?limit=0",
        "?limit=1025",
        "?limit=many",
        "?after=",
        "?offset=1",
    ] {
        let (status, body) = page(&server, refused).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}: {body}");
    }
    server.stop().await;
}

#[tokio::test]
async fn a_page_read_while_the_runtime_ticks_holds_each_instrument_once() {
    let (_dir, server, keys) = three_bonds().await;
    let base = server.base.clone();
    let client = server.client.clone();
    let ticking = tokio::spawn(async move {
        for sequence in 2..=40u64 {
            let price = format!("99.{:02}", 25 + sequence);
            let images: Vec<_> = keys.iter().map(|k| image_of(k, sequence, &price)).collect();
            let posted = client
                .post(format!("{base}/api/v1/market/quotes"))
                .json(&serde_json::json!({ "events": images }))
                .send()
                .await
                .unwrap();
            assert!(posted.status().is_success(), "{}", posted.status());
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    // Walk one instrument a page until the ticking stops: every walk reads each bond once,
    // in ID order, and never a record older than one read before.
    let mut seen: BTreeMap<String, u64> = BTreeMap::new();
    let mut walks = 0;
    while !ticking.is_finished() || walks < 2 {
        let mut walked = Vec::new();
        let mut query = "?limit=1".to_owned();
        loop {
            let (status, body) = page(&server, &query).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            for record in body["results"].as_array().unwrap() {
                let id = record["result"]["provenance"]["subject"]["instrument_id"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                let revision = record["revision"].as_u64().unwrap();
                let before = seen.insert(id.clone(), revision).unwrap_or(0);
                assert!(
                    revision >= before,
                    "{id}: revision {revision} after {before}"
                );
                walked.push(id);
            }
            if body["more"] != true {
                break;
            }
            query = format!("?limit=1&after={}", body["next_after"].as_str().unwrap());
        }
        assert_eq!(walked, ["BOND-001", "BOND-W001", "BOND-W002"]);
        walks += 1;
    }
    ticking.await.unwrap();
    server.stop().await;
}

/// D103a: before P17a the access layer inspected this route's output and refused a
/// response naming more than 1,024 resources: about 170 records, at six each.
#[tokio::test]
async fn a_full_page_passes_an_access_policy() {
    const WIDTH: usize = 256;
    let issuer = identity::IssuerFixture::start().await;
    let mut policy = identity::policy();
    for grant in &mut policy.grants {
        for kind in [
            ResourceKind::Instrument,
            ResourceKind::Curve,
            ResourceKind::Source,
        ] {
            grant.resources.insert(kind, vec!["*".into()]);
        }
    }
    let access = AccessControl::new(issuer.verifier.clone(), policy).unwrap();
    let s = support::setup().await;
    let mut plan = s.plan.clone();
    support::widen(&s, &mut plan, WIDTH).await;
    let seed = plan.seed_events[0].clone();
    for pin in plan.assembly.instruments.iter().skip(1) {
        let mut event = seed.clone();
        event.key.instrument_id = convex_core::ids::InstrumentId::new(pin.instrument_id.as_str());
        plan.seed_events.push(event);
    }
    let engine = support::make_engine(
        &s,
        support::archive(&s),
        plan,
        s.store.clone(),
        convex_engine::RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let router = convex_server::routes::create_router_with_access(engine.clone(), false, access);
    let (status, _, body) = identity::request(
        router,
        "GET",
        "/api/v1/runtime/results",
        Some(identity::HUMAN_TOKEN),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), WIDTH);
    assert!(results.iter().all(|r| r["result"]["status"] == "Complete"));
    assert_eq!(body["more"], false);
    engine.shutdown().await.unwrap();
    issuer.finish().await;
}
