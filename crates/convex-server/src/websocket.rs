//! WebSocket streaming of canonical runtime results.
//!
//! The runtime results channel (`PricingEngine::subscribe_results`) is the only
//! source of streamed data. A client opts in with `subscribe_valuations` and
//! receives a `valuation` message for each published result of those instruments.
//! REST calculators return their results in the response and never broadcast.
//! Heartbeat and ping/pong keep the connection healthy.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::Extension;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::time::interval;
use tracing::{error, info, warn};

use convex_core::ids::InstrumentId;

use crate::access::http::WsAccess;
use crate::access::{AccessError, Action, DataScope, Resource, ResourceKind};
use crate::handlers::AppState;
use crate::ServerProfile;

// =============================================================================
// MESSAGE TYPES
// =============================================================================

/// Inbound WebSocket message from client. Any other `type`, including the retired
/// bond, ETF and portfolio subscriptions, is refused with `INVALID_MESSAGE`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[allow(missing_docs)]
pub enum ClientMessage {
    /// Opt in to canonical snapshot-valued results.
    SubscribeValuations { instrument_ids: Vec<String> },
    /// Stop canonical valuation updates.
    UnsubscribeValuations { instrument_ids: Vec<String> },
    /// Ping (client heartbeat)
    Ping { timestamp: i64 },
}

/// Outbound WebSocket message to client.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum ServerMessage {
    /// Connection established
    Connected {
        session_id: String,
        server_time: i64,
    },
    /// Subscription confirmed
    Subscribed {
        subscription_type: String,
        count: usize,
    },
    /// Unsubscription confirmed
    Unsubscribed {
        subscription_type: String,
        count: usize,
    },
    /// Pong (server heartbeat response)
    Pong { timestamp: i64, server_time: i64 },
    /// Error message
    Error { code: String, message: String },
    /// Server heartbeat
    Heartbeat { server_time: i64 },
    /// Canonical durable result state from the pricing runtime.
    Valuation(Box<convex_engine::ports::runtime::PublishedValuation>),
}

// =============================================================================
// WEBSOCKET STATE
// =============================================================================

/// WebSocket connection state manager.
pub struct WebSocketState {
    /// Active connections count
    pub connection_count: std::sync::atomic::AtomicUsize,
    /// Session ID counter
    session_counter: std::sync::atomic::AtomicU64,
}

impl WebSocketState {
    /// Create a new WebSocket state manager.
    pub fn new() -> Self {
        Self {
            connection_count: std::sync::atomic::AtomicUsize::new(0),
            session_counter: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Generate a new session ID.
    pub fn next_session_id(&self) -> String {
        let id = self
            .session_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        format!("ws-{}", id)
    }

    /// Get active connection count.
    pub fn active_connections(&self) -> usize {
        self.connection_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for WebSocketState {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// CLIENT SESSION
// =============================================================================

/// Per-client subscription state.
#[derive(Default)]
struct ClientSession {
    valuation_subscriptions: HashSet<InstrumentId>,
}

impl ClientSession {
    fn topic_count(&self) -> usize {
        self.valuation_subscriptions.len()
    }
}

// =============================================================================
// WEBSOCKET HANDLER
// =============================================================================

/// WebSocket upgrade handler.
pub(crate) async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    profile: Option<Extension<ServerProfile>>,
    authorization: Option<Extension<WsAccess>>,
) -> impl IntoResponse {
    let profile = profile.map_or(ServerProfile::Production, |Extension(profile)| profile);
    let Some(Extension(authorization)) = authorization else {
        return crate::access::http::response(AccessError::Unauthenticated);
    };
    ws.max_message_size(65536)
        .max_frame_size(65536)
        .on_upgrade(move |socket| handle_socket(socket, state, profile, authorization))
        .into_response()
}

/// Handle an individual WebSocket connection.
async fn handle_socket(
    socket: WebSocket,
    state: Arc<AppState>,
    profile: ServerProfile,
    mut authorization: WsAccess,
) {
    let ws_state = &state.ws_state;

    // Generate session ID
    let session_id = ws_state.next_session_id();
    info!("WebSocket connection established: {}", session_id);

    // Increment connection count
    ws_state
        .connection_count
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    // Create client session
    let mut session = ClientSession::default();

    // The runtime results channel is the only source of streamed messages.
    let mut valuation_rx = state.engine.subscribe_results();
    let mut shutdown_rx = state.engine.subscribe_shutdown();

    // Split socket into sender and receiver
    let (mut sender, mut receiver) = socket.split();

    // Send connected message
    let connected_msg = ServerMessage::Connected {
        session_id: session_id.clone(),
        server_time: current_timestamp(),
    };
    if send_message(&mut sender, &connected_msg, profile, &authorization)
        .await
        .is_err()
    {
        error!(code = "WS_SEND_FAILED", "Connected frame could not be sent");
        ws_state
            .connection_count
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        return;
    }

    let mut policy_changes = authorization.access.subscribe_policy();
    let mut auth_timer = interval(Duration::from_millis(250));
    auth_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Heartbeat interval
    let mut heartbeat_interval = interval(Duration::from_secs(30));

    loop {
        tokio::select! {
            biased;
            changed = policy_changes.changed() => {
                if changed.is_err() || session_authorized(&session, &authorization).is_err() { let _ = tokio::time::timeout(Duration::from_secs(2), sender.send(Message::Close(None))).await; break; }
            }
            _ = auth_timer.tick() => {
                if session_authorized(&session, &authorization).is_err() { let _ = tokio::time::timeout(Duration::from_secs(2), sender.send(Message::Close(None))).await; break; }
                if !authorization.access.is_local_development() && authorization.access.session_needs_refresh(&authorization.context) {
                    match authorization.access.refresh(&authorization.context).await {
                        Ok(context) => authorization.context = context,
                        Err(_) => { let _ = tokio::time::timeout(Duration::from_secs(2), sender.send(Message::Close(None))).await; break; }
                    }
                    if session_authorized(&session, &authorization).is_err() { let _ = tokio::time::timeout(Duration::from_secs(2), sender.send(Message::Close(None))).await; break; }
                }
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() { let _ = tokio::time::timeout(Duration::from_secs(2), sender.send(Message::Close(None))).await; break; }
            }
            result = valuation_rx.recv() => {
                match result {
                    Ok(result) => {
                        let subscribed = match &result.result.provenance().subject {
                            convex_core::contracts::ValuationSubject::InstrumentVersion { instrument_id, .. } => session.valuation_subscriptions.contains(&InstrumentId::new(instrument_id.as_str())),
                            _ => false,
                        };
                        if subscribed && send_message(&mut sender, &ServerMessage::Valuation(Box::new(result)), profile, &authorization).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if send_message(&mut sender, &ServerMessage::Error { code: "RESYNC_REQUIRED".into(), message: "Read /api/v1/runtime/results and resume by (runtime_id, revision)".into() }, profile, &authorization).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // Handle incoming messages from client
            message = receiver.next() => {
                let Some(msg) = message else { break; };
                let admission = match authorization.access.admit_message(&authorization.context) { Ok(admission) => admission, Err(_) => { let _ = tokio::time::timeout(Duration::from_secs(2), sender.send(Message::Close(None))).await; break; } };
                match msg {
                    Ok(Message::Text(text)) => {
                        if handle_client_payload(Ok(&text), &mut session, &mut sender, profile, &authorization).await.is_err() { break; }
                    }
                    Ok(Message::Binary(data)) => {
                        let text = std::str::from_utf8(&data).map_err(|_| "INVALID_MESSAGE".to_owned());
                        if handle_client_payload(text, &mut session, &mut sender, profile, &authorization).await.is_err() { break; }
                    }
                    Ok(Message::Ping(data)) => {
                        if !tokio::time::timeout(Duration::from_secs(2), sender.send(Message::Pong(data))).await.is_ok_and(|r| r.is_ok()) {
                            break;
                        }
                    }
                    Ok(Message::Pong(_)) => {
                        // Client responded to our ping
                    }
                    Ok(Message::Close(_)) => {
                        info!("WebSocket closed by client: {}", session_id);
                        break;
                    }
                    Err(_) => {
                        error!(code = "WS_TRANSPORT_FAILED", "WebSocket transport failed");
                        break;
                    }
                }
                drop(admission);
            }

            // Send heartbeat
            _ = heartbeat_interval.tick() => {
                let heartbeat = ServerMessage::Heartbeat {
                    server_time: current_timestamp(),
                };
                if send_message(&mut sender, &heartbeat, profile, &authorization).await.is_err() {
                    break;
                }
            }
        }
    }

    // Cleanup
    ws_state
        .connection_count
        .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    info!("WebSocket connection closed: {}", session_id);
}

/// Identical safe denial semantics for text and UTF-8 binary frames.
async fn handle_client_payload(
    text: Result<&str, String>,
    session: &mut ClientSession,
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    profile: ServerProfile,
    authorization: &WsAccess,
) -> Result<(), axum::Error> {
    let result = match text {
        Ok(text) => handle_client_message(text, session, sender, profile, authorization).await,
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        let code = if [
            "FORBIDDEN",
            "UNAUTHENTICATED",
            "ACCESS_UNAVAILABLE",
            "ACCESS_QUOTA_EXCEEDED",
            "INVALID_ACCESS_REQUEST",
        ]
        .contains(&error.as_str())
        {
            error.clone()
        } else {
            "INVALID_MESSAGE".into()
        };
        warn!(code = %code, "WebSocket message rejected");
        send_message(
            sender,
            &ServerMessage::Error {
                code,
                message: error,
            },
            profile,
            authorization,
        )
        .await?;
    }
    Ok(())
}

/// Handle a client message.
async fn handle_client_message(
    text: &str,
    session: &mut ClientSession,
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    profile: ServerProfile,
    authorization: &WsAccess,
) -> Result<(), String> {
    let msg: ClientMessage =
        serde_json::from_str(text).map_err(|_| "INVALID_MESSAGE".to_owned())?;

    authorize_subscription(&msg, session, authorization).map_err(|error| error.to_string())?;
    let response = match msg {
        ClientMessage::SubscribeValuations { instrument_ids } => {
            if instrument_ids.len() > 256
                || session
                    .valuation_subscriptions
                    .len()
                    .saturating_add(instrument_ids.len())
                    > 256
            {
                return Err("at most 256 valuation subscriptions".into());
            }
            for id in &instrument_ids {
                convex_core::contracts::InstrumentId::new(id)
                    .map_err(|_| "invalid valuation identity")?;
            }
            let count = instrument_ids.len();
            session
                .valuation_subscriptions
                .extend(instrument_ids.iter().map(InstrumentId::new));
            ServerMessage::Subscribed {
                subscription_type: "valuations".into(),
                count,
            }
        }
        ClientMessage::UnsubscribeValuations { instrument_ids } => {
            if instrument_ids.len() > 256 {
                return Err("at most 256 valuation subscriptions".into());
            }
            let count = instrument_ids.len();
            for id in &instrument_ids {
                session
                    .valuation_subscriptions
                    .remove(&InstrumentId::new(id));
            }
            ServerMessage::Unsubscribed {
                subscription_type: "valuations".into(),
                count,
            }
        }
        ClientMessage::Ping { timestamp } => ServerMessage::Pong {
            timestamp,
            server_time: current_timestamp(),
        },
    };

    send_message(sender, &response, profile, authorization)
        .await
        .map_err(|_| "WS_SEND_FAILED".to_owned())
}

/// A valuation topic needs result, reference and market read on the instrument.
fn valuation_permission(id: &str, authorization: &WsAccess) -> Result<(), AccessError> {
    authorization.access.authorize(
        &authorization.context,
        Action::ResultRead,
        &[DataScope::Reference, DataScope::Market, DataScope::Results],
        &[
            Resource::new(ResourceKind::Instrument, id),
            Resource::new(ResourceKind::Source, "*"),
            Resource::new(ResourceKind::Curve, "*"),
        ],
    )
}
fn session_authorized(
    session: &ClientSession,
    authorization: &WsAccess,
) -> Result<(), AccessError> {
    authorization
        .access
        .authorize(&authorization.context, Action::Stream, &[], &[])?;
    if session.topic_count()
        > authorization
            .access
            .subscription_limit(&authorization.context)?
    {
        return Err(AccessError::Quota);
    }
    for id in &session.valuation_subscriptions {
        valuation_permission(id.as_str(), authorization)?;
    }
    Ok(())
}
fn authorize_subscription(
    message: &ClientMessage,
    session: &ClientSession,
    authorization: &WsAccess,
) -> Result<(), AccessError> {
    authorization
        .access
        .authorize(&authorization.context, Action::Stream, &[], &[])?;
    let ids = match message {
        ClientMessage::SubscribeValuations { instrument_ids } => instrument_ids,
        ClientMessage::UnsubscribeValuations { instrument_ids } => {
            if instrument_ids.len() > 256 {
                return Err(AccessError::Quota);
            }
            return Ok(());
        }
        ClientMessage::Ping { .. } => return Ok(()),
    };
    if ids.len() > 256 {
        return Err(AccessError::Quota);
    }
    let mut prospective: HashSet<String> = session
        .valuation_subscriptions
        .iter()
        .map(ToString::to_string)
        .collect();
    let before = prospective.len();
    for id in ids {
        if !crate::access::policy::valid_id(id) || id == "*" {
            return Err(AccessError::InvalidRequest);
        }
        valuation_permission(id, authorization)?;
        prospective.insert(id.clone());
    }
    if session
        .topic_count()
        .saturating_add(prospective.len().saturating_sub(before))
        > authorization
            .access
            .subscription_limit(&authorization.context)?
    {
        return Err(AccessError::Quota);
    }
    Ok(())
}
fn authorize_delivery(
    message: &ServerMessage,
    authorization: &WsAccess,
) -> Result<(), AccessError> {
    authorization
        .access
        .authorize(&authorization.context, Action::Stream, &[], &[])?;
    if !authorization.access.session_fresh(&authorization.context) {
        return Err(AccessError::Unauthenticated);
    }
    if let ServerMessage::Valuation(result) = message {
        match &result.result.provenance().subject {
            convex_core::contracts::ValuationSubject::InstrumentVersion {
                instrument_id, ..
            } => valuation_permission(instrument_id.as_str(), authorization)?,
            _ => return Err(AccessError::Forbidden),
        }
    }
    Ok(())
}

/// Send a server message over WebSocket.
async fn send_message(
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    msg: &ServerMessage,
    profile: ServerProfile,
    authorization: &WsAccess,
) -> Result<(), axum::Error> {
    authorize_delivery(msg, authorization).map_err(axum::Error::new)?;
    let json = serialize_message(msg, profile).map_err(axum::Error::new)?;
    if json.len() > 65536 {
        return Err(axum::Error::new(AccessError::Quota));
    }
    tokio::time::timeout(Duration::from_secs(2), sender.send(Message::Text(json)))
        .await
        .map_err(axum::Error::new)?
        .map_err(axum::Error::new)
}

fn serialize_message(
    msg: &ServerMessage,
    profile: ServerProfile,
) -> Result<String, serde_json::Error> {
    #[derive(Serialize)]
    struct WireMessage<'a> {
        #[serde(flatten)]
        message: &'a ServerMessage,
        #[serde(skip_serializing_if = "Option::is_none")]
        profile: Option<ServerProfile>,
        #[serde(skip_serializing_if = "Option::is_none")]
        data_quality: Option<&'static str>,
    }
    let demo = profile == ServerProfile::Demo;
    serde_json::to_string(&WireMessage {
        message: msg,
        profile: demo.then_some(profile),
        data_quality: demo.then_some("demo"),
    })
}

/// Get current timestamp in milliseconds.
fn current_timestamp() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// =============================================================================
// WEBSOCKET STATUS ENDPOINT
// =============================================================================

/// Response for WebSocket status endpoint.
#[derive(Serialize)]
pub struct WebSocketStatus {
    /// Number of active connections
    pub active_connections: usize,
    /// Server uptime in seconds
    pub uptime_seconds: u64,
}

/// Get WebSocket status.
pub async fn ws_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let status = WebSocketStatus {
        active_connections: state.ws_state.active_connections(),
        uptime_seconds: 0, // Would track actual uptime
    };
    axum::Json(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_message_subscribe_valuations() {
        let json =
            r#"{"type":"subscribe_valuations","instrument_ids":["US912810TD00","US037833DV24"]}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();

        match msg {
            ClientMessage::SubscribeValuations { instrument_ids } => {
                assert_eq!(instrument_ids.len(), 2);
                assert_eq!(instrument_ids[0], "US912810TD00");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn retired_subscriptions_are_not_client_messages() {
        for json in [
            r#"{"type":"subscribe_bonds","instrument_ids":["BOND-001"]}"#,
            r#"{"type":"unsubscribe_bonds","instrument_ids":["BOND-001"]}"#,
            r#"{"type":"subscribe_all_bonds"}"#,
            r#"{"type":"unsubscribe_all_bonds"}"#,
            r#"{"type":"subscribe_etfs","etf_ids":["ETF-001"]}"#,
            r#"{"type":"unsubscribe_etfs","etf_ids":["ETF-001"]}"#,
            r#"{"type":"subscribe_portfolios","portfolio_ids":["BOOK-001"]}"#,
            r#"{"type":"unsubscribe_portfolios","portfolio_ids":["BOOK-001"]}"#,
        ] {
            assert!(
                serde_json::from_str::<ClientMessage>(json).is_err(),
                "{json}"
            );
        }
    }

    #[test]
    fn test_client_message_ping() {
        let json = r#"{"type":"ping","timestamp":1234567890}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();

        match msg {
            ClientMessage::Ping { timestamp } => {
                assert_eq!(timestamp, 1234567890);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_server_message_connected() {
        let msg = ServerMessage::Connected {
            session_id: "ws-123".to_string(),
            server_time: 1234567890,
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"connected\""));
        assert!(json.contains("\"session_id\":\"ws-123\""));
    }

    #[test]
    fn demo_wire_labels_preserve_message_content() {
        let message = ServerMessage::Connected {
            session_id: "test".into(),
            server_time: 123,
        };
        let original = serde_json::to_value(&message).unwrap();
        let production: serde_json::Value =
            serde_json::from_str(&serialize_message(&message, ServerProfile::Production).unwrap())
                .unwrap();
        assert_eq!(production, original);
        let mut demo: serde_json::Value =
            serde_json::from_str(&serialize_message(&message, ServerProfile::Demo).unwrap())
                .unwrap();
        assert_eq!(
            demo.as_object_mut().unwrap().remove("profile").unwrap(),
            "demo"
        );
        assert_eq!(
            demo.as_object_mut()
                .unwrap()
                .remove("data_quality")
                .unwrap(),
            "demo"
        );
        assert_eq!(demo, original);
    }

    #[test]
    fn test_server_message_subscribed() {
        let msg = ServerMessage::Subscribed {
            subscription_type: "valuations".to_string(),
            count: 5,
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"subscribed\""));
        assert!(json.contains("\"count\":5"));
    }

    #[test]
    fn test_server_message_pong() {
        let msg = ServerMessage::Pong {
            timestamp: 1234567890,
            server_time: 1234567891,
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"pong\""));
        assert!(json.contains("\"timestamp\":1234567890"));
    }

    #[test]
    fn test_websocket_state_new() {
        let state = WebSocketState::new();
        assert_eq!(state.active_connections(), 0);
    }

    #[test]
    fn test_websocket_state_session_id() {
        let state = WebSocketState::new();

        let id1 = state.next_session_id();
        let id2 = state.next_session_id();

        assert!(id1.starts_with("ws-"));
        assert!(id2.starts_with("ws-"));
        assert_ne!(id1, id2);
    }
}
