//! WebSocket handlers for real-time quote streaming.
//!
//! Supports:
//! - Subscribing to individual bond quotes
//! - Subscribing to ETF iNAV updates
//! - Subscribing to portfolio analytics updates
//! - Heartbeat/ping-pong for connection health

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use convex_traits::ids::{EtfId, InstrumentId, PortfolioId};
use convex_traits::output::{BondQuoteOutput, EtfQuoteOutput, PortfolioAnalyticsOutput};

use crate::handlers::AppState;

// =============================================================================
// MESSAGE TYPES
// =============================================================================

/// Inbound WebSocket message from client.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum ClientMessage {
    /// Subscribe to bond quotes
    SubscribeBonds { instrument_ids: Vec<String> },
    /// Unsubscribe from bond quotes
    UnsubscribeBonds { instrument_ids: Vec<String> },
    /// Subscribe to all bond quotes
    SubscribeAllBonds,
    /// Unsubscribe from all bond quotes
    UnsubscribeAllBonds,
    /// Subscribe to ETF iNAV updates
    SubscribeEtfs { etf_ids: Vec<String> },
    /// Unsubscribe from ETF updates
    UnsubscribeEtfs { etf_ids: Vec<String> },
    /// Subscribe to portfolio analytics
    SubscribePortfolios { portfolio_ids: Vec<String> },
    /// Unsubscribe from portfolio analytics
    UnsubscribePortfolios { portfolio_ids: Vec<String> },
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
    /// Bond quote update
    BondQuote(BondQuoteOutput),
    /// ETF iNAV update
    EtfQuote(EtfQuoteOutput),
    /// Portfolio analytics update
    PortfolioAnalytics(PortfolioAnalyticsOutput),
    /// Pong (server heartbeat response)
    Pong { timestamp: i64, server_time: i64 },
    /// Error message
    Error { code: String, message: String },
    /// Server heartbeat
    Heartbeat { server_time: i64 },
    /// Node recalculated event (from reactive engine)
    NodeRecalculated {
        node_type: String,
        node_id: String,
        source: String,
        timestamp: i64,
    },
    /// Curve update notification
    CurveUpdated {
        curve_id: String,
        timestamp: i64,
    },
}

// =============================================================================
// BROADCAST TYPES
// =============================================================================

/// Broadcast update that can be sent to subscribers.
#[derive(Debug, Clone)]
pub enum BroadcastUpdate {
    /// Bond quote update
    BondQuote(BondQuoteOutput),
    /// ETF iNAV update
    EtfQuote(EtfQuoteOutput),
    /// Portfolio analytics update
    PortfolioAnalytics(PortfolioAnalyticsOutput),
    /// Node recalculated event
    NodeRecalculated {
        /// Type of node (bond_price, curve, etf_inav, etc.)
        node_type: String,
        /// Node identifier
        node_id: String,
        /// Source of update (immediate, throttled, interval, eod, on_demand)
        source: String,
        /// Timestamp
        timestamp: i64,
    },
    /// Curve updated event
    CurveUpdated {
        /// Curve identifier
        curve_id: String,
        /// Timestamp
        timestamp: i64,
    },
}

// =============================================================================
// WEBSOCKET STATE
// =============================================================================

/// WebSocket connection state manager.
pub struct WebSocketState {
    /// Broadcast sender for all updates
    pub broadcast_tx: broadcast::Sender<BroadcastUpdate>,
    /// Active connections count
    pub connection_count: std::sync::atomic::AtomicUsize,
    /// Session ID counter
    session_counter: std::sync::atomic::AtomicU64,
}

impl WebSocketState {
    /// Create a new WebSocket state manager.
    pub fn new() -> Self {
        let (broadcast_tx, _) = broadcast::channel(10000);
        Self {
            broadcast_tx,
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

    /// Publish a bond quote update.
    pub fn publish_bond_quote(&self, quote: BondQuoteOutput) {
        let _ = self.broadcast_tx.send(BroadcastUpdate::BondQuote(quote));
    }

    /// Publish an ETF quote update.
    pub fn publish_etf_quote(&self, quote: EtfQuoteOutput) {
        let _ = self.broadcast_tx.send(BroadcastUpdate::EtfQuote(quote));
    }

    /// Publish portfolio analytics update.
    pub fn publish_portfolio_analytics(&self, analytics: PortfolioAnalyticsOutput) {
        let _ = self
            .broadcast_tx
            .send(BroadcastUpdate::PortfolioAnalytics(analytics));
    }

    /// Get active connection count.
    pub fn active_connections(&self) -> usize {
        self.connection_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Publish a node recalculated event.
    pub fn publish_node_recalculated(
        &self,
        node_type: String,
        node_id: String,
        source: String,
        timestamp: i64,
    ) {
        let _ = self.broadcast_tx.send(BroadcastUpdate::NodeRecalculated {
            node_type,
            node_id,
            source,
            timestamp,
        });
    }

    /// Publish a curve updated event.
    pub fn publish_curve_updated(&self, curve_id: String, timestamp: i64) {
        let _ = self.broadcast_tx.send(BroadcastUpdate::CurveUpdated {
            curve_id,
            timestamp,
        });
    }
}

impl Default for WebSocketState {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// REACTIVE ENGINE INTEGRATION
// =============================================================================

use convex_engine::{NodeId as EngineNodeId, NodeUpdate, UpdateSource};

/// Convert engine NodeId to (node_type, node_id) strings.
fn node_id_to_strings(node_id: &EngineNodeId) -> (String, String) {
    match node_id {
        EngineNodeId::Quote { instrument_id } => ("quote".to_string(), instrument_id.to_string()),
        EngineNodeId::CurveInput { curve_id, instrument } => {
            ("curve_input".to_string(), format!("{}.{}", curve_id, instrument))
        }
        EngineNodeId::Curve { curve_id } => ("curve".to_string(), curve_id.to_string()),
        EngineNodeId::VolSurface { surface_id } => ("vol_surface".to_string(), surface_id.to_string()),
        EngineNodeId::FxRate { pair } => ("fx_rate".to_string(), pair.to_string()),
        EngineNodeId::IndexFixing { index, date } => {
            ("index_fixing".to_string(), format!("{}.{}", index, date))
        }
        EngineNodeId::InflationFixing { index, month } => {
            ("inflation_fixing".to_string(), format!("{}.{}", index, month))
        }
        EngineNodeId::Config { config_id } => ("config".to_string(), config_id.clone()),
        EngineNodeId::BondPrice { instrument_id } => ("bond_price".to_string(), instrument_id.to_string()),
        EngineNodeId::EtfInav { etf_id } => ("etf_inav".to_string(), etf_id.to_string()),
        EngineNodeId::EtfNav { etf_id } => ("etf_nav".to_string(), etf_id.to_string()),
        EngineNodeId::Portfolio { portfolio_id } => ("portfolio".to_string(), portfolio_id.to_string()),
    }
}

/// Convert engine UpdateSource to string.
fn update_source_to_string(source: &UpdateSource) -> String {
    match source {
        UpdateSource::Immediate => "immediate".to_string(),
        UpdateSource::Throttled => "throttled".to_string(),
        UpdateSource::Interval => "interval".to_string(),
        UpdateSource::EndOfDay => "eod".to_string(),
        UpdateSource::OnDemand => "on_demand".to_string(),
    }
}

/// Connect the reactive engine's node update stream to WebSocket broadcasting.
///
/// This spawns a background task that listens to node updates from the reactive
/// engine and forwards them to WebSocket clients via the broadcast channel.
pub fn connect_reactive_engine(
    ws_state: Arc<WebSocketState>,
    mut node_update_rx: tokio::sync::broadcast::Receiver<NodeUpdate>,
) {
    tokio::spawn(async move {
        info!("Reactive engine -> WebSocket bridge started");

        loop {
            match node_update_rx.recv().await {
                Ok(update) => {
                    let (node_type, node_id) = node_id_to_strings(&update.node_id);
                    let source = update_source_to_string(&update.source);

                    debug!(
                        "Broadcasting node update: {} {} ({})",
                        node_type, node_id, source
                    );

                    ws_state.publish_node_recalculated(
                        node_type.clone(),
                        node_id.clone(),
                        source,
                        update.timestamp,
                    );

                    // Also publish curve-specific events for curve nodes
                    if node_type == "curve" {
                        ws_state.publish_curve_updated(node_id, update.timestamp);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!("WebSocket bridge lagged by {} node updates", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    info!("Reactive engine channel closed, WebSocket bridge shutting down");
                    break;
                }
            }
        }
    });
}

// =============================================================================
// CLIENT SESSION
// =============================================================================

/// Per-client subscription state.
struct ClientSession {
    session_id: String,
    /// Subscribed bond instrument IDs
    bond_subscriptions: HashSet<InstrumentId>,
    /// Subscribe to all bonds
    subscribe_all_bonds: bool,
    /// Subscribed ETF IDs
    etf_subscriptions: HashSet<EtfId>,
    /// Subscribed portfolio IDs
    portfolio_subscriptions: HashSet<PortfolioId>,
}

impl ClientSession {
    fn new(session_id: String) -> Self {
        Self {
            session_id,
            bond_subscriptions: HashSet::new(),
            subscribe_all_bonds: false,
            etf_subscriptions: HashSet::new(),
            portfolio_subscriptions: HashSet::new(),
        }
    }

    /// Check if client is subscribed to a bond.
    fn is_subscribed_to_bond(&self, id: &InstrumentId) -> bool {
        self.subscribe_all_bonds || self.bond_subscriptions.contains(id)
    }

    /// Check if client is subscribed to an ETF.
    fn is_subscribed_to_etf(&self, id: &EtfId) -> bool {
        self.etf_subscriptions.contains(id)
    }

    /// Check if client is subscribed to a portfolio.
    fn is_subscribed_to_portfolio(&self, id: &PortfolioId) -> bool {
        self.portfolio_subscriptions.contains(id)
    }
}

// =============================================================================
// WEBSOCKET HANDLER
// =============================================================================

/// WebSocket upgrade handler.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handle an individual WebSocket connection.
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let ws_state = &state.ws_state;

    // Generate session ID
    let session_id = ws_state.next_session_id();
    info!("WebSocket connection established: {}", session_id);

    // Increment connection count
    ws_state
        .connection_count
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    // Create client session
    let mut session = ClientSession::new(session_id.clone());

    // Subscribe to broadcast channel
    let mut broadcast_rx = ws_state.broadcast_tx.subscribe();

    // Split socket into sender and receiver
    let (mut sender, mut receiver) = socket.split();

    // Send connected message
    let connected_msg = ServerMessage::Connected {
        session_id: session_id.clone(),
        server_time: current_timestamp(),
    };
    if let Err(e) = send_message(&mut sender, &connected_msg).await {
        error!("Failed to send connected message: {}", e);
        ws_state
            .connection_count
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        return;
    }

    // Heartbeat interval
    let mut heartbeat_interval = interval(Duration::from_secs(30));

    loop {
        tokio::select! {
            // Handle incoming messages from client
            Some(msg) = receiver.next() => {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Err(e) = handle_client_message(&text, &mut session, &mut sender).await {
                            warn!("Error handling client message: {}", e);
                            // Send error message to client
                            let error_msg = ServerMessage::Error {
                                code: "INVALID_MESSAGE".to_string(),
                                message: e,
                            };
                            if send_message(&mut sender, &error_msg).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(Message::Binary(data)) => {
                        // Try to parse as JSON
                        if let Ok(text) = String::from_utf8(data) {
                            if let Err(e) = handle_client_message(&text, &mut session, &mut sender).await {
                                warn!("Error handling binary message: {}", e);
                            }
                        }
                    }
                    Ok(Message::Ping(data)) => {
                        if sender.send(Message::Pong(data)).await.is_err() {
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
                    Err(e) => {
                        error!("WebSocket error for {}: {}", session_id, e);
                        break;
                    }
                }
            }

            // Handle broadcast updates
            Ok(update) = broadcast_rx.recv() => {
                if let Some(msg) = filter_update(&session, &update) {
                    if send_message(&mut sender, &msg).await.is_err() {
                        break;
                    }
                }
            }

            // Send heartbeat
            _ = heartbeat_interval.tick() => {
                let heartbeat = ServerMessage::Heartbeat {
                    server_time: current_timestamp(),
                };
                if send_message(&mut sender, &heartbeat).await.is_err() {
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

/// Handle a client message.
async fn handle_client_message(
    text: &str,
    session: &mut ClientSession,
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
) -> Result<(), String> {
    let msg: ClientMessage =
        serde_json::from_str(text).map_err(|e| format!("Invalid message: {}", e))?;

    let response = match msg {
        ClientMessage::SubscribeBonds { instrument_ids } => {
            let count = instrument_ids.len();
            for id in instrument_ids {
                session.bond_subscriptions.insert(InstrumentId::new(&id));
            }
            debug!(
                "Session {} subscribed to {} bonds",
                session.session_id, count
            );
            ServerMessage::Subscribed {
                subscription_type: "bonds".to_string(),
                count,
            }
        }
        ClientMessage::UnsubscribeBonds { instrument_ids } => {
            let count = instrument_ids.len();
            for id in instrument_ids {
                session.bond_subscriptions.remove(&InstrumentId::new(&id));
            }
            ServerMessage::Unsubscribed {
                subscription_type: "bonds".to_string(),
                count,
            }
        }
        ClientMessage::SubscribeAllBonds => {
            session.subscribe_all_bonds = true;
            debug!("Session {} subscribed to all bonds", session.session_id);
            ServerMessage::Subscribed {
                subscription_type: "all_bonds".to_string(),
                count: 1,
            }
        }
        ClientMessage::UnsubscribeAllBonds => {
            session.subscribe_all_bonds = false;
            session.bond_subscriptions.clear();
            ServerMessage::Unsubscribed {
                subscription_type: "all_bonds".to_string(),
                count: 1,
            }
        }
        ClientMessage::SubscribeEtfs { etf_ids } => {
            let count = etf_ids.len();
            for id in etf_ids {
                session.etf_subscriptions.insert(EtfId::new(&id));
            }
            debug!("Session {} subscribed to {} ETFs", session.session_id, count);
            ServerMessage::Subscribed {
                subscription_type: "etfs".to_string(),
                count,
            }
        }
        ClientMessage::UnsubscribeEtfs { etf_ids } => {
            let count = etf_ids.len();
            for id in etf_ids {
                session.etf_subscriptions.remove(&EtfId::new(&id));
            }
            ServerMessage::Unsubscribed {
                subscription_type: "etfs".to_string(),
                count,
            }
        }
        ClientMessage::SubscribePortfolios { portfolio_ids } => {
            let count = portfolio_ids.len();
            for id in portfolio_ids {
                session
                    .portfolio_subscriptions
                    .insert(PortfolioId::new(&id));
            }
            debug!(
                "Session {} subscribed to {} portfolios",
                session.session_id, count
            );
            ServerMessage::Subscribed {
                subscription_type: "portfolios".to_string(),
                count,
            }
        }
        ClientMessage::UnsubscribePortfolios { portfolio_ids } => {
            let count = portfolio_ids.len();
            for id in portfolio_ids {
                session
                    .portfolio_subscriptions
                    .remove(&PortfolioId::new(&id));
            }
            ServerMessage::Unsubscribed {
                subscription_type: "portfolios".to_string(),
                count,
            }
        }
        ClientMessage::Ping { timestamp } => ServerMessage::Pong {
            timestamp,
            server_time: current_timestamp(),
        },
    };

    send_message(sender, &response)
        .await
        .map_err(|e| e.to_string())
}

/// Filter a broadcast update for a specific client session.
fn filter_update(session: &ClientSession, update: &BroadcastUpdate) -> Option<ServerMessage> {
    match update {
        BroadcastUpdate::BondQuote(quote) => {
            if session.is_subscribed_to_bond(&quote.instrument_id) {
                Some(ServerMessage::BondQuote(quote.clone()))
            } else {
                None
            }
        }
        BroadcastUpdate::EtfQuote(quote) => {
            if session.is_subscribed_to_etf(&quote.etf_id) {
                Some(ServerMessage::EtfQuote(quote.clone()))
            } else {
                None
            }
        }
        BroadcastUpdate::PortfolioAnalytics(analytics) => {
            if session.is_subscribed_to_portfolio(&analytics.portfolio_id) {
                Some(ServerMessage::PortfolioAnalytics(analytics.clone()))
            } else {
                None
            }
        }
        BroadcastUpdate::NodeRecalculated {
            node_type,
            node_id,
            source,
            timestamp,
        } => {
            // Send node updates to clients that have subscribed to the relevant type
            // For now, send to all subscribed clients based on node type
            let should_send = match node_type.as_str() {
                "bond_price" => session.subscribe_all_bonds || session.bond_subscriptions.iter().any(|id| id.as_str() == node_id),
                "etf_inav" | "etf_nav" => session.etf_subscriptions.iter().any(|id| id.as_str() == node_id),
                "portfolio" => session.portfolio_subscriptions.iter().any(|id| id.as_str() == node_id),
                _ => false, // Don't send curve or other internal updates by default
            };
            if should_send {
                Some(ServerMessage::NodeRecalculated {
                    node_type: node_type.clone(),
                    node_id: node_id.clone(),
                    source: source.clone(),
                    timestamp: *timestamp,
                })
            } else {
                None
            }
        }
        BroadcastUpdate::CurveUpdated { curve_id, timestamp } => {
            // Curve updates are typically internal, but we can broadcast to clients
            // that want to know when curves change (e.g., for debugging or analytics dashboards)
            // For now, don't filter - clients can ignore if not interested
            Some(ServerMessage::CurveUpdated {
                curve_id: curve_id.clone(),
                timestamp: *timestamp,
            })
        }
    }
}

/// Send a server message over WebSocket.
async fn send_message(
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    msg: &ServerMessage,
) -> Result<(), axum::Error> {
    let json = serde_json::to_string(msg).unwrap();
    sender
        .send(Message::Text(json))
        .await
        .map_err(|e| axum::Error::new(e))
}

/// Get current timestamp in milliseconds.
fn current_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
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
    use rust_decimal_macros::dec;

    #[test]
    fn test_client_message_subscribe_bonds() {
        let json = r#"{"type":"subscribe_bonds","instrument_ids":["US912810TD00","US037833DV24"]}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();

        match msg {
            ClientMessage::SubscribeBonds { instrument_ids } => {
                assert_eq!(instrument_ids.len(), 2);
                assert_eq!(instrument_ids[0], "US912810TD00");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_client_message_subscribe_all_bonds() {
        let json = r#"{"type":"subscribe_all_bonds"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();

        assert!(matches!(msg, ClientMessage::SubscribeAllBonds));
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
    fn test_server_message_subscribed() {
        let msg = ServerMessage::Subscribed {
            subscription_type: "bonds".to_string(),
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

    #[test]
    fn test_broadcast_bond_quote() {
        use convex_core::{Currency, Date};

        let state = WebSocketState::new();
        let mut rx = state.broadcast_tx.subscribe();

        let quote = BondQuoteOutput {
            instrument_id: InstrumentId::new("TEST"),
            isin: None,
            currency: Currency::USD,
            settlement_date: Date::from_ymd(2025, 6, 17).unwrap(),
            clean_price_bid: None,
            clean_price_mid: Some(dec!(100)),
            clean_price_ask: None,
            accrued_interest: Some(dec!(0.50)),
            ytm_bid: None,
            ytm_mid: Some(dec!(0.05)),
            ytm_ask: None,
            ytw: None,
            ytc: None,
            z_spread_bid: None,
            z_spread_mid: None,
            z_spread_ask: None,
            i_spread_bid: None,
            i_spread_mid: None,
            i_spread_ask: None,
            g_spread_bid: None,
            g_spread_mid: None,
            g_spread_ask: None,
            asw_bid: None,
            asw_mid: None,
            asw_ask: None,
            oas_bid: None,
            oas_mid: None,
            oas_ask: None,
            discount_margin_bid: None,
            discount_margin_mid: None,
            discount_margin_ask: None,
            simple_margin_bid: None,
            simple_margin_mid: None,
            simple_margin_ask: None,
            modified_duration: Some(dec!(5.0)),
            macaulay_duration: None,
            effective_duration: None,
            spread_duration: None,
            convexity: Some(dec!(30)),
            effective_convexity: None,
            dv01: None,
            pv01: None,
            key_rate_durations: None,
            cs01: None,
            timestamp: 0,
            pricing_spec: "test".to_string(),
            source: "test".to_string(),
            is_stale: false,
            quality: 100,
        };

        state.publish_bond_quote(quote.clone());

        // Should receive the quote
        let update = rx.try_recv().unwrap();
        match update {
            BroadcastUpdate::BondQuote(q) => {
                assert_eq!(q.instrument_id.as_str(), "TEST");
            }
            _ => panic!("Wrong update type"),
        }
    }
}
