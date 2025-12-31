//! WebSocket integration tests for the Convex Server.
//!
//! These tests verify the full WebSocket message flow including:
//! - Connection establishment
//! - Subscription management
//! - Broadcast updates from pricing operations
//! - Heartbeat (ping/pong)

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use convex_engine::PricingEngineBuilder;
use convex_ext_file::{
    create_empty_output, EmptyBondReferenceSource, EmptyCurveInputSource, EmptyEtfHoldingsSource,
    EmptyEtfQuoteSource, EmptyFxRateSource, EmptyIndexFixingSource, EmptyInflationFixingSource,
    EmptyIssuerReferenceSource, EmptyQuoteSource, EmptyRatingSource, EmptyVolatilitySource,
};
use convex_server::routes::create_router;
use convex_traits::config::EngineConfig;
use convex_traits::market_data::MarketDataProvider;
use convex_traits::reference_data::ReferenceDataProvider;

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
        bonds: Arc::new(EmptyBondReferenceSource),
        issuers: Arc::new(EmptyIssuerReferenceSource),
        ratings: Arc::new(EmptyRatingSource),
        etf_holdings: Arc::new(EmptyEtfHoldingsSource),
    };

    let output = create_empty_output();
    let storage =
        convex_ext_redb::create_memory_storage().expect("Failed to create memory storage");

    let engine = PricingEngineBuilder::new()
        .with_config(EngineConfig::default())
        .with_market_data(Arc::new(market_data))
        .with_reference_data(Arc::new(reference_data))
        .with_storage(Arc::new(storage))
        .with_output(Arc::new(output))
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
async fn test_websocket_subscribe_bonds() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    // Subscribe to bonds
    let subscribe_msg = json!({
        "type": "subscribe_bonds",
        "instrument_ids": ["US912828ZT09", "US912828ZQ69"]
    });
    write
        .send(Message::Text(subscribe_msg.to_string()))
        .await
        .unwrap();

    // Should receive subscribed confirmation
    let msg = recv_json_skip_heartbeats(&mut read)
        .await
        .expect("Should receive subscribed message");

    assert_eq!(msg["type"], "subscribed");
    assert_eq!(msg["subscription_type"], "bonds");
    assert_eq!(msg["count"], 2);
}

#[tokio::test]
async fn test_websocket_subscribe_all_bonds() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    // Subscribe to all bonds
    let subscribe_msg = json!({ "type": "subscribe_all_bonds" });
    write
        .send(Message::Text(subscribe_msg.to_string()))
        .await
        .unwrap();

    // Should receive subscribed confirmation
    let msg = recv_json_skip_heartbeats(&mut read)
        .await
        .expect("Should receive subscribed message");

    assert_eq!(msg["type"], "subscribed");
    assert_eq!(msg["subscription_type"], "all_bonds");
}

#[tokio::test]
async fn test_websocket_subscribe_etfs() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    // Subscribe to ETFs
    let subscribe_msg = json!({
        "type": "subscribe_etfs",
        "etf_ids": ["LQD", "HYG"]
    });
    write
        .send(Message::Text(subscribe_msg.to_string()))
        .await
        .unwrap();

    let msg = recv_json_skip_heartbeats(&mut read)
        .await
        .expect("Should receive subscribed message");

    assert_eq!(msg["type"], "subscribed");
    assert_eq!(msg["subscription_type"], "etfs");
}

#[tokio::test]
async fn test_websocket_subscribe_portfolios() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    // Subscribe to portfolios
    let subscribe_msg = json!({
        "type": "subscribe_portfolios",
        "portfolio_ids": ["PORT001", "PORT002"]
    });
    write
        .send(Message::Text(subscribe_msg.to_string()))
        .await
        .unwrap();

    let msg = recv_json_skip_heartbeats(&mut read)
        .await
        .expect("Should receive subscribed message");

    assert_eq!(msg["type"], "subscribed");
    assert_eq!(msg["subscription_type"], "portfolios");
}

#[tokio::test]
async fn test_websocket_unsubscribe_bonds() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    // Subscribe first
    let subscribe_msg = json!({
        "type": "subscribe_bonds",
        "instrument_ids": ["BOND1", "BOND2"]
    });
    write
        .send(Message::Text(subscribe_msg.to_string()))
        .await
        .unwrap();
    let _ = recv_json_skip_heartbeats(&mut read).await;

    // Now unsubscribe
    let unsubscribe_msg = json!({
        "type": "unsubscribe_bonds",
        "instrument_ids": ["BOND1"]
    });
    write
        .send(Message::Text(unsubscribe_msg.to_string()))
        .await
        .unwrap();

    let msg = recv_json_skip_heartbeats(&mut read)
        .await
        .expect("Should receive unsubscribed message");

    assert_eq!(msg["type"], "unsubscribed");
    assert_eq!(msg["subscription_type"], "bonds");
}

#[tokio::test]
async fn test_websocket_unsubscribe_all_bonds() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    // Subscribe to all bonds first
    let subscribe_msg = json!({ "type": "subscribe_all_bonds" });
    write
        .send(Message::Text(subscribe_msg.to_string()))
        .await
        .unwrap();
    let _ = recv_json_skip_heartbeats(&mut read).await;

    // Unsubscribe from all bonds
    let unsubscribe_msg = json!({ "type": "unsubscribe_all_bonds" });
    write
        .send(Message::Text(unsubscribe_msg.to_string()))
        .await
        .unwrap();

    let msg = recv_json_skip_heartbeats(&mut read)
        .await
        .expect("Should receive unsubscribed message");

    assert_eq!(msg["type"], "unsubscribed");
    assert_eq!(msg["subscription_type"], "all_bonds");
}

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
        .send(Message::Text(ping_msg.to_string()))
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
// BROADCAST TESTS
// =============================================================================

#[tokio::test]
async fn test_websocket_receives_bond_quote_broadcast() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    // Connect WebSocket client
    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    // Subscribe to all bonds to receive broadcasts
    let subscribe_msg = json!({ "type": "subscribe_all_bonds" });
    write
        .send(Message::Text(subscribe_msg.to_string()))
        .await
        .unwrap();
    let _ = recv_json_skip_heartbeats(&mut read).await;

    // Trigger batch pricing via REST API
    let client = reqwest::Client::new();
    let batch_request = json!({
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

    let response = client
        .post(&format!("http://{}/api/v1/batch/price", addr))
        .json(&batch_request)
        .send()
        .await
        .expect("Batch price request failed");

    // Ensure the request succeeded
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        panic!(
            "Batch price request returned error status {}: {}",
            status, body
        );
    }

    // Give time for broadcast to propagate
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should receive bond quote broadcast
    let msg = recv_json_skip_heartbeats(&mut read)
        .await
        .expect("Should receive bond_quote broadcast");

    assert_eq!(msg["type"], "bond_quote");
    assert!(msg["instrument_id"].is_string());
}

#[tokio::test]
async fn test_websocket_receives_filtered_bond_quote() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    // Connect WebSocket client
    let (ws_stream, _) = connect_async(&ws_url).await.expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // Consume connected message
    let _ = recv_json(&mut read).await;

    // Subscribe only to specific bond
    let subscribe_msg = json!({
        "type": "subscribe_bonds",
        "instrument_ids": ["US912828ZT09"]
    });
    write
        .send(Message::Text(subscribe_msg.to_string()))
        .await
        .unwrap();
    let _ = recv_json_skip_heartbeats(&mut read).await;

    // Trigger batch pricing for subscribed bond
    let client = reqwest::Client::new();
    let batch_request = json!({
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

    let response = client
        .post(&format!("http://{}/api/v1/batch/price", addr))
        .json(&batch_request)
        .send()
        .await
        .expect("Batch price request failed");

    // Ensure the request succeeded
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        panic!(
            "Batch price request returned error status {}: {}",
            status, body
        );
    }

    // Give time for broadcast to propagate
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should receive bond quote broadcast for subscribed instrument
    let msg = recv_json_skip_heartbeats(&mut read)
        .await
        .expect("Should receive bond_quote broadcast");

    assert_eq!(msg["type"], "bond_quote");
    assert_eq!(msg["instrument_id"], "US912828ZT09");
}

// =============================================================================
// MULTIPLE CLIENT TESTS
// =============================================================================

#[tokio::test]
async fn test_websocket_multiple_clients_receive_broadcast() {
    let (addr, _engine) = start_test_server().await;
    let ws_url = format!("ws://{}/ws", addr);

    // Connect two WebSocket clients
    let (ws_stream1, _) = connect_async(&ws_url)
        .await
        .expect("Failed to connect client 1");
    let (mut write1, mut read1) = ws_stream1.split();

    let (ws_stream2, _) = connect_async(&ws_url)
        .await
        .expect("Failed to connect client 2");
    let (mut write2, mut read2) = ws_stream2.split();

    // Consume connected messages
    let msg1 = recv_json(&mut read1).await.unwrap();
    let msg2 = recv_json(&mut read2).await.unwrap();
    assert_eq!(msg1["type"], "connected");
    assert_eq!(msg2["type"], "connected");

    // Both subscribe to all bonds
    let subscribe_msg = json!({ "type": "subscribe_all_bonds" });
    write1
        .send(Message::Text(subscribe_msg.to_string()))
        .await
        .unwrap();
    write2
        .send(Message::Text(subscribe_msg.to_string()))
        .await
        .unwrap();

    // Consume subscribed confirmations
    let _ = recv_json_skip_heartbeats(&mut read1).await;
    let _ = recv_json_skip_heartbeats(&mut read2).await;

    // Trigger batch pricing
    let client = reqwest::Client::new();
    let batch_request = json!({
        "bonds": [{
            "bond": {
                "instrument_id": "MULTI_TEST",
                "isin": "MULTI_TEST",
                "cusip": "MULTI",
                "description": "Multi Client Test Bond",
                "currency": "USD",
                "issue_date": "2020-01-15",
                "maturity_date": "2030-01-15",
                "coupon_rate": "0.03",
                "frequency": 2,
                "day_count": "30/360",
                "face_value": "100",
                "bond_type": "FixedBullet",
                "issuer_type": "CorporateIG",
                "issuer_id": "TEST",
                "issuer_name": "Test Corp",
                "seniority": "Senior",
                "is_callable": false,
                "call_schedule": [],
                "is_putable": false,
                "is_sinkable": false,
                "has_deflation_floor": false,
                "country_of_risk": "US",
                "sector": "Financials",
                "last_updated": 0,
                "source": "test"
            },
            "market_price": "99.00"
        }],
        "settlement_date": "2024-01-15"
    });

    let response = client
        .post(&format!("http://{}/api/v1/batch/price", addr))
        .json(&batch_request)
        .send()
        .await
        .expect("Batch price request failed");

    // Ensure the request succeeded
    assert!(
        response.status().is_success(),
        "Batch price request returned error status"
    );

    // Give time for broadcast to propagate
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Both clients should receive the broadcast
    let broadcast1 = recv_json_skip_heartbeats(&mut read1)
        .await
        .expect("Client 1 should receive broadcast");
    let broadcast2 = recv_json_skip_heartbeats(&mut read2)
        .await
        .expect("Client 2 should receive broadcast");

    assert_eq!(broadcast1["type"], "bond_quote");
    assert_eq!(broadcast2["type"], "bond_quote");
    assert_eq!(broadcast1["instrument_id"], "MULTI_TEST");
    assert_eq!(broadcast2["instrument_id"], "MULTI_TEST");
}

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
        .send(Message::Text("not valid json".to_string()))
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
        .send(Message::Text(unknown_msg.to_string()))
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
        .get(&format!("http://{}/api/v1/ws/status", addr))
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
        .get(&format!("http://{}/api/v1/ws/status", addr))
        .send()
        .await
        .unwrap();
    let status: Value = response.json().await.unwrap();
    assert_eq!(status["active_connections"].as_u64().unwrap(), 1);
}
