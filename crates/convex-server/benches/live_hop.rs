//! P13a (G37): market data to UI. A `live` profile server over the real composition on a
//! loopback listener, with an in-process HTTP and WebSocket client. A case is timed from
//! `POST /api/v1/market/quotes` until the session holds the `valuation` frame of the tick
//! that admitted the last posted quote. One `Instant` spans the path, so no wall-clock
//! stamp is compared. It measures the server's own cost on loopback, not a network; the
//! client's request serialization and frame parsing are inside the time.
//!
//! - `post/256`: one quote until its `202`, the ingress alone.
//! - `1` and `burst1/256`: one quote until its frame, at 1 and 256 bonds. A tick values
//!   and publishes only the quoted bond (D55).
//! - `batch256/256`: one `POST` of 256 quotes, one per bond, until the frame of the tick
//!   that admitted the last; reported as quotes/s.
//! - `burst1/256/sessions8`: one quote until each of eight sessions holds its frame. Each
//!   session serializes its own copy of the record.
//!
//! An error frame, such as `RESYNC_REQUIRED` after a lagging session (G38), fails the case.
//!
//! Run with: cargo bench -p convex-server --bench live_hop --profile ci
//! Report medians only; CI builds it and sets no timing threshold.

#[path = "../tests/support/live_server.rs"]
mod live_server;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use convex_engine::ports::quote_event::{QuoteEvent, QuoteKey};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use futures_util::{SinkExt, StreamExt};
use live_server::{image, wide_live_setup, Running};
use reqwest::StatusCode;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// 256 bonds, `MAX_SNAPSHOT_INSTRUMENTS`; also the most events one `POST` carries.
const WIDE: usize = 256;
/// Sessions in the fan-out case.
const SESSIONS: usize = 8;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// The fields of a server frame the client reads; the parser skips the rest.
#[derive(serde::Deserialize)]
struct Frame {
    #[serde(rename = "type")]
    kind: String,
    generation: Option<u64>,
}

/// Read frames until one satisfies `done`. Any error frame fails the case.
async fn until(socket: &mut Socket, done: impl Fn(&Frame) -> bool) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let message = socket
                .next()
                .await
                .expect("the session stays open")
                .expect("a frame");
            let Message::Text(text) = message else {
                continue;
            };
            let frame: Frame = serde_json::from_str(text.as_str()).unwrap();
            assert_ne!(frame.kind, "error", "{}", text.as_str());
            if done(&frame) {
                return;
            }
        }
    })
    .await
    .expect("a frame within 30 s")
}

/// The frame of the tick that admitted generation `expected` or a later one.
async fn valuation(socket: &mut Socket, expected: u64) {
    until(socket, |f| {
        f.kind == "valuation" && f.generation.is_some_and(|g| g >= expected)
    })
    .await
}

/// Read until the server answers a ping sent now. Its session loop sends every record it
/// already holds before it reads the ping (the select is biased), so the rest of a wide
/// tick does not spill into the next measurement.
async fn drain(socket: &mut Socket) {
    let ping = serde_json::json!({"type": "ping", "timestamp": 0});
    socket
        .send(Message::Text(ping.to_string().into()))
        .await
        .unwrap();
    until(socket, |f| f.kind == "pong").await
}

/// A Live server over `width` bonds, after every bond's first image.
struct Live {
    server: Running,
    keys: Vec<QuoteKey>,
    sequence: u64,
    /// The last generation the runtime admitted: one per posted quote.
    generation: u64,
    _dir: tempfile::TempDir,
}

impl Live {
    async fn start(width: usize) -> Self {
        let (dir, _, config, keys) = wide_live_setup(width).await;
        let server = Running::start(config).await;
        let generation = server.runtime().await["generation"].as_u64().unwrap();
        let mut live = Self {
            server,
            keys,
            sequence: 0,
            generation,
            _dir: dir,
        };
        let images = live.quotes(width);
        live.post(images).await;
        let expected = live.generation;
        tokio::time::timeout(Duration::from_secs(30), async {
            while live.server.runtime().await["generation"].as_u64() < Some(expected) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("every first image is admitted");
        live.server.wait_ready().await;
        live
    }

    fn ids(&self) -> Vec<String> {
        self.keys
            .iter()
            .map(|k| k.instrument_id.as_str().to_owned())
            .collect()
    }

    /// The next `count` quotes, round-robin over the keys.
    fn quotes(&mut self, count: usize) -> Vec<QuoteEvent> {
        (0..count)
            .map(|_| {
                self.sequence += 1;
                let mut event = image(self.sequence, "99.25");
                event.key = self.keys[self.sequence as usize % self.keys.len()].clone();
                event
            })
            .collect()
    }

    async fn post(&mut self, quotes: Vec<QuoteEvent>) {
        let count = quotes.len() as u64;
        let response = self.server.post_quotes(quotes).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        self.generation += count;
    }

    /// A session subscribed to every bond, past its `connected` and `subscribed` frames.
    async fn session(&self) -> Socket {
        let (mut socket, _) = tokio_tungstenite::connect_async(self.server.ws_url())
            .await
            .unwrap();
        until(&mut socket, |f| f.kind == "connected").await;
        let subscribe = serde_json::json!({
            "type": "subscribe_valuations",
            "instrument_ids": self.ids(),
        });
        socket
            .send(Message::Text(subscribe.to_string().into()))
            .await
            .unwrap();
        until(&mut socket, |f| f.kind == "subscribed").await;
        socket
    }

    /// Post `count` quotes and return the time until every session holds the frame of
    /// the tick that admitted the last; then drain each session, untimed.
    async fn hop(&mut self, sockets: &mut [Socket], count: usize) -> Duration {
        let quotes = self.quotes(count);
        let expected = self.generation + count as u64;
        let started = Instant::now();
        let server = &self.server;
        let (response, elapsed) = tokio::join!(server.post_quotes(quotes), async {
            futures_util::future::join_all(sockets.iter_mut().map(|s| valuation(s, expected)))
                .await;
            started.elapsed()
        });
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        self.generation = expected;
        for socket in sockets.iter_mut() {
            drain(socket).await;
        }
        elapsed
    }

    async fn stop(self, sockets: Vec<Socket>) {
        for mut socket in sockets {
            let _ = socket.close(None).await;
        }
        self.server.stop().await;
    }
}

fn bench_live_hop(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("live_hop");

    let mut wide = rt.block_on(Live::start(WIDE));
    group.throughput(Throughput::Elements(1));
    // No session is open, so no frame waits unread.
    group.bench_function(BenchmarkId::new("post", WIDE), |b| {
        b.iter_custom(|iterations| {
            rt.block_on(async {
                let mut total = Duration::ZERO;
                for _ in 0..iterations {
                    let quotes = wide.quotes(1);
                    let started = Instant::now();
                    wide.post(quotes).await;
                    total += started.elapsed();
                }
                total
            })
        })
    });
    let mut sockets = vec![rt.block_on(wide.session())];
    group.bench_function(BenchmarkId::new("burst1", WIDE), |b| {
        b.iter_custom(|iterations| {
            rt.block_on(async {
                let mut total = Duration::ZERO;
                for _ in 0..iterations {
                    total += wide.hop(&mut sockets, 1).await;
                }
                total
            })
        })
    });
    group.throughput(Throughput::Elements(WIDE as u64));
    group.bench_function(BenchmarkId::new(format!("batch{WIDE}"), WIDE), |b| {
        b.iter_custom(|iterations| {
            rt.block_on(async {
                let mut total = Duration::ZERO;
                for _ in 0..iterations {
                    total += wide.hop(&mut sockets, WIDE).await;
                }
                total
            })
        })
    });
    for _ in 1..SESSIONS {
        sockets.push(rt.block_on(wide.session()));
    }
    group.throughput(Throughput::Elements(1));
    group.bench_function(
        BenchmarkId::new("burst1", format!("{WIDE}/sessions{SESSIONS}")),
        |b| {
            b.iter_custom(|iterations| {
                rt.block_on(async {
                    let mut total = Duration::ZERO;
                    for _ in 0..iterations {
                        total += wide.hop(&mut sockets, 1).await;
                    }
                    total
                })
            })
        },
    );
    rt.block_on(wide.stop(sockets));

    let mut one = rt.block_on(Live::start(1));
    let mut sockets = vec![rt.block_on(one.session())];
    group.bench_function(BenchmarkId::from_parameter(1), |b| {
        b.iter_custom(|iterations| {
            rt.block_on(async {
                let mut total = Duration::ZERO;
                for _ in 0..iterations {
                    total += one.hop(&mut sockets, 1).await;
                }
                total
            })
        })
    });
    rt.block_on(one.stop(sockets));
    group.finish();
}

criterion_group!(benches, bench_live_hop);
criterion_main!(benches);
