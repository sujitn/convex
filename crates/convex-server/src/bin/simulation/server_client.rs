//! What the RFQ and BMS simulators share: their arguments, the server's HTTP API under the
//! caller's token, and the `rfq` topic.
//!
//! The token, when access is required, is read from `CONVEX_SIM_TOKEN`, so it is not on
//! the command line. The server listens on loopback only, so the topic is plain `ws://`.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::str::FromStr;
use std::time::Duration;

use convex_engine::ports::rfq::{RfqRequest, RfqState};
use futures::{SinkExt, StreamExt};
use reqwest::header::RETRY_AFTER;
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

/// The environment variable a bearer token is read from.
const TOKEN_VARIABLE: &str = "CONVEX_SIM_TOKEN";
/// Times a request is sent while no answer comes.
const SENDS: usize = 5;
/// The longest wait for a server too busy for a request.
const LONGEST_WAIT: Duration = Duration::from_secs(60);

/// `--name value` pairs.
pub struct Arguments(BTreeMap<String, String>);

impl Arguments {
    /// The process's arguments, each named in `known`.
    pub fn parse(known: &[&str]) -> Result<Self, String> {
        let mut pairs = BTreeMap::new();
        let mut args = std::env::args().skip(1);
        while let Some(name) = args.next() {
            let Some(name) = name.strip_prefix("--") else {
                return Err(format!("expected --name, found {name}"));
            };
            if !known.contains(&name) {
                return Err(format!("--{name} is not an option"));
            }
            let value = args
                .next()
                .ok_or_else(|| format!("--{name} needs a value"))?;
            if pairs.insert(name.to_owned(), value).is_some() {
                return Err(format!("--{name} is given twice"));
            }
        }
        Ok(Self(pairs))
    }

    pub fn required(&self, name: &str) -> Result<&str, String> {
        self.0
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("--{name} is required"))
    }

    /// The value of `--name`, or `default` without one.
    pub fn optional<T: FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        match self.0.get(name) {
            Some(value) => value
                .parse()
                .map_err(|_| format!("--{name} {value} is not valid")),
            None => Ok(default),
        }
    }

    /// The comma-separated values of `--name`, at least one.
    pub fn list(&self, name: &str) -> Result<Vec<String>, String> {
        let values: Vec<String> = self
            .required(name)?
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();
        if values.is_empty() {
            return Err(format!("--{name} lists nothing"));
        }
        Ok(values)
    }
}

/// The server, as the simulator's identity calls it.
#[derive(Clone)]
pub struct ServerClient {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
}

impl ServerClient {
    /// The server at `base`, `http://host:port`.
    pub fn new(base: &str) -> Result<Self, String> {
        if !base.starts_with("http://") {
            return Err("--server is http://host:port: the server listens on loopback".into());
        }
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_owned(),
            token: std::env::var(TOKEN_VARIABLE).ok(),
        })
    }

    /// A request to `path`, under the token if one is set.
    pub fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let request = self.http.request(method, format!("{}{path}", self.base));
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    /// The request `rfq_key` as its entries leave it.
    pub async fn rfq(&self, rfq_key: &str) -> Result<RfqRead, String> {
        /// An entry of `GET /api/v1/rfqs/:rfq_key`; only the first, the receipt, names the
        /// request and its bond.
        #[derive(Deserialize)]
        struct Entry {
            record_id: u64,
            state: RfqState,
            request: Option<RfqRequest>,
            instrument_id: Option<String>,
        }
        #[derive(Deserialize)]
        struct Entries {
            records: Vec<Entry>,
        }
        let response =
            send_until_answered(self.request(Method::GET, &format!("/api/v1/rfqs/{rfq_key}")))
                .await?;
        match response.status() {
            StatusCode::NOT_FOUND => return Ok(RfqRead::Unknown),
            StatusCode::FORBIDDEN => return Ok(RfqRead::Withheld),
            _ => {}
        }
        let entries: Entries = response
            .error_for_status()
            .map_err(|error| format!("a request could not be read: {error}"))?
            .json()
            .await
            .map_err(|error| format!("a request is unreadable: {error}"))?;
        let mut records = entries.records.into_iter();
        let Some(receipt) = records.next() else {
            return Ok(RfqRead::Unknown);
        };
        let (Some(request), Some(instrument_id)) = (receipt.request, receipt.instrument_id) else {
            return Err("a request's first entry is not its receipt".into());
        };
        let last = records.last();
        Ok(RfqRead::Row(Box::new(RfqRow {
            rfq_key: rfq_key.to_owned(),
            record_id: last
                .as_ref()
                .map_or(receipt.record_id, |last| last.record_id),
            state: last.map_or(receipt.state, |last| last.state),
            request,
            instrument_id,
        })))
    }

    /// The `rfq` topic on `books`, opened and followed: its seed is the first rows `next`
    /// gives.
    pub async fn rfq_topic(&self, books: &[String]) -> Result<RfqTopic, String> {
        let url = format!("ws{}/api/v1/ws", &self.base["http".len()..]);
        let mut request = url
            .into_client_request()
            .map_err(|error| error.to_string())?;
        if let Some(token) = &self.token {
            let value = format!("Bearer {token}")
                .parse()
                .map_err(|_| "the token is not a header value".to_owned())?;
            request.headers_mut().insert("authorization", value);
        }
        let (socket, _) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|error| format!("the stream could not be opened: {error}"))?;
        let mut topic = RfqTopic {
            socket,
            books: books.to_vec(),
            seen: SeenRows::default(),
            ready: VecDeque::new(),
        };
        topic.catch_up(self).await?;
        Ok(topic)
    }
}

/// Send `request`, and again as it was while no answer comes: no reply, or the server too
/// busy for it saying when to send again (`Retry-After`), after that wait. A refusal that
/// says nothing of when, as a full desk's, is an answer. The server answers a committed
/// write sent again under its idempotency key from the receipt it keeps.
pub async fn send_until_answered(request: RequestBuilder) -> Result<Response, String> {
    let mut failure = String::new();
    for _ in 0..SENDS {
        let attempt = request
            .try_clone()
            .ok_or("a request cannot be sent again")?;
        match attempt.send().await {
            Ok(response) if response.status() == StatusCode::TOO_MANY_REQUESTS => {
                let Some(wait) = response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|seconds| seconds.to_str().ok()?.parse().ok())
                    .map(Duration::from_secs)
                else {
                    return Ok(response);
                };
                failure = "the server stayed too busy".into();
                tokio::time::sleep(wait.min(LONGEST_WAIT)).await;
            }
            Ok(response) => return Ok(response),
            Err(error) => failure = error.to_string(),
        }
    }
    Err(format!("a request was not answered: {failure}"))
}

/// A request as `GET /api/v1/rfqs/:rfq_key` gives it.
pub enum RfqRead {
    /// Its row, as its entries leave it.
    Row(Box<RfqRow>),
    /// The server knows no request by the key.
    Unknown,
    /// The caller may not read it.
    Withheld,
}

/// A request for quote as the topic shows it.
#[derive(Debug, Clone, Deserialize)]
pub struct RfqRow {
    pub rfq_key: String,
    pub record_id: u64,
    pub state: RfqState,
    pub request: RfqRequest,
    /// The bond, as the server resolved it at receipt.
    pub instrument_id: String,
}

/// The latest row given of a request.
struct Seen {
    record_id: u64,
    is_final: bool,
}

/// Of each request's rows, the later only; a final row is the last. Each request seen is
/// remembered for the run, so a final row read over HTTP and the same row still on the
/// stream are given once.
#[derive(Default)]
struct SeenRows(HashMap<String, Seen>);

impl SeenRows {
    /// Whether `row` is later than any given of its request; if so it is now the latest.
    fn is_later(&mut self, row: &RfqRow) -> bool {
        if let Some(seen) = self.0.get(&row.rfq_key) {
            if seen.is_final || row.record_id <= seen.record_id {
                return false;
            }
        }
        self.0.insert(
            row.rfq_key.clone(),
            Seen {
                record_id: row.record_id,
                is_final: row.state.is_final(),
            },
        );
        true
    }

    /// Each request whose latest row given is open.
    fn open_keys(&self) -> impl Iterator<Item = &String> {
        self.0
            .iter()
            .filter(|(_, seen)| !seen.is_final)
            .map(|(key, _)| key)
    }
}

/// The `rfq` topic on some books, giving each request's rows in order, the later only.
/// After falling behind it subscribes again, and reads each request it saw open that the
/// new subscription does not cover, so a request that went final meanwhile still gives its
/// final row.
pub struct RfqTopic {
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    books: Vec<String>,
    seen: SeenRows,
    /// Rows to give before reading the stream again.
    ready: VecDeque<RfqRow>,
}

impl RfqTopic {
    /// How many requests are open as far as the topic has shown.
    pub fn open_count(&self) -> usize {
        self.seen.open_keys().count()
    }

    /// The next row later than any given of its request. What it has read is lost if it is
    /// dropped before it returns, as a `select!` branch may be; read it on a task of its
    /// own there.
    pub async fn next(&mut self, server: &ServerClient) -> Result<RfqRow, String> {
        loop {
            if let Some(row) = self.ready.pop_front() {
                return Ok(row);
            }
            let frame = self.frame().await?;
            match frame["type"].as_str() {
                Some("rfq") => self.take_if_later(read_row(frame)?),
                Some("error") if frame["code"] == "RESYNC_REQUIRED" => {
                    self.catch_up(server).await?
                }
                Some("rfq_withheld" | "error") => {
                    eprintln!("{}", json!({"event": "stream_error", "frame": frame}))
                }
                _ => {}
            }
        }
    }

    /// Subscribe, then read each request still open as given that the subscription does
    /// not cover.
    async fn catch_up(&mut self, server: &ServerClient) -> Result<(), String> {
        let covered = self.subscribe().await?;
        let unread: Vec<String> = self
            .seen
            .open_keys()
            .filter(|key| !covered.contains(*key))
            .cloned()
            .collect();
        for rfq_key in unread {
            match server.rfq(&rfq_key).await? {
                RfqRead::Row(row) => self.take_if_later(*row),
                RfqRead::Unknown => {}
                RfqRead::Withheld => {
                    eprintln!("{}", json!({"event": "rfq_withheld", "rfq_key": rfq_key}))
                }
            }
        }
        Ok(())
    }

    /// Queue `row` if it is later than any given of its request.
    fn take_if_later(&mut self, row: RfqRow) {
        if self.seen.is_later(&row) {
            self.ready.push_back(row);
        }
    }

    /// Subscribe to the books and read until the acknowledgement, taking each later row:
    /// any still on the stream, then each open request on the books, by deadline. Returns
    /// the requests named after the last `RESYNC_REQUIRED`, whose later rows follow on the
    /// stream: the server sends the seed and its acknowledgement together, after any
    /// `RESYNC_REQUIRED`, so only rows before one may be missing later ones. A refusal
    /// ends the simulation.
    async fn subscribe(&mut self) -> Result<HashSet<String>, String> {
        let subscribe = json!({"type": "subscribe_rfqs", "book_ids": self.books});
        self.socket
            .send(Message::Text(subscribe.to_string().into()))
            .await
            .map_err(|error| error.to_string())?;
        let mut covered = HashSet::new();
        loop {
            let frame = self.frame().await?;
            match frame["type"].as_str() {
                Some("subscribed") => return Ok(covered),
                Some("rfq") => {
                    let row = read_row(frame)?;
                    covered.insert(row.rfq_key.clone());
                    self.take_if_later(row);
                }
                Some("error") if frame["code"] == "RESYNC_REQUIRED" => covered.clear(),
                Some("error") => return Err(format!("the subscription was refused: {frame}")),
                Some("rfq_withheld") => {
                    eprintln!("{}", json!({"event": "stream_error", "frame": frame}))
                }
                _ => {}
            }
        }
    }

    /// The next text frame other than a heartbeat.
    async fn frame(&mut self) -> Result<Value, String> {
        loop {
            match self.socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    let frame: Value = serde_json::from_str(&text)
                        .map_err(|error| format!("a frame is unreadable: {error}"))?;
                    if frame["type"] != "heartbeat" {
                        return Ok(frame);
                    }
                }
                Some(Ok(Message::Close(_))) | None => return Err("the stream closed".into()),
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(format!("the stream failed: {error}")),
            }
        }
    }
}

fn read_row(frame: Value) -> Result<RfqRow, String> {
    serde_json::from_value(frame).map_err(|error| format!("an rfq message is unreadable: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(rfq_key: &str, record_id: u64, state: &str) -> RfqRow {
        serde_json::from_value(json!({
            "rfq_key": rfq_key, "record_id": record_id, "state": state,
            "instrument_id": "BOND-001",
            "request": {
                "source_id": "venue-sim", "rfq_id": "RFQ-1", "client_id": "CLIENT-01",
                "client_tier": "1", "instrument_id": "BOND-001", "side": "buy",
                "face": "1000000", "settlement_date": "2026-11-25", "book_id": "BOOK-A",
                "deadline": "2026-11-23T12:00:00Z",
            },
        }))
        .unwrap()
    }

    /// A row no later than the latest of its request is not given again, and nothing
    /// after its final row is: not the same final row still on the stream after it was
    /// read over HTTP.
    #[test]
    fn each_request_gives_its_later_rows_and_its_final_row_once() {
        let mut seen = SeenRows::default();
        assert!(seen.is_later(&row("a", 2, "priced")));
        assert!(seen.is_later(&row("b", 3, "priced")));
        assert!(!seen.is_later(&row("a", 2, "priced")));
        assert!(seen.is_later(&row("a", 5, "quoted")));
        assert!(!seen.is_later(&row("a", 4, "priced")));
        assert_eq!(seen.open_keys().count(), 2);
        assert!(seen.is_later(&row("a", 7, "done")));
        assert!(!seen.is_later(&row("a", 7, "done")));
        assert!(!seen.is_later(&row("a", 9, "done")));
        assert_eq!(seen.open_keys().collect::<Vec<_>>(), ["b"]);
    }
}
