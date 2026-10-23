//! WebSocket integration tests for the Convex Server.
//!
//! These tests verify the full WebSocket message flow including:
//! - Connection establishment
//! - Valuation subscription management
//! - Refusal of the retired bond, ETF and portfolio subscriptions
//! - REST calculators sending nothing on the WebSocket
//! - Heartbeat (ping/pong)

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use convex_engine::ports::config::EngineConfig;
use convex_engine::ports::market_data::MarketDataProvider;
use convex_engine::ports::reference_data::ReferenceDataProvider;
use convex_engine::PricingEngineBuilder;
use convex_ext_file::{
    EmptyCurveInputSource, EmptyEtfHoldingsSource, EmptyEtfQuoteSource, EmptyFxRateSource,
    EmptyIndexFixingSource, EmptyInflationFixingSource, EmptyIssuerReferenceSource,
    EmptyQuoteSource, EmptyRatingSource, EmptyVolatilitySource,
};
fn create_router(engine: Arc<convex_engine::PricingEngine>) -> axum::Router {
    convex_server::routes::create_router_with_access(
        engine,
        true,
        convex_server::access::AccessControl::local_development(),
    )
}

#[path = "../../convex-ports/tests/support/reference_terms.rs"]
mod reference_terms;

async fn complete_batch_reference(request: &mut Value, engine: &convex_engine::PricingEngine) {
    use convex_core::contracts::{RequestId, UtcTimestamp};
    use convex_ports::reference_data::BondReferenceData;
    use convex_ports::reference_repository::ReferenceCommand;
    let mut resources = std::collections::BTreeMap::new();
    for item in request["bonds"].as_array_mut().unwrap() {
        let bond: BondReferenceData = serde_json::from_value(item["bond"].clone()).unwrap();
        let bond = reference_terms::complete(bond);
        for resource in reference_terms::resources(&bond) {
            resources.insert(resource.definition.key(), resource);
        }
        item["bond"] = serde_json::to_value(bond).unwrap();
    }
    let command = ReferenceCommand {
        idempotency_key: RequestId::new("websocket-fixture-resources").unwrap(),
        changes: Vec::new(),
        master_changes: resources.into_values().collect(),
    };
    engine
        .commit_reference(
            &command,
            UtcTimestamp::parse("2000-01-01T00:00:00Z").unwrap(),
        )
        .await
        .unwrap();
}

/// Create a test engine with empty providers.
fn create_test_engine() -> Arc<convex_engine::PricingEngine> {
    let market_data = MarketDataProvider {
        quotes: Arc::new(EmptyQuoteSource),
        curve_inputs: Arc::new(EmptyCurveInputSource),
        index_fixings: Arc::new(EmptyIndexFixingSource),
        volatility: Arc::new(EmptyVolatilitySource),
        fx_rates: Arc::new(EmptyFxRateSource),
        inflation_fixings: Arc::new(EmptyInflationFixingSource),
        etf_quotes: Arc::new(EmptyEtfQuoteSource),
    };

    let reference_data = ReferenceDataProvider {
        issuers: Arc::new(EmptyIssuerReferenceSource),
        ratings: Arc::new(EmptyRatingSource),
        etf_holdings: Arc::new(EmptyEtfHoldingsSource),
    };

    let storage = convex_ext_redb::create_test_storage().expect("Failed to create memory storage");

    let engine = PricingEngineBuilder::new()
        .with_config(EngineConfig::default())
        .with_market_data(Arc::new(market_data))
        .with_reference_data(Arc::new(reference_data))
        .with_storage(Arc::new(storage))
        .build()
        .expect("Failed to build engine");

    Arc::new(engine)
}

/// Start a test server on a random port and return the address.
async fn start_test_server() -> (SocketAddr, Arc<convex_engine::PricingEngine>) {
    let engine = create_test_engine();
    let router = create_router(engine.clone());

    // Bind to port 0 to get a random available port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Spawn server in background
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    (addr, engine)
}

/// Helper to receive and parse a JSON message with timeout.
async fn recv_json(
    ws: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> Option<Value> {
    match timeout(Duration::from_secs(5), ws.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str(&text).ok(),
        _ => None,
    }
}

/// Helper to receive a JSON message, skipping heartbeats.
async fn recv_json_skip_heartbeats(
    ws: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> Option<Value> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match timeout(remaining, ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                if let Ok(json) = serde_json::from_str::<Value>(&text) {
                    // Skip heartbeat messages
                    if json.get("type").and_then(|t| t.as_str()) != Some("heartbeat") {
                        return Some(json);
                    }
                }
            }
            _ => return None,
        }
    }
}

/// Assert that nothing but heartbeats arrives for `window`.
async fn assert_silent(
    ws: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    window: Duration,
) {
    let deadline = tokio::time::Instant::now() + window;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match timeout(remaining, ws.next()).await {
            Err(_) => return,
            Ok(Some(Ok(Message::Text(text)))) => {
                let json: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(json["type"], "heartbeat", "unexpected message: {json}");
            }
            Ok(other) => panic!("unexpected frame: {other:?}"),
        }
    }
}

// =============================================================================
// CONNECTION TESTS
// =============================================================================

#[tokio::test]
async fn test_websocket_connection_receives_connected_message() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut _write, mut read) = ws_stream.split();

    // First message should be "connected"
    let msg = recv_json(&mut read)
        .await
        .expect("Should receive connected message");

    assert_eq!(msg["type"], "connected");
    assert!(msg["session_id"].is_string());
    assert!(msg["server_time"].is_number());
}

#[tokio::test]
async fn test_websocket_connection_via_api_path() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/api/v1/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut _write, mut read) = ws_stream.split();

    let msg = recv_json(&mut read)
        .await
        .expect("Should receive connected message");
    assert_eq!(msg["type"], "connected");
}

// =============================================================================
// SUBSCRIPTION TESTS
// =============================================================================

#[tokio::test]
async fn test_websocket_subscribe_valuations() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    let subscribe_msg = json!({
        "type": "subscribe_valuations",
        "instrument_ids": ["US912828ZT09", "US912828ZQ69"]
    });
    write
        .send(Message::Text(subscribe_msg.to_string().into()))
        .await
        .unwrap();

    // Should receive subscribed confirmation
    let msg = recv_json_skip_heartbeats(&mut read)
        .await
        .expect("Should receive subscribed message");

    assert_eq!(msg["type"], "subscribed");
    assert_eq!(msg["subscription_type"], "valuations");
    assert_eq!(msg["count"], 2);
}

#[tokio::test]
async fn test_websocket_unsubscribe_valuations() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    // Subscribe first
    let subscribe_msg = json!({
        "type": "subscribe_valuations",
        "instrument_ids": ["BOND1", "BOND2"]
    });
    write
        .send(Message::Text(subscribe_msg.to_string().into()))
        .await
        .unwrap();
    let _ = recv_json_skip_heartbeats(&mut read).await;

    // Now unsubscribe
    let unsubscribe_msg = json!({
        "type": "unsubscribe_valuations",
        "instrument_ids": ["BOND1"]
    });
    write
        .send(Message::Text(unsubscribe_msg.to_string().into()))
        .await
        .unwrap();

    let msg = recv_json_skip_heartbeats(&mut read)
        .await
        .expect("Should receive unsubscribed message");

    assert_eq!(msg["type"], "unsubscribed");
    assert_eq!(msg["subscription_type"], "valuations");
}

/// The bond, ETF and portfolio subscriptions are retired with the REST broadcasts that fed them.
#[tokio::test]
async fn test_websocket_refuses_retired_subscriptions() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    for retired in [
        json!({"type": "subscribe_bonds", "instrument_ids": ["BOND1"]}),
        json!({"type": "unsubscribe_bonds", "instrument_ids": ["BOND1"]}),
        json!({"type": "subscribe_all_bonds"}),
        json!({"type": "unsubscribe_all_bonds"}),
        json!({"type": "subscribe_etfs", "etf_ids": ["LQD"]}),
        json!({"type": "unsubscribe_etfs", "etf_ids": ["LQD"]}),
        json!({"type": "subscribe_portfolios", "portfolio_ids": ["PORT001"]}),
        json!({"type": "unsubscribe_portfolios", "portfolio_ids": ["PORT001"]}),
    ] {
        write
            .send(Message::Text(retired.to_string().into()))
            .await
            .unwrap();
        let msg = recv_json_skip_heartbeats(&mut read)
            .await
            .expect("Should receive error message");
        assert_eq!(msg["type"], "error", "{retired}");
        assert_eq!(msg["code"], "INVALID_MESSAGE", "{retired}");
    }
}

// =============================================================================
// HEARTBEAT TESTS
// =============================================================================
// HEARTBEAT TESTS
// =============================================================================

#[tokio::test]
async fn test_websocket_ping_pong() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    // Send ping
    let timestamp = 1703980800000i64;
    let ping_msg = json!({
        "type": "ping",
        "timestamp": timestamp
    });
    write
        .send(Message::Text(ping_msg.to_string().into()))
        .await
        .unwrap();

    // Should receive pong with same timestamp
    let msg = recv_json_skip_heartbeats(&mut read)
        .await
        .expect("Should receive pong message");

    assert_eq!(msg["type"], "pong");
    assert_eq!(msg["timestamp"].as_i64().unwrap(), timestamp);
}

// =============================================================================
// REST CALCULATORS DO NOT BROADCAST
// =============================================================================

/// Pricing over REST returns the quote in the response only; the runtime results
/// channel is the only source of WebSocket data.
#[tokio::test]
async fn test_rest_pricing_sends_nothing_on_the_websocket() {
    let (addr, engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    let subscribe_msg = json!({
        "type": "subscribe_valuations",
        "instrument_ids": ["US912828ZT09"]
    });
    write
        .send(Message::Text(subscribe_msg.to_string().into()))
        .await
        .unwrap();
    let msg = recv_json_skip_heartbeats(&mut read).await.unwrap();
    assert_eq!(msg["type"], "subscribed");

    let client = reqwest::Client::new();
    let mut batch_request = json!({
        "bonds": [{
            "bond": {
                "instrument_id": "US912828ZT09",
                "isin": "US912828ZT09",
                "cusip": "912828ZT0",
                "description": "Treasury 2.5% 2030",
                "currency": "USD",
                "issue_date": "2020-01-15",
                "maturity_date": "2030-01-15",
                "coupon_rate": "0.025",
                "frequency": 2,
                "day_count": "ACT/ACT",
                "face_value": "100",
                "bond_type": "FixedBullet",
                "issuer_type": "Sovereign",
                "issuer_id": "US_TREASURY",
                "issuer_name": "US Treasury",
                "seniority": "Senior",
                "is_callable": false,
                "call_schedule": [],
                "is_putable": false,
                "is_sinkable": false,
                "has_deflation_floor": false,
                "country_of_risk": "US",
                "sector": "Sovereign",
                "last_updated": 0,
                "source": "test"
            },
            "market_price": "98.50"
        }],
        "settlement_date": "2024-01-15"
    });

    complete_batch_reference(&mut batch_request, &engine).await;
    let response = client
        .post(format!("http://{}/api/v1/batch/price", addr))
        .json(&batch_request)
        .send()
        .await
        .expect("Batch price request failed");
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert!(status.is_success(), "{status}: {body}");
    assert_eq!(body["quotes"][0]["instrument_id"], "US912828ZT09", "{body}");

    assert_silent(&mut read, Duration::from_millis(500)).await;
}

// =============================================================================
// ERROR HANDLING TESTS
// =============================================================================
// ERROR HANDLING TESTS
// =============================================================================

#[tokio::test]
async fn test_websocket_invalid_message_format() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    // Send invalid JSON
    write
        .send(Message::Text("not valid json".to_string().into()))
        .await
        .unwrap();

    // Should receive error message
    let msg = recv_json_skip_heartbeats(&mut read)
        .await
        .expect("Should receive error message");

    assert_eq!(msg["type"], "error");
    assert!(msg["message"].is_string());
}

#[tokio::test]
async fn test_websocket_unknown_message_type() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    // Send unknown message type
    let unknown_msg = json!({
        "type": "unknown_type",
        "data": "test"
    });
    write
        .send(Message::Text(unknown_msg.to_string().into()))
        .await
        .unwrap();

    // Should receive error message
    let msg = recv_json_skip_heartbeats(&mut read)
        .await
        .expect("Should receive error message");

    assert_eq!(msg["type"], "error");
}

// =============================================================================
// STATUS ENDPOINT TESTS
// =============================================================================

#[tokio::test]
async fn test_websocket_status_shows_active_connections() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    // Check initial status - 0 connections
    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://{}/api/v1/ws/status", addr))
        .send()
        .await
        .unwrap();
    let status: Value = response.json().await.unwrap();
    assert_eq!(status["active_connections"].as_u64().unwrap(), 0);

    // Connect a WebSocket client
    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (_write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    // Give server time to update connection count
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Check status again - should show 1 connection
    let response = client
        .get(format!("http://{}/api/v1/ws/status", addr))
        .send()
        .await
        .unwrap();
    let status: Value = response.json().await.unwrap();
    assert_eq!(status["active_connections"].as_u64().unwrap(), 1);
}
