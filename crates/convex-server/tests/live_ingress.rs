//! The `live` profile over the real composition and HTTP: canonical quotes posted to
//! `/api/v1/market/quotes` price through the Live runtime, and a restart waits for
//! fresh images instead of restoring stored ticks.
#[path = "support/live_server.rs"]
mod live_server;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use convex_engine::ports::quote_event::QuoteEvent;
use convex_server::ServerProfile;
use live_server::*;
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::time::Duration;

#[tokio::test]
async fn posted_quotes_price_and_a_restart_waits_for_fresh_images() {
    let (dir, database, live) = live_setup().await;
    let server = Running::start(live.clone()).await;
    let runtime = server.runtime().await;
    assert_eq!(runtime["mode"], "live", "{runtime}");
    assert_eq!(runtime["ready"], false);

    // A body that is not a quote batch is 400; 422 is reserved for row refusals.
    for body in [
        json!({"events": [{"key": 1}]}),
        json!({"events": [], "extra": 1}),
    ] {
        let malformed = server
            .client
            .post(format!("{}/api/v1/market/quotes", server.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(malformed.status(), StatusCode::BAD_REQUEST, "{body}");
    }
    let untyped = server
        .client
        .post(format!("{}/api/v1/market/quotes", server.base))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(untyped.status(), StatusCode::BAD_REQUEST);

    let mut other = image(1, "99.25");
    other.key.source = "feed-b".into();
    let rejected = server.post_quotes(vec![image(1, "99.25"), other]).await;
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = rejected.json().await.unwrap();
    assert_eq!(body["rows"][0]["index"], 1);
    assert_eq!(body["rows"][0]["code"], "UNKNOWN_KEY");
    assert_eq!(server.runtime().await["ready"], false);

    let accepted = server.post_quotes(vec![image(1, "99.25")]).await;
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let receipt: Value = accepted.json().await.unwrap();
    assert_eq!(receipt["accepted"], 1);
    let first = server.wait_ready().await;
    let ready = server
        .client
        .get(format!("{}/ready", server.base))
        .send()
        .await
        .unwrap();
    assert_eq!(ready.status(), StatusCode::OK);
    let results: Vec<Value> = server
        .client
        .get(format!("{}/api/v1/runtime/results", server.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0]["live_valid_until"].is_string(), "{}", results[0]);
    server.stop().await;

    let server = Running::start(live).await;
    let restored = server.runtime().await;
    assert_eq!(restored["ready"], false, "{restored}");
    assert!(restored["revision"].as_u64() > first["revision"].as_u64());
    let accepted = server.post_quotes(vec![image(1, "99.50")]).await;
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let fresh = server.wait_ready().await;
    assert!(fresh["revision"].as_u64() > restored["revision"].as_u64());
    server.stop().await;
    // Live stores no market snapshot and archives no result.
    let stored = convex_ext_redb::open_redb_storage(&database).unwrap();
    assert_eq!(stored.snapshots.market_snapshot_count().unwrap(), 0);
    assert!(stored.results.is_empty().unwrap());
    drop(dir);
}

/// Quotes offered faster than the runtime values them end in 429 at the full ingress
/// queue, never in lost delivery: every accepted quote is valued and the runtime stays up.
/// A tick admits up to 1,024 queued quotes (D54) and values each changed bond once (D55),
/// so at 256 bonds one client posting batch after batch can keep pace with the drain.
/// Each round therefore posts `ROUND` batches at once, four times what the queue holds,
/// so a tick that drains the queue mid-round still leaves more than it holds. A key's
/// images can then reach the runtime out of order; each is admitted, and an older one
/// is dropped as out of order without blocking the key.
#[tokio::test]
async fn an_offered_rate_above_the_drain_rate_is_refused_with_429_and_nothing_is_lost() {
    const ROUND: usize = 16;
    let (dir, _, live, keys) = wide_live_setup(256).await;
    let server = Running::start(live).await;
    let mut sequence = 0;
    let mut batch = |size: usize| -> Vec<QuoteEvent> {
        (0..size)
            .map(|_| {
                let mut event = image(sequence + 1, "99.25");
                event.key = keys[sequence as usize % keys.len()].clone();
                sequence += 1;
                event
            })
            .collect()
    };
    // Every bond needs its first image before the runtime is ready.
    let accepted = server.post_quotes(batch(keys.len())).await;
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let start = server.wait_ready().await["generation"].as_u64().unwrap();
    let mut queued = 0;
    let mut refused = Vec::new();
    for _ in 0..16 {
        let batches: Vec<_> = (0..ROUND).map(|_| batch(256)).collect();
        let responses =
            futures::future::join_all(batches.into_iter().map(|b| server.post_quotes(b))).await;
        for response in responses {
            match response.status() {
                StatusCode::ACCEPTED => {
                    let receipt: Value = response.json().await.unwrap();
                    queued += receipt["accepted"].as_u64().unwrap();
                }
                StatusCode::TOO_MANY_REQUESTS => {
                    refused.push(response.json::<Value>().await.unwrap());
                }
                other => panic!("unexpected status {other}"),
            }
        }
        if !refused.is_empty() {
            break;
        }
    }
    assert!(!refused.is_empty(), "a full queue refuses a batch");
    for body in &refused {
        assert_eq!(body["code"], "CAPACITY_EXCEEDED", "{body}");
        assert_eq!(body["retryable"], true, "{body}");
    }
    let drained = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let runtime = server.runtime().await;
            assert!(runtime["failure"].is_null(), "{runtime}");
            if runtime["generation"].as_u64() == Some(start + queued) && runtime["ready"] == true {
                return runtime;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("every accepted quote is valued");
    assert_eq!(drained["phase"], "ready", "{drained}");
    server.stop().await;
    drop(dir);
}

#[tokio::test]
async fn a_server_without_a_live_runtime_refuses_quotes() {
    let directory = tempfile::tempdir().unwrap();
    let server = Running::start(config(
        ServerProfile::Demo,
        &directory.path().join("state.redb"),
        None,
    ))
    .await;
    let response = server.post_quotes(vec![image(1, "99.25")]).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    server.stop().await;
}
