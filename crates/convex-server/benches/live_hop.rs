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
//!   session serializes the record, which the sessions share (P17a).
//! - `subscribe/256`: a `subscribe_valuations` of every bond until its `subscribed`
//!   acknowledgement, which follows the 256 results it seeds (D99). Listing followed bonds
//!   again resends their results, so one session repeats it (P17a).
//! - `benchmark_tick/256`, `/1024` and `/4089`: every bond is quoted as a spread over the
//!   10-year of a set of seven Treasuries the runtime values beside them. One quote on the
//!   10-year until the session holds the frame of every bond it reprices and its own.
//!   4,089 bonds and the seven Treasuries fill a Live runtime.
//!
//! An error frame, such as `RESYNC_REQUIRED` after a lagging session (G38), fails the case.
//!
//! The bench spends at most [`ATTEMPTS_PER_SECOND`] requests and WebSocket messages and
//! waits outside the timer, so each quote reaches a runtime that has finished its previous
//! tick, as in the engine's `live_throughput/burst1/256`, and the numbers compare with
//! P13a's. P13a paced because every request then counted against a process window of
//! 6,000 a minute, in local mode too (G40); local development now counts none (D78).
//!
//! Run with: cargo bench -p convex-server --bench live_hop --profile ci
//! Report medians only; CI builds it and sets no timing threshold.

#[path = "../tests/support/live_server.rs"]
mod live_server;
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;

use convex_engine::ports::quote_event::{
    FieldUpdate, QuoteEvent, QuoteEventKind, QuoteField, QuoteKey, QuoteValue,
};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use futures_util::{SinkExt, StreamExt};
use live_server::{image, level_live_setup, wide_live_setup, LevelKeys, Running, BENCHMARK};
use reqwest::StatusCode;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// 256 bonds, `MAX_SNAPSHOT_INSTRUMENTS`; also the most events one `POST` carries, and the
/// most instruments one subscription lists (D104).
const WIDE: usize = 256;
/// Live widths past 256, up to `MAX_RUNTIME_INSTRUMENTS` (P17b).
const LIVE_WIDTHS: [usize; 2] = [1024, 4096];
/// Sessions in the fan-out case.
const SESSIONS: usize = 8;
/// Bonds quoted over one benchmark in the benchmark tick cases.
const DEPENDENTS: [usize; 3] = [256, 1024, 4089];
/// Requests and inbound WebSocket messages per second: P13a's rate, kept so the cases
/// compare across P13b's steps.
const ATTEMPTS_PER_SECOND: u32 = 80;

/// Spaces the bench's requests and messages, untimed.
struct Pacer {
    next: tokio::time::Instant,
}

impl Pacer {
    /// Wait, untimed, until `attempts` more fit under [`ATTEMPTS_PER_SECOND`].
    async fn wait(&mut self, attempts: u32) {
        tokio::time::sleep_until(self.next).await;
        self.next =
            tokio::time::Instant::now() + Duration::from_secs(1) * attempts / ATTEMPTS_PER_SECOND;
    }
}

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
async fn until(socket: &mut Socket, mut done: impl FnMut(&Frame) -> bool) {
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

/// A refused post fails the case with the server's reason.
async fn accepted(response: reqwest::Response) {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
}

/// Read frames until the session holds `count` valuation frames of the tick that admitted
/// generation `expected` or a later one.
async fn valuations(socket: &mut Socket, expected: u64, count: usize) {
    let mut held = 0;
    until(socket, |f| {
        if f.kind == "valuation" && f.generation.is_some_and(|g| g >= expected) {
            held += 1;
        }
        held == count
    })
    .await
}

/// A Live server over `width` bonds, after every bond's first image.
struct Live {
    server: Running,
    keys: Vec<QuoteKey>,
    /// The Treasuries of the benchmark tick cases, quoted by price; every other key is then
    /// quoted by a spread.
    treasuries: Vec<QuoteKey>,
    sequence: u64,
    /// The last generation the runtime admitted: one per posted quote.
    generation: u64,
    pacer: Pacer,
    _dir: tempfile::TempDir,
}

impl Live {
    async fn start(width: usize) -> Self {
        let (dir, _, config, keys) = wide_live_setup(width).await;
        Self::started(dir, config, keys, Vec::new()).await
    }

    /// `dependents` bonds quoted as a spread over the 10-year of a set of Treasuries,
    /// which are valued too.
    async fn start_quoted_over_benchmark(dependents: usize) -> Self {
        let (dir, _, config, LevelKeys { bonds, treasuries }) = level_live_setup(dependents).await;
        let keys = bonds
            .into_iter()
            .chain(treasuries.iter().cloned())
            .collect();
        Self::started(dir, config, keys, treasuries).await
    }

    async fn started(
        dir: tempfile::TempDir,
        config: convex_server::ServerConfig,
        keys: Vec<QuoteKey>,
        treasuries: Vec<QuoteKey>,
    ) -> Self {
        let width = keys.len();
        let server = Running::start(config).await;
        let generation = server.runtime().await["generation"].as_u64().unwrap();
        let mut live = Self {
            server,
            keys,
            treasuries,
            sequence: 0,
            generation,
            pacer: Pacer {
                next: tokio::time::Instant::now(),
            },
            _dir: dir,
        };
        // A POST carries at most 256 events, and the ingress queues 1,024, so each batch
        // settles before the next.
        for _ in 0..width.div_ceil(WIDE) {
            let images = live.quotes(width.min(WIDE));
            live.post(images).await;
            live.settle().await;
        }
        live.server.wait_ready().await;
        live
    }

    /// Wait, untimed, until the runtime has admitted every posted quote, so no backlog
    /// reaches the next case.
    async fn settle(&self) {
        let expected = self.generation;
        tokio::time::timeout(Duration::from_secs(30), async {
            while self.server.runtime().await["generation"].as_u64() < Some(expected) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("every posted quote is admitted");
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
                let key = self.keys[self.sequence as usize % self.keys.len()].clone();
                self.quote(key)
            })
            .collect()
    }

    /// The next quote on `key`: a price, or a spread for a bond quoted over the benchmark.
    fn quote(&self, key: QuoteKey) -> QuoteEvent {
        let mut event = image(self.sequence, "99.25");
        if !self.treasuries.is_empty() && !self.treasuries.contains(&key) {
            let QuoteEventKind::Image { fields } = &mut event.event else {
                unreachable!("an image")
            };
            fields[0].field = QuoteField::MidSpread;
            fields[0].update = FieldUpdate::Set(QuoteValue {
                value: rust_decimal::Decimal::new(125, 0),
                unit: QuoteField::MidSpread.unit(),
            });
        }
        event.key = key;
        event
    }

    /// Post one quote on the benchmark and return the time until the session holds the
    /// frame of each bond the tick reprices and the benchmark's own.
    async fn benchmark_tick(&mut self, socket: &mut Socket) -> Duration {
        self.pacer.wait(2).await;
        self.sequence += 1;
        let benchmark = self
            .treasuries
            .iter()
            .find(|key| key.instrument_id.as_str() == BENCHMARK)
            .cloned()
            .expect("a benchmark case");
        let quote = self.quote(benchmark);
        let expected = self.generation + 1;
        // Every bond quoted over it, and the benchmark itself.
        let frames = self.keys.len() - self.treasuries.len() + 1;
        let started = Instant::now();
        let server = &self.server;
        let (response, elapsed) = tokio::join!(server.post_quotes(vec![quote]), async {
            valuations(socket, expected, frames).await;
            started.elapsed()
        });
        accepted(response).await;
        self.generation = expected;
        drain(socket).await;
        elapsed
    }

    /// Post `quotes` and return the time until the `202`.
    async fn post(&mut self, quotes: Vec<QuoteEvent>) -> Duration {
        self.pacer.wait(1).await;
        let count = quotes.len() as u64;
        let started = Instant::now();
        let response = self.server.post_quotes(quotes).await;
        let elapsed = started.elapsed();
        accepted(response).await;
        self.generation += count;
        elapsed
    }

    /// A session subscribed to every bond, past its `connected` and `subscribed` frames.
    async fn session(&self) -> Socket {
        let (mut socket, _) = tokio_tungstenite::connect_async(self.server.ws_url())
            .await
            .unwrap();
        until(&mut socket, |f| f.kind == "connected").await;
        // 256 instruments a message (D104).
        for ids in self.ids().chunks(WIDE) {
            let subscribe = serde_json::json!({
                "type": "subscribe_valuations",
                "instrument_ids": ids,
            });
            socket
                .send(Message::Text(subscribe.to_string().into()))
                .await
                .unwrap();
            until(&mut socket, |f| f.kind == "subscribed").await;
        }
        socket
    }

    /// Post `count` quotes and return the time until every session holds the frame of
    /// the tick that admitted the last. A tick of one quote publishes one frame (D55); after
    /// a wider one, `drain` reads each session to its end, untimed.
    async fn hop(&mut self, sockets: &mut [Socket], count: usize, drain_after: bool) -> Duration {
        let pings = if drain_after { sockets.len() as u32 } else { 0 };
        self.pacer.wait(1 + pings).await;
        let quotes = self.quotes(count);
        let expected = self.generation + count as u64;
        let started = Instant::now();
        let server = &self.server;
        let (response, elapsed) = tokio::join!(server.post_quotes(quotes), async {
            futures_util::future::join_all(sockets.iter_mut().map(|s| valuation(s, expected)))
                .await;
            started.elapsed()
        });
        accepted(response).await;
        self.generation = expected;
        if drain_after {
            for socket in sockets.iter_mut() {
                drain(socket).await;
            }
        }
        elapsed
    }

    /// Subscribe `socket` to every bond again and return the time until the
    /// acknowledgement that follows the seed of each bond's result.
    async fn subscribe(&mut self, socket: &mut Socket) -> Duration {
        self.pacer.wait(1).await;
        let subscribe = serde_json::json!({
            "type": "subscribe_valuations",
            "instrument_ids": self.ids(),
        });
        let started = Instant::now();
        socket
            .send(Message::Text(subscribe.to_string().into()))
            .await
            .unwrap();
        until(socket, |f| f.kind == "subscribed").await;
        started.elapsed()
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
    // Paced iterations: criterion plans by wall time, so a case takes about 10 s.
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(10));

    let mut wide = rt.block_on(Live::start(WIDE));
    group.throughput(Throughput::Elements(1));
    // No session is open, so no frame waits unread.
    group.bench_function(BenchmarkId::new("post", WIDE), |b| {
        b.iter_custom(|iterations| {
            rt.block_on(async {
                let mut total = Duration::ZERO;
                for _ in 0..iterations {
                    let quotes = wide.quotes(1);
                    total += wide.post(quotes).await;
                }
                total
            })
        })
    });
    // `post` waits only for each `202`; the runtime admits those quotes before the next case.
    rt.block_on(wide.settle());
    let mut sockets = vec![rt.block_on(wide.session())];
    group.bench_function(BenchmarkId::new("burst1", WIDE), |b| {
        b.iter_custom(|iterations| {
            rt.block_on(async {
                let mut total = Duration::ZERO;
                for _ in 0..iterations {
                    total += wide.hop(&mut sockets, 1, false).await;
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
                    total += wide.hop(&mut sockets, WIDE, true).await;
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
                        total += wide.hop(&mut sockets, 1, false).await;
                    }
                    total
                })
            })
        },
    );
    // No tick runs, so the other sessions wait idle.
    group.throughput(Throughput::Elements(WIDE as u64));
    group.bench_function(BenchmarkId::new("subscribe", WIDE), |b| {
        b.iter_custom(|iterations| {
            rt.block_on(async {
                let mut total = Duration::ZERO;
                for _ in 0..iterations {
                    total += wide.subscribe(&mut sockets[0]).await;
                }
                total
            })
        })
    });
    group.throughput(Throughput::Elements(1));
    rt.block_on(wide.stop(sockets));

    let mut one = rt.block_on(Live::start(1));
    let mut sockets = vec![rt.block_on(one.session())];
    group.bench_function(BenchmarkId::from_parameter(1), |b| {
        b.iter_custom(|iterations| {
            rt.block_on(async {
                let mut total = Duration::ZERO;
                for _ in 0..iterations {
                    total += one.hop(&mut sockets, 1, false).await;
                }
                total
            })
        })
    });
    rt.block_on(one.stop(sockets));

    // One quote until its frame at 1,024 and 4,096 bonds, with one session following every
    // bond.
    for width in LIVE_WIDTHS {
        let mut live = rt.block_on(Live::start(width));
        let mut sockets = vec![rt.block_on(live.session())];
        group.bench_function(BenchmarkId::new("burst1", width), |b| {
            b.iter_custom(|iterations| {
                rt.block_on(async {
                    let mut total = Duration::ZERO;
                    for _ in 0..iterations {
                        total += live.hop(&mut sockets, 1, false).await;
                    }
                    total
                })
            })
        });
        rt.block_on(live.stop(sockets));
    }

    // One benchmark quote until the frame of every bond quoted over it. Reported with no
    // target: such a tick values every dependent.
    for dependents in DEPENDENTS {
        let mut live = rt.block_on(Live::start_quoted_over_benchmark(dependents));
        let mut socket = rt.block_on(live.session());
        group.throughput(Throughput::Elements(dependents as u64));
        group.bench_function(BenchmarkId::new("benchmark_tick", dependents), |b| {
            b.iter_custom(|iterations| {
                rt.block_on(async {
                    let mut total = Duration::ZERO;
                    for _ in 0..iterations {
                        total += live.benchmark_tick(&mut socket).await;
                    }
                    total
                })
            })
        });
        rt.block_on(live.stop(vec![socket]));
    }
    group.finish();
}

criterion_group!(benches, bench_live_hop);
criterion_main!(benches);
