//! WebSocket streaming of canonical runtime results.
//!
//! The runtime results channel (`PricingEngine::subscribe_results`) is the source of
//! streamed valuations. A client opts in with `subscribe_valuations` and receives a
//! `valuation` message for each published result of those instruments: first the current
//! result of each instrument the request lists, then the `subscribed` acknowledgement, then
//! each later result. A result too large for one 64 KiB message arrives as a
//! `RESULT_TOO_LARGE` error naming its instrument, and the session continues. With
//! `subscribe_limits` it receives a `limit_status` message for each recorded change of a
//! book's limit status, as the limit monitor records it. With `subscribe_books` it receives
//! each book's latest `book_summary`, then the acknowledgement, then each later one, at most
//! four a second a book; of two, the one with the higher `sequence` is the later. With
//! `subscribe_rfqs` it receives
//! an `rfq` message for each open request for quote on those books, then the
//! acknowledgement, then one for each recorded transition of a request on them; of two
//! messages for one request, the one with the higher `record_id` is the later.
//! REST calculators return their results in the response and never broadcast.
//! Heartbeat and ping/pong keep the connection healthy.

use std::collections::{HashMap, HashSet};
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
use convex_engine::ports::reference_repository::ReferenceError;

use crate::access::http::WsAccess;
use crate::access::{AccessError, Action, DataScope, Resource, ResourceKind};
use crate::book_summary::{BookFeed, BookFrame, BookSummary};
use crate::handlers::AppState;
use crate::limit_monitor::{LimitFeed, LimitFrame, LimitStatusChange};
use crate::rfq_desk::{Rfq, RfqDesk, RfqFrame, NO_RFQ_DESK};
use crate::rfqs::{book_instruments, readable, RfqReader};
use crate::ServerProfile;

/// Books one session may follow: every book the monitor watches.
const MAX_LIMIT_SUBSCRIPTIONS: usize = crate::watched_books::MAX_WATCHED_BOOKS;
/// Books one session may follow for summaries: every book the monitor watches.
const MAX_BOOK_SUBSCRIPTIONS: usize = crate::watched_books::MAX_WATCHED_BOOKS;
/// Why a session may follow no more books' summaries.
const TOO_MANY_SUMMARY_BOOKS: &str = "at most 64 book subscriptions";
/// Books one session may follow for requests for quote, and one message may list.
const MAX_RFQ_SUBSCRIPTIONS: usize = convex_engine::configuration::MAX_PLAN_BOOKS;
/// Why a session may follow no more books' limit status; the Excel add-in reads this text.
const TOO_MANY_LIMIT_BOOKS: &str = "at most 64 limit subscriptions";
/// Why a session may follow no more books' requests for quote.
const TOO_MANY_RFQ_BOOKS: &str = "at most 64 RFQ subscriptions";
const _: () = assert!(
    MAX_LIMIT_SUBSCRIPTIONS == 64 && MAX_BOOK_SUBSCRIPTIONS == 64 && MAX_RFQ_SUBSCRIPTIONS == 64
);
/// The subprotocol a browser page offers when it opens the stream beside its token's,
/// and the one the server selects: a browser fails the handshake when it offers
/// protocols and none is selected.
const STREAM_PROTOCOL: &str = "convex.v1";
/// Instruments one session may follow: every instrument the runtime values.
const MAX_VALUATION_SUBSCRIPTIONS: usize = convex_engine::MAX_RUNTIME_INSTRUMENTS;
/// Instruments one valuation message lists, so it fits one message and its seed is at
/// most this many results. A session following 4,096 sends 16.
const MAX_MESSAGE_INSTRUMENTS: usize = 256;
/// The largest message the server sends or accepts, in bytes.
const MAX_MESSAGE_BYTES: usize = 65536;
/// Why `subscribe_limits` is refused by a server without a limit monitor.
const NOT_MONITORED: &str = "this server does not monitor limits";

// The Excel add-in tells these errors apart by their text, since a limits error has the
// same code as a valuations one (`excel/Convex.Excel/server/LimitWire.cs`, `LimitErrors`).
// A test pins each opening.

/// A subscribed book's change the session may not read: it lacks `BookRead` on an
/// instrument the book holds or a limit names. It answers no client message.
fn limit_forbidden(book: &str) -> ServerMessage {
    ServerMessage::Error {
        code: "FORBIDDEN".into(),
        message: format!(
            "limit status of book {book} needs read access to each instrument it holds or a \
             limit names"
        ),
    }
}

/// A followed book's summary the session may not read: it lacks `BookRead` on an
/// instrument the book holds, a limit names or the close holds. It answers no client
/// message but the subscription it is sent for.
fn summary_forbidden(book: &str) -> ServerMessage {
    ServerMessage::Error {
        code: "FORBIDDEN".into(),
        message: format!(
            "the summary of book {book} needs read access to each instrument it holds, a \
             limit names or its close holds"
        ),
    }
}

/// The session fell behind the book summary stream.
fn books_resync() -> ServerMessage {
    ServerMessage::Error {
        code: "RESYNC_REQUIRED".into(),
        message: "Subscribe again to each book to receive its latest summary; keep the newer \
                  by (monitor_started_at, sequence)"
            .into(),
    }
}

/// A book's summary as the session may read it now, or in its place why not.
fn summary_message(
    frame: &BookFrame,
    authorization: &WsAccess,
    checked: &mut HashMap<String, SummaryAccess>,
) -> ServerMessage {
    let book = frame.summary.book_id.as_str();
    let policy_version = authorization.access.policy_version();
    let is_readable = match checked.get(book) {
        Some(access)
            if Arc::ptr_eq(&access.instruments, &frame.instruments)
                && access.policy_version == policy_version =>
        {
            access.is_readable
        }
        _ => {
            let is_readable =
                limit_permission(book, frame.instruments.iter(), authorization).is_ok();
            checked.insert(
                book.to_owned(),
                SummaryAccess {
                    instruments: frame.instruments.clone(),
                    policy_version,
                    is_readable,
                },
            );
            is_readable
        }
    };
    if is_readable {
        ServerMessage::BookSummary(frame.summary.clone())
    } else {
        summary_forbidden(book)
    }
}

/// Whether a session may read a book's summaries, as last checked: against the book's
/// instruments then, under the policy then. A summary over the same instruments under the
/// same policy is not checked again; the book itself is checked on every one as it goes out.
struct SummaryAccess {
    instruments: Arc<[String]>,
    policy_version: Option<u64>,
    is_readable: bool,
}

/// The session fell behind the limit status stream.
fn limits_resync() -> ServerMessage {
    ServerMessage::Error {
        code: "RESYNC_REQUIRED".into(),
        message: "Read /api/v1/books/{book_id}/limits/status for each subscribed book".into(),
    }
}

/// A request for quote on a followed book, as the session may read it now against its
/// book's instruments: the request, or in its place why it is withheld, as the list
/// leaves it out.
fn rfq_message(
    rfq: Arc<Rfq>,
    book_instruments: &Result<Arc<[String]>, ReferenceError>,
    authorization: &WsAccess,
) -> ServerMessage {
    // Read before the check, so a policy that changes during it is checked again.
    let policy_version = authorization.access.policy_version();
    let book = rfq.book_id().as_str();
    let (code, message) = match book_instruments {
        Ok(instruments) if rfq_permission(&rfq, instruments, authorization).is_ok() => {
            return ServerMessage::Rfq(ReadableRfq {
                policy_version,
                book_instruments: instruments.clone(),
                rfq,
            })
        }
        Ok(_) => (
            "FORBIDDEN",
            format!(
                "a request for quote on book {book} needs read access to each instrument the \
                 book holds, a limit names or the request is for"
            ),
        ),
        Err(_) => (
            "UNAVAILABLE",
            format!(
                "whether a request for quote on book {book} may be read could not be checked; \
                 read /api/v1/rfqs"
            ),
        ),
    };
    ServerMessage::RfqWithheld {
        book_id: book.to_owned(),
        code,
        message,
    }
}

/// The session fell behind the RFQ stream.
fn rfqs_resync() -> ServerMessage {
    ServerMessage::Error {
        code: "RESYNC_REQUIRED".into(),
        message: "Subscribe to the books' requests for quote again to receive each open one, \
                  or read /api/v1/rfqs; keep the newer by (rfq_key, record_id)"
            .into(),
    }
}

/// The session fell behind the valuation stream.
fn valuations_resync() -> ServerMessage {
    ServerMessage::Error {
        code: "RESYNC_REQUIRED".into(),
        message: "Subscribe again to receive each instrument's current result, or read \
                  /api/v1/runtime/results; keep the newer by (runtime_id, revision)"
            .into(),
    }
}

// =============================================================================
// MESSAGE TYPES
// =============================================================================

/// Inbound WebSocket message from client. Any other `type`, including the retired
/// bond, ETF and portfolio subscriptions, is refused with `INVALID_MESSAGE`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ClientMessage {
    /// Opt in to canonical snapshot-valued results.
    SubscribeValuations { instrument_ids: Vec<String> },
    /// Stop canonical valuation updates.
    UnsubscribeValuations { instrument_ids: Vec<String> },
    /// Opt in to each recorded limit status change of these books.
    SubscribeLimits { book_ids: Vec<String> },
    /// Stop limit status changes of these books.
    UnsubscribeLimits { book_ids: Vec<String> },
    /// Opt in to each book's latest summary, and each later one.
    SubscribeBooks { book_ids: Vec<String> },
    /// Stop summaries of these books.
    UnsubscribeBooks { book_ids: Vec<String> },
    /// Opt in to each open request for quote on these books, and each later transition.
    SubscribeRfqs { book_ids: Vec<String> },
    /// Stop requests for quote on these books.
    UnsubscribeRfqs { book_ids: Vec<String> },
    /// Ping (client heartbeat)
    Ping { timestamp: i64 },
}

/// Outbound WebSocket message to client.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ServerMessage {
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
    /// Shared with the runtime, so a frame to many sessions copies no record.
    Valuation(Arc<convex_engine::ports::runtime::PublishedValuation>),
    /// A recorded change of one limit's status.
    LimitStatus(Arc<LimitStatusChange>),
    /// A watched book's risk, P&L since its close and limit statuses, from one evaluation.
    BookSummary(Arc<BookSummary>),
    /// A request for quote as its latest entry left it.
    Rfq(ReadableRfq),
    /// A request for quote on a followed book the session may not read now, in its place.
    RfqWithheld {
        book_id: String,
        code: &'static str,
        message: String,
    },
}

/// A request for quote the session was found to read, with the policy and the book's
/// instruments that check read, so it is made again as it is sent if the policy changed.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub(crate) struct ReadableRfq {
    rfq: Arc<Rfq>,
    #[serde(skip)]
    policy_version: Option<u64>,
    #[serde(skip)]
    book_instruments: Arc<[String]>,
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
struct ClientSession {
    valuation_subscriptions: HashSet<InstrumentId>,
    limits: BookTopic,
    books: BookTopic,
    rfqs: BookTopic,
    /// Each followed book's summary access, as last checked.
    summary_access: HashMap<String, SummaryAccess>,
}

impl ClientSession {
    fn new() -> Self {
        Self {
            valuation_subscriptions: HashSet::new(),
            limits: BookTopic::new(MAX_LIMIT_SUBSCRIPTIONS, TOO_MANY_LIMIT_BOOKS),
            books: BookTopic::new(MAX_BOOK_SUBSCRIPTIONS, TOO_MANY_SUMMARY_BOOKS),
            rfqs: BookTopic::new(MAX_RFQ_SUBSCRIPTIONS, TOO_MANY_RFQ_BOOKS),
            summary_access: HashMap::new(),
        }
    }

    /// Each topic that follows books.
    fn book_topics(&self) -> [&BookTopic; 3] {
        [&self.limits, &self.books, &self.rfqs]
    }

    /// Topics followed: instruments and each topic's books together, as the subscription
    /// quota counts them.
    fn topic_count(&self) -> usize {
        self.valuation_subscriptions.len()
            + self
                .book_topics()
                .iter()
                .map(|topic| topic.followed.len())
                .sum::<usize>()
    }
}

/// The books a session follows on one topic.
struct BookTopic {
    followed: HashSet<String>,
    /// The most it follows, and the most a message lists.
    most: usize,
    /// Why more is refused.
    too_many: &'static str,
}

impl BookTopic {
    fn new(most: usize, too_many: &'static str) -> Self {
        Self {
            followed: HashSet::new(),
            most,
            too_many,
        }
    }

    /// Follow `book_ids` too: the distinct books listed. Listing a book already followed
    /// costs nothing.
    fn follow(&mut self, book_ids: Vec<String>) -> Result<HashSet<String>, String> {
        if book_ids.len() > self.most {
            return Err(self.too_many.into());
        }
        for id in &book_ids {
            crate::books::book_id(id.clone()).map_err(|_| "invalid book identity")?;
        }
        let listed: HashSet<String> = book_ids.into_iter().collect();
        let added = listed
            .iter()
            .filter(|id| !self.followed.contains(*id))
            .count();
        if self.followed.len().saturating_add(added) > self.most {
            return Err(self.too_many.into());
        }
        self.followed.extend(listed.iter().cloned());
        Ok(listed)
    }

    /// Stop following `book_ids`: how many were listed.
    fn unfollow(&mut self, book_ids: &[String]) -> Result<usize, String> {
        if book_ids.len() > self.most {
            return Err(self.too_many.into());
        }
        for id in book_ids {
            self.followed.remove(id);
        }
        Ok(book_ids.len())
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
    books: Option<Extension<BookFeed>>,
    desk: Option<Extension<Arc<RfqDesk>>>,
) -> impl IntoResponse {
    let profile = profile.map_or(ServerProfile::Production, |Extension(profile)| profile);
    let Some(Extension(authorization)) = authorization else {
        return crate::access::http::response(AccessError::Unauthenticated);
    };
    let limits = limits.map(|Extension(feed)| feed);
    let books = books.map(|Extension(feed)| feed);
    let desk = desk.map(|Extension(desk)| desk);
    ws.protocols([STREAM_PROTOCOL])
        .max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| {
            handle_socket(socket, state, profile, authorization, limits, books, desk)
        })
        .into_response()
}

/// The next frame on `feed`, or never without one: a server without a limit monitor or an
/// RFQ desk has no such feed.
async fn next_on<T: Clone>(
    feed: &mut Option<broadcast::Receiver<T>>,
) -> Result<T, broadcast::error::RecvError> {
    match feed {
        Some(feed) => feed.recv().await,
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
    books: Option<BookFeed>,
    desk: Option<Arc<RfqDesk>>,
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
    let mut session = ClientSession::new();

    // The runtime results channel is the only source of streamed messages.
    let mut valuation_rx = state.engine.subscribe_results();
    let mut limit_rx = limits.as_ref().map(LimitFeed::subscribe);
    let mut book_rx = books.as_ref().map(BookFeed::subscribe);
    let mut rfq_rx = desk.as_ref().map(|desk| desk.subscribe());
    let streams = Streams {
        limits: limits.as_ref(),
        books: books.as_ref(),
        desk: desk.as_deref(),
    };
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
                        Ok(context) => {
                            authorization.context = context;
                            session.summary_access.clear();
                        }
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
                        if subscribed && send_message(&mut sender, &ServerMessage::Valuation(result), profile, &authorization).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if send_message(&mut sender, &valuations_resync(), profile, &authorization).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            frame = next_on::<Arc<LimitFrame>>(&mut limit_rx) => {
                match frame {
                    Ok(frame) => {
                        let book = frame.change.book_id.as_str();
                        if session.limits.followed.contains(book) {
                            // As the status route: the book and every instrument it holds or a limit names.
                            let message = if limit_permission(book, frame.instruments.iter(), &authorization).is_ok() {
                                ServerMessage::LimitStatus(frame.change.clone())
                            } else {
                                limit_forbidden(book)
                            };
                            if send_message(&mut sender, &message, profile, &authorization).await.is_err() { break; }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if !session.limits.followed.is_empty() && send_message(&mut sender, &limits_resync(), profile, &authorization).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Closed) => limit_rx = None,
                }
            }
            frame = next_on::<Arc<BookFrame>>(&mut book_rx) => {
                match frame {
                    Ok(frame) => {
                        // As P&L: the book and every instrument it holds, a limit names or its close holds.
                        if session.books.followed.contains(frame.summary.book_id.as_str()) && send_message(&mut sender, &summary_message(&frame, &authorization, &mut session.summary_access), profile, &authorization).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if !session.books.followed.is_empty() && send_message(&mut sender, &books_resync(), profile, &authorization).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Closed) => book_rx = None,
                }
            }
            frame = next_on::<Arc<RfqFrame>>(&mut rfq_rx) => {
                match frame {
                    Ok(frame) => {
                        let book = frame.rfq.book_id();
                        if session.rfqs.followed.contains(book.as_str()) {
                            // As the list reads it. The book's instruments are read once, by the
                            // first session to send the frame, and each session checks its own access.
                            let instruments = frame.book_instruments.get_or_init(|| book_instruments(state.engine.reference_repository().as_ref(), book)).await;
                            let message = rfq_message(frame.rfq.clone(), instruments, &authorization);
                            if send_message(&mut sender, &message, profile, &authorization).await.is_err() { break; }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if !session.rfqs.followed.is_empty() && send_message(&mut sender, &rfqs_resync(), profile, &authorization).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Closed) => rfq_rx = None,
                }
            }
            // Handle incoming messages from client
            message = receiver.next() => {
                let Some(msg) = message else { break; };
                let admission = match authorization.access.admit_message(&authorization.context) { Ok(admission) => admission, Err(_) => { let _ = tokio::time::timeout(Duration::from_secs(2), sender.send(Message::Close(None))).await; break; } };
                match msg {
                    Ok(Message::Text(text)) => {
                        if handle_client_payload(Ok(&text), admission, &mut session, &mut sender, profile, &authorization, &state, streams).await.is_err() { break; }
                    }
                    Ok(Message::Binary(data)) => {
                        let text = std::str::from_utf8(&data).map_err(|_| "INVALID_MESSAGE".to_owned());
                        if handle_client_payload(text, admission, &mut session, &mut sender, profile, &authorization, &state, streams).await.is_err() { break; }
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

/// The feeds a session's subscriptions read, as the server composed them.
#[derive(Clone, Copy)]
struct Streams<'s> {
    /// Recorded limit status changes, from the limit monitor.
    limits: Option<&'s LimitFeed>,
    /// Book summaries, from the limit monitor.
    books: Option<&'s BookFeed>,
    /// Requests for quote.
    desk: Option<&'s RfqDesk>,
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
    state: &AppState,
    streams: Streams<'_>,
) -> Result<(), axum::Error> {
    let result = match text {
        Ok(text) => handle_client_message(text, session, authorization, state, streams).await,
        Err(error) => Err(error),
    };
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

/// Why a valuation message listing more than [`MAX_MESSAGE_INSTRUMENTS`] is refused.
fn per_message() -> String {
    format!("at most {MAX_MESSAGE_INSTRUMENTS} instruments a message")
}

/// The replies to a client message, in order, or the reason it is refused.
async fn handle_client_message(
    text: &str,
    session: &mut ClientSession,
    authorization: &WsAccess,
    state: &AppState,
    streams: Streams<'_>,
) -> Result<Vec<ServerMessage>, String> {
    let engine = &state.engine;
    let msg: ClientMessage =
        serde_json::from_str(text).map_err(|_| "INVALID_MESSAGE".to_owned())?;

    authorize_subscription(&msg, session, authorization).map_err(|error| error.to_string())?;
    let response = match msg {
        ClientMessage::SubscribeValuations { instrument_ids } => {
            let too_many =
                || format!("at most {MAX_VALUATION_SUBSCRIPTIONS} valuation subscriptions");
            if instrument_ids.len() > MAX_MESSAGE_INSTRUMENTS {
                return Err(per_message());
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
            // Each listed instrument's current result, then the acknowledgement. The
            // results are read after the subscription is in place, so a result published
            // meanwhile is either in this read or follows on the stream. A frame still
            // queued on the stream can be older than the one sent here; the client keeps
            // the newer by (runtime_id, revision).
            let mut replies: Vec<ServerMessage> = engine
                .runtime_results_of(listed.iter().map(InstrumentId::as_str))
                .into_iter()
                .map(ServerMessage::Valuation)
                .collect();
            replies.push(ServerMessage::Subscribed {
                subscription_type: "valuations".into(),
                count,
            });
            return Ok(replies);
        }
        ClientMessage::UnsubscribeValuations { instrument_ids } => {
            if instrument_ids.len() > MAX_MESSAGE_INSTRUMENTS {
                return Err(per_message());
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
            if streams.limits.is_none() {
                return Err(NOT_MONITORED.into());
            }
            ServerMessage::Subscribed {
                subscription_type: "limits".into(),
                count: session.limits.follow(book_ids)?.len(),
            }
        }
        ClientMessage::UnsubscribeLimits { book_ids } => ServerMessage::Unsubscribed {
            subscription_type: "limits".into(),
            count: session.limits.unfollow(&book_ids)?,
        },
        ClientMessage::SubscribeBooks { book_ids } => {
            let Some(books) = streams.books else {
                return Err(NOT_MONITORED.into());
            };
            let listed = session.books.follow(book_ids)?;
            // Each listed book's latest summary, then the acknowledgement. They are read after
            // the subscription is in place, so a later one is either in this read or follows on
            // the stream; one still queued on the stream can be older than the one sent here.
            let mut replies: Vec<ServerMessage> = listed
                .iter()
                .filter_map(|book| books.latest(book))
                .map(|frame| summary_message(&frame, authorization, &mut session.summary_access))
                .collect();
            replies.push(ServerMessage::Subscribed {
                subscription_type: "books".into(),
                count: listed.len(),
            });
            return Ok(replies);
        }
        ClientMessage::UnsubscribeBooks { book_ids } => {
            for book in &book_ids {
                session.summary_access.remove(book);
            }
            ServerMessage::Unsubscribed {
                subscription_type: "books".into(),
                count: session.books.unfollow(&book_ids)?,
            }
        }
        ClientMessage::SubscribeRfqs { book_ids } => {
            let Some(desk) = streams.desk else {
                return Err(NO_RFQ_DESK.into());
            };
            let listed = session.rfqs.follow(book_ids)?;
            // Each open request on the listed books, by deadline, then the acknowledgement.
            // They are read after the subscription is in place, so a transition recorded
            // meanwhile is either in this read or follows on the stream; one still queued
            // on the stream can be older than the request sent here.
            let repo = state.engine.reference_repository().as_ref();
            let mut reader =
                RfqReader::new(&authorization.access, Some(&authorization.context), repo);
            let mut replies = Vec::new();
            for rfq in desk.open_rfqs().await {
                if listed.contains(rfq.book_id().as_str()) {
                    let instruments = reader.book_instruments(rfq.book_id()).await;
                    replies.push(rfq_message(rfq, instruments, authorization));
                }
            }
            replies.push(ServerMessage::Subscribed {
                subscription_type: "rfqs".into(),
                count: listed.len(),
            });
            return Ok(replies);
        }
        ClientMessage::UnsubscribeRfqs { book_ids } => ServerMessage::Unsubscribed {
            subscription_type: "rfqs".into(),
            count: session.rfqs.unfollow(&book_ids)?,
        },
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

/// Whether the session may read `rfq`, against its book's `instruments`, as the list reads it.
fn rfq_permission(
    rfq: &Rfq,
    instruments: &[String],
    authorization: &WsAccess,
) -> Result<(), AccessError> {
    readable(
        &authorization.access,
        Some(&authorization.context),
        rfq,
        instruments,
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
    for book in session
        .book_topics()
        .into_iter()
        .flat_map(|topic| &topic.followed)
    {
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
            return authorize_books(book_ids, &session.limits, session, authorization)
        }
        ClientMessage::SubscribeBooks { book_ids } => {
            return authorize_books(book_ids, &session.books, session, authorization)
        }
        ClientMessage::SubscribeRfqs { book_ids } => {
            return authorize_books(book_ids, &session.rfqs, session, authorization)
        }
        ClientMessage::UnsubscribeLimits { book_ids } => {
            return within(book_ids.len(), session.limits.most)
        }
        ClientMessage::UnsubscribeBooks { book_ids } => {
            return within(book_ids.len(), session.books.most)
        }
        ClientMessage::UnsubscribeRfqs { book_ids } => {
            return within(book_ids.len(), session.rfqs.most)
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

/// Whether a message listing `listed` books fits a topic taking at most `most`.
fn within(listed: usize, most: usize) -> Result<(), AccessError> {
    if listed > most {
        return Err(AccessError::Quota);
    }
    Ok(())
}

/// Whether the session may follow these books on `topic` too, within its topic quota.
fn authorize_books(
    ids: &[String],
    topic: &BookTopic,
    session: &ClientSession,
    authorization: &WsAccess,
) -> Result<(), AccessError> {
    within(ids.len(), topic.most)?;
    let mut prospective: HashSet<&str> = topic.followed.iter().map(String::as_str).collect();
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
    let book = match message {
        ServerMessage::LimitStatus(change) => Some(&change.book_id),
        ServerMessage::BookSummary(summary) => Some(&summary.book_id),
        _ => None,
    };
    if let Some(book) = book {
        limit_permission(book.as_str(), [], authorization)?;
    }
    // A request's grants change only with the policy's version, as while a seed is sent;
    // then its read check is made again, against the instruments it read.
    if let ServerMessage::Rfq(sent) = message {
        if authorization.access.policy_version() != sent.policy_version {
            rfq_permission(&sent.rfq, &sent.book_instruments, authorization)?;
        }
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
    use crate::access::policy::{Grant, IdentityKind, PolicyDocument, Role};
    use crate::access::{AccessControl, BearerToken, IdentityVerifier, VerifiedIdentity};

    const ISSUER: &str = "https://identity.example.invalid/issuer";

    /// Every token is the reader's.
    struct ReaderTokens;

    #[async_trait::async_trait]
    impl IdentityVerifier for ReaderTokens {
        async fn verify(&self, _: &BearerToken) -> Result<VerifiedIdentity, AccessError> {
            Ok(VerifiedIdentity {
                issuer: ISSUER.into(),
                subject: "reader".into(),
                audience: "convex".into(),
                expires_at: i64::MAX,
                scopes: ["convex.api".to_owned()].into(),
            })
        }

        async fn probe(&self) -> Result<(), AccessError> {
            Ok(())
        }
    }

    /// Revision `version`: the reader reads book BOOK-A and `instruments`.
    fn reader_policy(version: u64, instruments: &[&str]) -> PolicyDocument {
        let valid_until = chrono::Utc::now().timestamp() + 3600;
        PolicyDocument {
            schema_version: 1,
            version,
            issuer: ISSUER.into(),
            audience: "convex".into(),
            valid_until,
            actor: "operator".into(),
            reason: "the reader's grants".into(),
            grants: vec![Grant {
                subject: "reader".into(),
                kind: IdentityKind::Service,
                roles: vec![Role::Reader],
                data: crate::limits::STATUS_DATA.to_vec(),
                resources: [
                    (ResourceKind::Book, vec!["BOOK-A".to_owned()]),
                    (
                        ResourceKind::Instrument,
                        instruments.iter().map(|id| (*id).to_owned()).collect(),
                    ),
                ]
                .into(),
                token_scopes: vec!["convex.api".into()],
                valid_until,
                quotas: Default::default(),
            }],
        }
    }

    /// A request for quote is checked again as it is sent, against the book's instruments
    /// its check read: a grant withdrawn since, as while a seed is sent, withholds it.
    #[tokio::test]
    async fn a_request_for_quote_is_checked_again_as_it_is_sent() {
        let access = AccessControl::new(
            Arc::new(ReaderTokens),
            reader_policy(1, &["BOND-001", "BOND-HELD"]),
        )
        .unwrap();
        let context = access
            .authenticate(Some(BearerToken::parse("token").unwrap()))
            .await
            .unwrap();
        let admission = access
            .admit(&context, true, access.admit_attempt().unwrap())
            .unwrap();
        let authorization = WsAccess {
            access: access.clone(),
            context,
            _admission: Arc::new(admission),
        };
        let request = serde_json::from_value(serde_json::json!({
            "source_id": "venue-sim", "rfq_id": "RFQ-1", "client_id": "CLIENT-01",
            "client_tier": "1", "instrument_id": "BOND-001", "side": "buy",
            "face": "1000000", "settlement_date": "2026-11-25", "book_id": "BOOK-A",
            "deadline": "2026-11-23T12:00:00Z",
        }))
        .unwrap();
        let rfq = Arc::new(crate::rfq_desk::held(
            request,
            convex_core::contracts::InstrumentId::new("BOND-001").unwrap(),
        ));
        let message = rfq_message(
            rfq,
            &Ok(Arc::from(["BOND-HELD".to_owned()])),
            &authorization,
        );
        assert!(matches!(message, ServerMessage::Rfq(_)));
        assert_eq!(authorize_delivery(&message, &authorization), Ok(()));
        // The book's instrument withdrawn, then the bond.
        for (version, instruments) in [(2, ["BOND-001"]), (3, ["BOND-HELD"])] {
            access
                .replace_policy(version - 1, reader_policy(version, &instruments))
                .unwrap();
            assert_eq!(
                authorize_delivery(&message, &authorization),
                Err(AccessError::Forbidden)
            );
        }
    }

    /// A book's summary access is checked once for the instruments its frames name under one
    /// policy, and again when either changes.
    #[tokio::test]
    async fn a_summary_is_checked_again_under_a_new_policy_or_new_instruments() {
        use crate::book_close::NoBookPnlReason;
        use crate::book_summary::BookPnl;
        use convex_core::contracts::{UtcTimestamp, Version};
        use convex_engine::ports::limits::{BookRisk, UnknownReason};
        use convex_engine::ports::reference_master::BookId;

        let access = AccessControl::new(
            Arc::new(ReaderTokens),
            reader_policy(1, &["BOND-001", "BOND-002"]),
        )
        .unwrap();
        let context = access
            .authenticate(Some(BearerToken::parse("token").unwrap()))
            .await
            .unwrap();
        let admission = access
            .admit(&context, true, access.admit_attempt().unwrap())
            .unwrap();
        let authorization = WsAccess {
            access: access.clone(),
            context,
            _admission: Arc::new(admission),
        };
        let at = UtcTimestamp::parse("2026-11-25T12:00:00Z").unwrap();
        let summary = Arc::new(BookSummary {
            book_id: BookId::new("BOOK-A").unwrap(),
            book_version: Version::new(1).unwrap(),
            limit_set_version: None,
            sequence: 1,
            monitor_started_at: at,
            evaluated_at: at,
            currency: None,
            settlement_date: None,
            risk: BookRisk::Unknown {
                reason: UnknownReason::BookCurrencyRequired {},
            },
            pnl: BookPnl::Unknown {
                reason: NoBookPnlReason::NoClose,
            },
            limits: Vec::new(),
        });
        let frame = |instrument: &str| BookFrame {
            summary: summary.clone(),
            instruments: Arc::from([instrument.to_owned()]),
        };
        let is_sent = |message: ServerMessage| matches!(message, ServerMessage::BookSummary(_));
        let mut checked = HashMap::new();
        let held = frame("BOND-001");
        assert!(is_sent(summary_message(
            &held,
            &authorization,
            &mut checked
        )));
        access
            .replace_policy(1, reader_policy(2, &["BOND-002"]))
            .unwrap();
        assert!(!is_sent(summary_message(
            &held,
            &authorization,
            &mut checked
        )));
        assert!(is_sent(summary_message(
            &frame("BOND-002"),
            &authorization,
            &mut checked
        )));
    }

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
            quote_input: None,
            last_good: None,
            retained_quality: None,
        }
    }

    #[test]
    fn a_valuation_too_large_for_one_message_is_sent_as_an_error_naming_it() {
        let fits = ServerMessage::Valuation(Arc::new(valuation(8)));
        let sent: serde_json::Value =
            serde_json::from_str(&encode(&fits, ServerProfile::Production).unwrap()).unwrap();
        assert_eq!(sent["type"], "valuation");

        // 256 records with 128-character ids: the most a lineage holds, about 77 KiB.
        let large = ServerMessage::Valuation(Arc::new(valuation(256)));
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
        // The Excel add-in reads the instrument, runtime and revision from this
        // opening (excel/Convex.Excel/server/ServerWire.cs `TooLarge`).
        assert!(
            message
                .starts_with("The result of instrument BOND-001 (runtime runtime-1, revision 7) "),
            "{message}"
        );
        assert!(message.contains("/api/v1/runtime/results"), "{message}");

        let demo: serde_json::Value =
            serde_json::from_str(&encode(&large, ServerProfile::Demo).unwrap()).unwrap();
        assert_eq!(demo["code"], "RESULT_TOO_LARGE");
        assert_eq!(demo["data_quality"], "demo");
    }

    /// The Excel add-in reads these openings (`excel/Convex.Excel/server/LimitWire.cs`
    /// `LimitErrors`): a limits error has the same code as a valuations one, and a
    /// book's `FORBIDDEN` answers no client message, so only the text says which it is.
    #[test]
    fn the_limit_stream_errors_keep_the_text_the_excel_add_in_reads() {
        let text = |message: ServerMessage| match message {
            ServerMessage::Error { code, message } => (code, message),
            other => panic!("{other:?}"),
        };
        let (code, message) = text(limit_forbidden("BOOK-A"));
        assert_eq!(code, "FORBIDDEN");
        assert!(
            message.starts_with("limit status of book BOOK-A needs read access"),
            "{message}"
        );
        let (code, message) = text(limits_resync());
        assert_eq!(code, "RESYNC_REQUIRED");
        assert!(message.starts_with("Read /api/v1/books/"), "{message}");
        let (code, message) = text(valuations_resync());
        assert_eq!(code, "RESYNC_REQUIRED");
        assert!(message.starts_with("Subscribe again"), "{message}");
        assert_eq!(NOT_MONITORED, "this server does not monitor limits");
    }

    #[test]
    fn any_other_message_too_large_for_one_message_is_refused() {
        let error = ServerMessage::Error {
            code: "INVALID_MESSAGE".into(),
            message: "x".repeat(MAX_MESSAGE_BYTES),
        };
        assert!(encode(&error, ServerProfile::Production).is_err());
    }

    /// A summary at the bounds fits one message: 64 limits, every identity at its longest,
    /// and each risk measure unknown, naming as many instruments as a reason lists.
    #[test]
    fn the_largest_book_summary_fits_one_message() {
        use crate::book_summary::{BookPnl, LimitStatusNow};
        use convex_core::contracts::{BusinessDate, ExactDecimal, UtcTimestamp, Version};
        use convex_engine::ports::limits::{
            BookRisk, InstrumentList, KeyRateDv01, LimitStatus, RiskMeasure, RiskMeasures,
            UnknownReason, MAX_BOOK_LIMITS, MAX_LISTED_INSTRUMENTS,
        };
        use convex_engine::ports::reference_master::BookId;
        use rust_decimal::Decimal;

        let longest = |prefix: String| format!("{prefix}{}", "x".repeat(128 - prefix.len()));
        let widest = ExactDecimal::new(Decimal::MIN);
        let finest = ExactDecimal::new(Decimal::new(-1, 28) - Decimal::ONE);
        let instruments: Vec<_> = (0..MAX_LISTED_INSTRUMENTS)
            .map(|i| {
                convex_core::contracts::InstrumentId::new(longest(format!("I{i:02}"))).unwrap()
            })
            .collect();
        let unknown = |measure: &str| RiskMeasure::Unknown {
            reason: UnknownReason::MeasureNotComputed {
                measure: measure.into(),
                instruments: InstrumentList {
                    count: u32::MAX,
                    first: instruments.clone(),
                },
            },
        };
        let version = Version::new(u64::MAX).unwrap();
        let at = UtcTimestamp::parse("2026-11-25T23:59:59.999Z").unwrap();
        let date =
            BusinessDate::try_from(convex_core::Date::from_ymd(2026, 11, 25).unwrap()).unwrap();
        let summary = BookSummary {
            book_id: BookId::new(longest("B".into())).unwrap(),
            book_version: version,
            limit_set_version: Some(version),
            sequence: u64::MAX,
            monitor_started_at: at,
            evaluated_at: at,
            currency: Some(convex_core::Currency::USD),
            settlement_date: Some(date),
            risk: BookRisk::Measured(Box::new(RiskMeasures {
                market_value: RiskMeasure::Known { value: widest },
                gross_market_value: RiskMeasure::Known { value: finest },
                dv01: unknown("dv01_mid"),
                cs01: unknown("cs01_mid"),
                modified_duration: unknown("modified_duration_mid"),
                key_rate_dv01: convex_engine::pricing_router::runtime_key_rate_tenors()
                    .into_iter()
                    .map(|tenor| KeyRateDv01 {
                        dv01: unknown(&format!("krd_{}_mid", tenor.as_str().to_lowercase())),
                        tenor,
                    })
                    .collect(),
            })),
            pnl: BookPnl::Measured {
                close_version: version,
                close_date: date,
                pnl: widest,
                entitled_cash: finest,
                excluded_count: 1000,
            },
            limits: (0..MAX_BOOK_LIMITS)
                .map(|i| LimitStatusNow {
                    limit_id: convex_engine::ports::limits::LimitId::new(longest(format!(
                        "L{i:02}"
                    )))
                    .unwrap(),
                    status: LimitStatus::Breach,
                    value: Some(widest),
                    utilisation: Some(finest),
                })
                .collect(),
        };
        let message = ServerMessage::BookSummary(Arc::new(summary));
        let json = encode(&message, ServerProfile::Production).unwrap();
        assert!(
            json.len() < MAX_MESSAGE_BYTES * 3 / 4,
            "{} bytes",
            json.len()
        );
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
