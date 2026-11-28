//! The `rfq` topic as the RFQ and BMS simulators follow it, and a request read over HTTP.

use std::collections::{HashMap, HashSet, VecDeque};

use convex_engine::ports::rfq::{RfqRequest, RfqState};
use futures::{SinkExt, StreamExt};
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use crate::server_client::{send_until_answered, ServerClient};

/// The request `rfq_key` as its entries leave it.
pub async fn read_rfq(server: &ServerClient, rfq_key: &str) -> Result<RfqRead, String> {
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
        send_until_answered(server.request(Method::GET, &format!("/api/v1/rfqs/{rfq_key}")))
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
    /// The topic on `books`, opened and followed: its seed is the first rows `next` gives.
    pub async fn open(server: &ServerClient, books: &[String]) -> Result<Self, String> {
        let (socket, _) = tokio_tungstenite::connect_async(server.stream_request()?)
            .await
            .map_err(|error| format!("the stream could not be opened: {error}"))?;
        let mut topic = RfqTopic {
            socket,
            books: books.to_vec(),
            seen: SeenRows::default(),
            ready: VecDeque::new(),
        };
        topic.catch_up(server).await?;
        Ok(topic)
    }

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
            match read_rfq(server, &rfq_key).await? {
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
