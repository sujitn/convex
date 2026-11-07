//! WebSocket streaming of canonical runtime results.
//!
//! The runtime results channel (`PricingEngine::subscribe_results`) is the source of
//! streamed valuations. A client opts in with `subscribe_valuations` and receives a
//! `valuation` message for each published result of those instruments: first the current
//! result of each instrument the request lists, then the `subscribed` acknowledgement, then
//! each later result (D99). A result too large for one 64 KiB message arrives as a
//! `RESULT_TOO_LARGE` error naming its instrument, and the session continues. With
//! `subscribe_limits` it receives a `limit_status` message for each recorded change of a
//! book's limit status (P14c), as the limit monitor records it.
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
use crate::limit_monitor::{LimitFeed, LimitFrame, LimitStatusChange};
use crate::ServerProfile;

/// Books one session may follow: every book the monitor watches (D87).
const MAX_LIMIT_SUBSCRIPTIONS: usize = crate::limits::MAX_LIMITED_BOOKS;
/// Instruments one session may follow: every instrument the runtime values (D100).
const MAX_VALUATION_SUBSCRIPTIONS: usize = convex_engine::MAX_RUNTIME_INSTRUMENTS;
/// The largest message the server sends or accepts, in bytes.
const MAX_MESSAGE_BYTES: usize = 65536;

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
    /// Opt in to each recorded limit status change of these books.
    SubscribeLimits { book_ids: Vec<String> },
    /// Stop limit status changes of these books.
    UnsubscribeLimits { book_ids: Vec<String> },
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
    /// A recorded change of one limit's status.
    LimitStatus(Arc<LimitStatusChange>),
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
    limit_subscriptions: HashSet<String>,
}

impl ClientSession {
    /// Topics followed: instruments and books together, as the subscription quota counts them.
    fn topic_count(&self) -> usize {
        self.valuation_subscriptions.len() + self.limit_subscriptions.len()
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
    limits: Option<Extension<LimitFeed>>,
) -> impl IntoResponse {
    let profile = profile.map_or(ServerProfile::Production, |Extension(profile)| profile);
    let Some(Extension(authorization)) = authorization else {
        return crate::access::http::response(AccessError::Unauthenticated);
    };
    let limits = limits.map(|Extension(feed)| feed);
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_socket(socket, state, profile, authorization, limits))
        .into_response()
}

/// The next recorded limit status change, or never without a monitor.
async fn next_limit(
    frames: &mut Option<broadcast::Receiver<Arc<LimitFrame>>>,
) -> Result<Arc<LimitFrame>, broadcast::error::RecvError> {
    match frames {
        Some(frames) => frames.recv().await,
        None => std::future::pending().await,
    }
}

/// Handle an individual WebSocket connection.
async fn handle_socket(
    socket: WebSocket,
    state: Arc<AppState>,
    profile: ServerProfile,
    mut authorization: WsAccess,
    limits: Option<LimitFeed>,
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
    let mut limit_rx = limits.as_ref().map(LimitFeed::subscribe);
    let monitored = limit_rx.is_some();
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
                        let subscribed = valuation_instrument(&result).is_some_and(|id| session.valuation_subscriptions.contains(&id));
                        if subscribed && send_message(&mut sender, &ServerMessage::Valuation(Box::new(result)), profile, &authorization).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if send_message(&mut sender, &ServerMessage::Error { code: "RESYNC_REQUIRED".into(), message: "Subscribe again to receive each instrument's current result, or read /api/v1/runtime/results; keep the newer by (runtime_id, revision)".into() }, profile, &authorization).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            frame = next_limit(&mut limit_rx) => {
                match frame {
                    Ok(frame) => {
                        let book = frame.change.book_id.as_str();
                        if session.limit_subscriptions.contains(book) {
                            // As the status route: the book and every instrument it holds or a limit names.
                            let message = if limit_permission(book, frame.instruments.iter(), &authorization).is_ok() {
                                ServerMessage::LimitStatus(frame.change.clone())
                            } else {
                                ServerMessage::Error { code: "FORBIDDEN".into(), message: format!("limit status of book {book} needs read access to each instrument it holds or a limit names") }
                            };
                            if send_message(&mut sender, &message, profile, &authorization).await.is_err() { break; }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if !session.limit_subscriptions.is_empty() && send_message(&mut sender, &ServerMessage::Error { code: "RESYNC_REQUIRED".into(), message: "Read /api/v1/books/{book_id}/limits/status for each subscribed book".into() }, profile, &authorization).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Closed) => limit_rx = None,
                }
            }
            // Handle incoming messages from client
            message = receiver.next() => {
                let Some(msg) = message else { break; };
                let admission = match authorization.access.admit_message(&authorization.context) { Ok(admission) => admission, Err(_) => { let _ = tokio::time::timeout(Duration::from_secs(2), sender.send(Message::Close(None))).await; break; } };
                match msg {
                    Ok(Message::Text(text)) => {
                        if handle_client_payload(Ok(&text), admission, &mut session, &mut sender, profile, &authorization, monitored, &state.engine).await.is_err() { break; }
                    }
                    Ok(Message::Binary(data)) => {
                        let text = std::str::from_utf8(&data).map_err(|_| "INVALID_MESSAGE".to_owned());
                        if handle_client_payload(text, admission, &mut session, &mut sender, profile, &authorization, monitored, &state.engine).await.is_err() { break; }
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
                // A text or binary message releases its admission inside `handle_client_payload`;
                // any other releases it here, at the end of this arm.
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

/// Identical safe denial semantics for text and UTF-8 binary frames. The message's admission
/// covers handling it, not sending the replies: a subscription can send up to 256 results,
/// each with its own two-second budget. A reply that cannot be delivered ends the session, as
/// a streamed frame does; a result too large for one message is not such a reply (`encode`).
#[allow(clippy::too_many_arguments)]
async fn handle_client_payload(
    text: Result<&str, String>,
    admission: crate::access::Admission,
    session: &mut ClientSession,
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    profile: ServerProfile,
    authorization: &WsAccess,
    monitored: bool,
    engine: &convex_engine::PricingEngine,
) -> Result<(), axum::Error> {
    let result = text
        .and_then(|text| handle_client_message(text, session, authorization, monitored, engine));
    drop(admission);
    match result {
        Ok(replies) => {
            for reply in &replies {
                send_message(sender, reply, profile, authorization).await?;
            }
        }
        Err(error) => reject(error, sender, profile, authorization).await?,
    }
    Ok(())
}

/// Send the typed error for a refused client message.
async fn reject(
    error: String,
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    profile: ServerProfile,
    authorization: &WsAccess,
) -> Result<(), axum::Error> {
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
    .await
}

/// The replies to a client message, in order, or the reason it is refused.
fn handle_client_message(
    text: &str,
    session: &mut ClientSession,
    authorization: &WsAccess,
    monitored: bool,
    engine: &convex_engine::PricingEngine,
) -> Result<Vec<ServerMessage>, String> {
    let msg: ClientMessage =
        serde_json::from_str(text).map_err(|_| "INVALID_MESSAGE".to_owned())?;

    authorize_subscription(&msg, session, authorization).map_err(|error| error.to_string())?;
    let response = match msg {
        ClientMessage::SubscribeValuations { instrument_ids } => {
            let too_many =
                || format!("at most {MAX_VALUATION_SUBSCRIPTIONS} valuation subscriptions");
            if instrument_ids.len() > MAX_VALUATION_SUBSCRIPTIONS {
                return Err(too_many());
            }
            for id in &instrument_ids {
                convex_core::contracts::InstrumentId::new(id)
                    .map_err(|_| "invalid valuation identity")?;
            }
            let listed: HashSet<InstrumentId> =
                instrument_ids.iter().map(InstrumentId::new).collect();
            // The distinct instruments listed; a repeated id counts once.
            let count = listed.len();
            // Listing an instrument already followed resends its result and costs nothing.
            let added = listed
                .iter()
                .filter(|id| !session.valuation_subscriptions.contains(*id))
                .count();
            if session.valuation_subscriptions.len().saturating_add(added)
                > MAX_VALUATION_SUBSCRIPTIONS
            {
                return Err(too_many());
            }
            session
                .valuation_subscriptions
                .extend(listed.iter().cloned());
            // D99: each listed instrument's current result, then the acknowledgement. The
            // results are read after the subscription is in place, so a result published
            // meanwhile is either in this read or follows on the stream. A frame still
            // queued on the stream can be older than the one sent here; the client keeps
            // the newer by (runtime_id, revision).
            let mut replies: Vec<ServerMessage> = engine
                .runtime_results()
                .into_iter()
                .filter(|result| {
                    valuation_instrument(result).is_some_and(|id| listed.contains(&id))
                })
                .map(|result| ServerMessage::Valuation(Box::new(result)))
                .collect();
            replies.push(ServerMessage::Subscribed {
                subscription_type: "valuations".into(),
                count,
            });
            return Ok(replies);
        }
        ClientMessage::UnsubscribeValuations { instrument_ids } => {
            if instrument_ids.len() > MAX_VALUATION_SUBSCRIPTIONS {
                return Err(format!(
                    "at most {MAX_VALUATION_SUBSCRIPTIONS} valuation subscriptions"
                ));
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
        ClientMessage::SubscribeLimits { book_ids } => {
            if !monitored {
                return Err("this server does not monitor limits".into());
            }
            if book_ids.len() > MAX_LIMIT_SUBSCRIPTIONS
                || session
                    .limit_subscriptions
                    .len()
                    .saturating_add(book_ids.len())
                    > MAX_LIMIT_SUBSCRIPTIONS
            {
                return Err("at most 64 limit subscriptions".into());
            }
            for id in &book_ids {
                crate::books::book_id(id.clone()).map_err(|_| "invalid book identity")?;
            }
            let count = book_ids.len();
            session.limit_subscriptions.extend(book_ids);
            ServerMessage::Subscribed {
                subscription_type: "limits".into(),
                count,
            }
        }
        ClientMessage::UnsubscribeLimits { book_ids } => {
            if book_ids.len() > MAX_LIMIT_SUBSCRIPTIONS {
                return Err("at most 64 limit subscriptions".into());
            }
            let count = book_ids.len();
            for id in &book_ids {
                session.limit_subscriptions.remove(id);
            }
            ServerMessage::Unsubscribed {
                subscription_type: "limits".into(),
                count,
            }
        }
        ClientMessage::Ping { timestamp } => ServerMessage::Pong {
            timestamp,
            server_time: current_timestamp(),
        },
    };

    Ok(vec![response])
}

/// The instrument a published valuation is for, if it is for one.
fn valuation_instrument(
    result: &convex_engine::ports::runtime::PublishedValuation,
) -> Option<InstrumentId> {
    match &result.result.provenance().subject {
        convex_core::contracts::ValuationSubject::InstrumentVersion { instrument_id, .. } => {
            Some(InstrumentId::new(instrument_id.as_str()))
        }
        _ => None,
    }
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
/// A limit topic needs book read, with books, results, reference and market data, on the
/// book and on each of `instruments`, as the status route does. A subscription checks the
/// book; each frame checks the instruments the book holds or a limit names when it is sent.
fn limit_permission<'a>(
    book: &str,
    instruments: impl IntoIterator<Item = &'a String>,
    authorization: &WsAccess,
) -> Result<(), AccessError> {
    let mut resources = vec![Resource::new(ResourceKind::Book, book)];
    resources.extend(
        instruments
            .into_iter()
            .map(|id| Resource::new(ResourceKind::Instrument, id.as_str())),
    );
    authorization.access.authorize(
        &authorization.context,
        Action::BookRead,
        &[
            DataScope::Books,
            DataScope::Results,
            DataScope::Reference,
            DataScope::Market,
        ],
        &resources,
    )
}

/// Whether the session may still stream everything it follows under the current policy.
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
    for book in &session.limit_subscriptions {
        limit_permission(book, [], authorization)?;
    }
    Ok(())
}

/// Whether the session may make this subscription change, within its topic quota.
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
            if instrument_ids.len() > MAX_VALUATION_SUBSCRIPTIONS {
                return Err(AccessError::Quota);
            }
            return Ok(());
        }
        ClientMessage::SubscribeLimits { book_ids } => {
            return authorize_limits(book_ids, session, authorization)
        }
        ClientMessage::UnsubscribeLimits { book_ids } => {
            if book_ids.len() > MAX_LIMIT_SUBSCRIPTIONS {
                return Err(AccessError::Quota);
            }
            return Ok(());
        }
        ClientMessage::Ping { .. } => return Ok(()),
    };
    if ids.len() > MAX_VALUATION_SUBSCRIPTIONS {
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

/// Whether the session may follow these books, within its topic quota.
fn authorize_limits(
    ids: &[String],
    session: &ClientSession,
    authorization: &WsAccess,
) -> Result<(), AccessError> {
    if ids.len() > MAX_LIMIT_SUBSCRIPTIONS {
        return Err(AccessError::Quota);
    }
    let mut prospective: HashSet<&str> = session
        .limit_subscriptions
        .iter()
        .map(String::as_str)
        .collect();
    let before = prospective.len();
    for id in ids {
        if !crate::access::policy::valid_id(id) || id == "*" {
            return Err(AccessError::InvalidRequest);
        }
        limit_permission(id, [], authorization)?;
        prospective.insert(id);
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

/// Whether the session may still receive this message when it is sent.
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
    if let ServerMessage::LimitStatus(change) = message {
        limit_permission(change.book_id.as_str(), [], authorization)?;
    }
    Ok(())
}

/// Send a server message over WebSocket. An error ends the session.
async fn send_message(
    sender: &mut futures::stream::SplitSink<WebSocket, Message>,
    msg: &ServerMessage,
    profile: ServerProfile,
    authorization: &WsAccess,
) -> Result<(), axum::Error> {
    authorize_delivery(msg, authorization).map_err(axum::Error::new)?;
    let json = encode(msg, profile)?;
    tokio::time::timeout(Duration::from_secs(2), sender.send(Message::Text(json)))
        .await
        .map_err(axum::Error::new)?
        .map_err(axum::Error::new)
}

/// The JSON sent for a message. A valuation too large for one message is replaced by a
/// `RESULT_TOO_LARGE` error naming its instrument, so the session continues. Any other
/// message over the limit is refused.
fn encode(msg: &ServerMessage, profile: ServerProfile) -> Result<String, axum::Error> {
    let json = serialize_message(msg, profile).map_err(axum::Error::new)?;
    if json.len() <= MAX_MESSAGE_BYTES {
        return Ok(json);
    }
    let ServerMessage::Valuation(result) = msg else {
        return Err(axum::Error::new(AccessError::Quota));
    };
    let instrument = valuation_instrument(result).map_or_else(String::new, |id| id.to_string());
    let error = ServerMessage::Error {
        code: "RESULT_TOO_LARGE".into(),
        message: format!(
            "The result of instrument {instrument} (runtime {}, revision {}) exceeds the \
             {MAX_MESSAGE_BYTES}-byte message limit; read it from /api/v1/runtime/results",
            result.runtime_id, result.revision
        ),
    };
    serialize_message(&error, profile).map_err(axum::Error::new)
}

/// A message as JSON, with the demo labels when the profile is demo.
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

    /// A valuation of BOND-001 whose lineage lists `inputs` records with 128-character ids.
    fn valuation(inputs: usize) -> convex_engine::ports::runtime::PublishedValuation {
        use convex_core::contracts::*;
        let at = UtcTimestamp::parse("2026-11-07T12:00:00Z").unwrap();
        let long = |prefix: &str, n: usize| format!("{prefix}-{n:04}-{}", "x".repeat(118));
        let input_versions = (0..inputs)
            .map(|n| InputVersion {
                kind: SourceId::new(long("kind", n)).unwrap(),
                id: SourceId::new(long("id", n)).unwrap(),
                version: Version::new(1).unwrap(),
            })
            .collect();
        let error =
            StructuredError::new(ErrorCode::MissingMarketData, None, "no quote", false).unwrap();
        let result = ResultEnvelope::new(
            RequestId::new("request-1").unwrap(),
            ResultStatus::Failed,
            DataQuality::Invalid,
            std::collections::BTreeMap::new(),
            vec![error],
            Provenance {
                subject: ValuationSubject::InstrumentVersion {
                    instrument_id: convex_core::contracts::InstrumentId::new("BOND-001").unwrap(),
                    version: Version::new(1).unwrap(),
                },
                snapshot_id: SnapshotId::new(format!("sha256:{}", "0".repeat(64))).unwrap(),
                configuration_version: Version::new(1).unwrap(),
                valuation_time: at,
                known_at: at,
                calculated_at: at,
                model_version: ModelId::new("model-1").unwrap(),
                build_id: ModelId::new("build-1").unwrap(),
                input_versions,
            },
        )
        .unwrap();
        convex_engine::ports::runtime::PublishedValuation {
            runtime_id: SourceId::new("runtime-1").unwrap(),
            revision: 7,
            generation: Version::new(1).unwrap(),
            context_key: "context".into(),
            live_valid_until: None,
            result,
            last_good: None,
            retained_quality: None,
        }
    }

    #[test]
    fn a_valuation_too_large_for_one_message_is_sent_as_an_error_naming_it() {
        let fits = ServerMessage::Valuation(Box::new(valuation(8)));
        let sent: serde_json::Value =
            serde_json::from_str(&encode(&fits, ServerProfile::Production).unwrap()).unwrap();
        assert_eq!(sent["type"], "valuation");

        // 256 records with 128-character ids: the most a lineage holds, about 77 KiB.
        let large = ServerMessage::Valuation(Box::new(valuation(256)));
        assert!(
            serialize_message(&large, ServerProfile::Production)
                .unwrap()
                .len()
                > MAX_MESSAGE_BYTES
        );
        let sent = encode(&large, ServerProfile::Production).unwrap();
        assert!(sent.len() < 1024);
        let sent: serde_json::Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(sent["type"], "error");
        assert_eq!(sent["code"], "RESULT_TOO_LARGE");
        let message = sent["message"].as_str().unwrap();
        for part in [
            "BOND-001",
            "runtime-1",
            "revision 7",
            "/api/v1/runtime/results",
        ] {
            assert!(message.contains(part), "{message}");
        }

        let demo: serde_json::Value =
            serde_json::from_str(&encode(&large, ServerProfile::Demo).unwrap()).unwrap();
        assert_eq!(demo["code"], "RESULT_TOO_LARGE");
        assert_eq!(demo["data_quality"], "demo");
    }

    #[test]
    fn any_other_message_too_large_for_one_message_is_refused() {
        let error = ServerMessage::Error {
            code: "INVALID_MESSAGE".into(),
            message: "x".repeat(MAX_MESSAGE_BYTES),
        };
        assert!(encode(&error, ServerProfile::Production).is_err());
    }

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
