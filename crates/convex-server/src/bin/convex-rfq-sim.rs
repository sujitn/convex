//! A source of clients' requests for quote, for a desk demo and tests. It sends requests
//! of realistic sizes, sides, client tiers and deadlines to `POST /api/v1/rfqs`, follows
//! the `rfq` topic on its books, and once the desk quotes reports what the client did.
//!
//! ```text
//! convex-rfq-sim --server http://127.0.0.1:8080 --source-id venue-sim --books BOOK-IG \
//!     --bonds BOND-001,BOND-002 --tiers 1,2,3 [--clients 24] [--rate 0.5] [--count 20] \
//!     [--seed 7]
//! ```
//!
//! `--rate` is requests a second on average; `--count` stops once that many were taken and
//! each is final. A request the server is too busy for, or the desk is full or cannot
//! record, is not taken and another is drawn; any other refusal ends the run, since every
//! request after it would be refused too. Each of `--clients` clients has a fixed tier.
//! Each request's client, bond, book, side, size, deadline and what its client will do are
//! drawn when it is sent, so one seed gives the same requests and answers however fast the
//! desk is. Each stdout
//! line is one JSON event: `following` once it follows the books, `sent` and `final` for
//! each request, and `summary` at the end.
//!
//! Under required access the identity, whose token is read from `CONVEX_SIM_TOKEN`, needs
//! `rfq_source` on the source, books and bonds, and `reader` on the books and every
//! instrument they hold or their limits name, to follow them.
#![forbid(unsafe_code)]

#[path = "simulation/draws.rs"]
mod draws;
#[path = "simulation/server_client.rs"]
mod server_client;

use std::collections::{BTreeMap, HashMap};
use std::process::ExitCode;
use std::time::Duration;

use chrono::Utc;
use convex_core::contracts::{
    BusinessDate, ExactDecimal, InstrumentId, ReferenceDataId, SourceId, UtcTimestamp,
};
use convex_core::Date;
use convex_engine::ports::reference_master::BookId;
use convex_engine::ports::rfq::{ClientSide, ClientTier, RfqRequest, RfqState, SourceOutcome};
use draws::Draws;
use reqwest::{Method, StatusCode};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use server_client::{Arguments, RfqRead, RfqRow, RfqTopic, ServerClient};
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Face amounts asked for, by weight: odd lots, round lots and blocks.
const FACES: [(u64, u32); 11] = [
    (100_000, 4),
    (250_000, 6),
    (500_000, 8),
    (750_000, 4),
    (1_000_000, 20),
    (2_000_000, 14),
    (3_000_000, 10),
    (5_000_000, 14),
    (10_000_000, 10),
    (15_000_000, 4),
    (25_000_000, 6),
];
/// Seconds a client gives the desk, by weight.
const DEADLINES: [(u64, u32); 5] = [(15, 10), (30, 30), (60, 35), (120, 20), (300, 5)];
/// What a client does once quoted, by weight.
const OUTCOMES: [(SourceOutcome, u32); 3] = [
    (SourceOutcome::Done, 30),
    (SourceOutcome::TradedAway, 50),
    (SourceOutcome::Covered, 20),
];
/// In how many of a hundred requests the client withdraws, quoted or not.
const WITHDRAWN_PER_HUNDRED: usize = 5;
/// The longest a client waits before it withdraws.
const LONGEST_WITHDRAWAL: Duration = Duration::from_secs(8);
/// How long past its deadline a request whose final the topic has not shown is read.
const FINAL_GRACE: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("convex-rfq-sim: {error}");
            ExitCode::FAILURE
        }
    }
}

/// What a client will do with its request.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ClientIntent {
    /// Withdraw it this long after sending, quoted or not.
    Withdraws { after: Duration },
    /// Once quoted, answer with `outcome` this long after the quote.
    Answers {
        outcome: SourceOutcome,
        after: Duration,
    },
}

/// One request drawn: the request and what its client will do.
struct Drawn {
    request: RfqRequest,
    deadline: Duration,
    intent: ClientIntent,
}

/// A client and its tier.
struct TieredClient {
    client_id: String,
    client_tier: ClientTier,
}

/// The venue whose clients send the requests.
struct Venue {
    source_id: SourceId,
    books: Vec<BookId>,
    bonds: Vec<InstrumentId>,
    clients: Vec<TieredClient>,
    /// Makes this run's request ids its own.
    run_id: String,
}

impl Venue {
    fn from(arguments: &Arguments) -> Result<Self, String> {
        let invalid = |name: &'static str| move |_| format!("--{name} names an invalid identifier");
        let tiers = arguments
            .list("tiers")?
            .into_iter()
            .map(ClientTier::new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "--tiers names an invalid tier".to_owned())?;
        let clients = (1..=arguments.optional("clients", 24usize)?)
            .map(|index| TieredClient {
                client_id: format!("CLIENT-{index:02}"),
                client_tier: tiers[(index - 1) % tiers.len()].clone(),
            })
            .collect::<Vec<_>>();
        if clients.is_empty() {
            return Err("--clients is at least 1".into());
        }
        Ok(Self {
            source_id: SourceId::new(arguments.required("source-id")?)
                .map_err(invalid("source-id"))?,
            books: arguments
                .list("books")?
                .into_iter()
                .map(ReferenceDataId::new)
                .collect::<Result<_, _>>()
                .map_err(invalid("books"))?,
            bonds: arguments
                .list("bonds")?
                .into_iter()
                .map(InstrumentId::new)
                .collect::<Result<_, _>>()
                .map_err(invalid("bonds"))?,
            clients,
            run_id: format!("R{}", Utc::now().timestamp_millis()),
        })
    }

    /// The `sequence`th request.
    fn draw(&self, draws: &mut Draws, sequence: usize) -> Result<Drawn, String> {
        let client = &self.clients[draws.below(self.clients.len())];
        let deadline = Duration::from_secs(draws.weighted(&DEADLINES));
        let side = if draws.below(2) == 0 {
            ClientSide::Buy
        } else {
            ClientSide::Sell
        };
        let face = draws.weighted(&FACES);
        let instrument_id = self.bonds[draws.below(self.bonds.len())].clone();
        let book_id = self.books[draws.below(self.books.len())].clone();
        let intent = if draws.below(100) < WITHDRAWN_PER_HUNDRED {
            let longest = (deadline / 2).min(LONGEST_WITHDRAWAL);
            ClientIntent::Withdraws {
                after: draws.between(Duration::from_secs(1), longest),
            }
        } else {
            let after = draws.between(Duration::from_millis(500), Duration::from_secs(4));
            ClientIntent::Answers {
                outcome: draws.weighted(&OUTCOMES),
                after,
            }
        };
        let request = RfqRequest {
            source_id: self.source_id.clone(),
            rfq_id: format!("{}-{sequence:05}", self.run_id),
            client_id: client.client_id.clone(),
            client_tier: client.client_tier.clone(),
            instrument_id: Some(instrument_id),
            identifier: None,
            side,
            face: ExactDecimal::new(Decimal::from(face)),
            settlement_date: settlement_date()?,
            book_id,
            deadline: UtcTimestamp::from_datetime(Utc::now() + deadline)
                .map_err(|_| "the clock is outside the supported range".to_owned())?,
        };
        Ok(Drawn {
            request,
            deadline,
            intent,
        })
    }
}

/// One business day after today, UTC.
fn settlement_date() -> Result<BusinessDate, String> {
    Date::from(Utc::now().date_naive())
        .add_business_days(1)
        .ok()
        .and_then(|date| BusinessDate::try_from(date).ok())
        .ok_or_else(|| "the clock is outside the supported range".to_owned())
}

/// A request taken, not yet final.
struct Sent {
    deadline: Instant,
    intent: ClientIntent,
}

async fn run() -> Result<(), String> {
    let arguments = Arguments::parse(&[
        "server",
        "source-id",
        "books",
        "bonds",
        "tiers",
        "clients",
        "rate",
        "count",
        "seed",
    ])?;
    let server = ServerClient::new(arguments.required("server")?)?;
    let venue = Venue::from(&arguments)?;
    let rate: f64 = arguments.optional("rate", 0.5)?;
    if !(rate.is_finite() && rate > 0.0) {
        return Err("--rate is a positive number of requests a second".into());
    }
    let count: usize = arguments.optional("count", usize::MAX)?;
    let mut draws = Draws(arguments.optional("seed", Utc::now().timestamp_millis() as u64)?);

    let books: Vec<String> = venue.books.iter().map(ToString::to_string).collect();
    let topic = server.rfq_topic(&books).await?;
    println!(
        "{}",
        json!({"event": "following", "books": books, "open": topic.open_count()})
    );
    let mut rows = follow(topic, server.clone());
    let mut simulation = Simulation {
        sent: HashMap::new(),
        answers: BTreeMap::new(),
        finals: BTreeMap::new(),
    };
    let mut taken = 0usize;
    let mut sequence = 0usize;
    let mut next_arrival = Instant::now();
    let mut sweep = tokio::time::interval(Duration::from_secs(1));
    while taken < count || !simulation.sent.is_empty() {
        let next_answer = simulation.answers.keys().next().map(|(at, _)| *at);
        tokio::select! {
            () = tokio::time::sleep_until(next_arrival), if taken < count => {
                sequence += 1;
                if simulation.send(&server, venue.draw(&mut draws, sequence)?).await? {
                    taken += 1;
                }
                let gap = -(1.0 - draws.unit()).ln() / rate;
                next_arrival = Instant::now() + Duration::from_secs_f64(gap.min(3_600.0));
            }
            () = tokio::time::sleep_until(next_answer.unwrap_or_else(Instant::now)), if next_answer.is_some() => {
                simulation.answer_due(&server).await?;
            }
            row = rows.recv() => simulation.saw(&row.unwrap_or_else(|| Err("the topic ended".into()))?),
            _ = sweep.tick() => simulation.read_missed(&server).await?,
        }
    }
    println!(
        "{}",
        json!({"event": "summary", "taken": taken, "final": simulation.finals})
    );
    Ok(())
}

/// The topic's rows, read on a task of their own, so a timer that fires while the topic
/// catches up loses nothing it read. The first error is the last row.
fn follow(mut topic: RfqTopic, server: ServerClient) -> mpsc::Receiver<Result<RfqRow, String>> {
    let (sender, rows) = mpsc::channel(64);
    tokio::spawn(async move {
        loop {
            let row = topic.next(&server).await;
            let is_last = row.is_err();
            if sender.send(row).await.is_err() || is_last {
                break;
            }
        }
    });
    rows
}

struct Simulation {
    /// Each request taken and not yet final, by key.
    sent: HashMap<String, Sent>,
    /// Each answer due, by when and the request's key.
    answers: BTreeMap<(Instant, String), SourceOutcome>,
    /// How many requests ended in each final state.
    finals: BTreeMap<RfqState, usize>,
}

impl Simulation {
    /// Send `drawn`; whether it was taken. A refusal that is not passing is an error.
    async fn send(&mut self, server: &ServerClient, drawn: Drawn) -> Result<bool, String> {
        let response = server
            .request(Method::POST, "/api/v1/rfqs")
            .json(&json!({ "rfqs": [drawn.request] }))
            .send()
            .await
            .map_err(|error| format!("ingest failed: {error}"))?;
        let status = response.status();
        let reply: Value = match response.json().await {
            Ok(reply) => reply,
            Err(error) if status.is_success() => {
                return Err(format!(
                    "whether a request was taken is not known: its reply ({status}) is \
                     unreadable: {error}"
                ))
            }
            Err(_) => Value::Null,
        };
        let answer = &reply["rfqs"][0];
        let Some(rfq_key) = answer["rfq_key"]
            .as_str()
            .filter(|_| answer["result"] == "accepted")
        else {
            // A refusal a later request may not meet: the desk full, or its records
            // unreadable or unwritable for now. One without a code is the server's, busy or
            // starting.
            let is_passing = match answer["code"].as_str() {
                Some(code) => matches!(code, "RFQ_CAPACITY" | "RFQ_UNREADABLE" | "RFQ_UNRECORDED"),
                None => matches!(
                    status,
                    StatusCode::SERVICE_UNAVAILABLE | StatusCode::TOO_MANY_REQUESTS
                ),
            };
            if !is_passing {
                return Err(format!("a request was refused ({status}): {reply}"));
            }
            eprintln!(
                "{}",
                json!({"event": "not_taken", "status": status.as_u16(), "reply": reply})
            );
            return Ok(false);
        };
        println!(
            "{}",
            json!({"event": "sent", "rfq_key": rfq_key, "request": drawn.request})
        );
        let now = Instant::now();
        if let ClientIntent::Withdraws { after } = drawn.intent {
            self.answers
                .insert((now + after, rfq_key.to_owned()), SourceOutcome::Cancelled);
        }
        self.sent.insert(
            rfq_key.to_owned(),
            Sent {
                deadline: now + drawn.deadline,
                intent: drawn.intent,
            },
        );
        Ok(true)
    }

    /// A request as the topic shows it: once quoted its client's answer is due, and once
    /// final it is done with.
    fn saw(&mut self, row: &RfqRow) {
        let Some(sent) = self.sent.get(&row.rfq_key) else {
            return;
        };
        if let (RfqState::Quoted, ClientIntent::Answers { outcome, after }) =
            (row.state, sent.intent)
        {
            let at = (Instant::now() + after).min(sent.deadline);
            self.answers.insert((at, row.rfq_key.clone()), outcome);
        }
        if row.state.is_final() {
            self.sent.remove(&row.rfq_key);
            *self.finals.entry(row.state).or_default() += 1;
            println!(
                "{}",
                json!({"event": "final", "rfq_key": row.rfq_key, "rfq_id": row.request.rfq_id,
                       "instrument_id": row.instrument_id, "state": row.state})
            );
        }
    }

    /// Send the earliest answer due. One refused because the request is final already is
    /// not an error: its deadline passed, or the trader passed.
    async fn answer_due(&mut self, server: &ServerClient) -> Result<(), String> {
        let Some(((_, rfq_key), outcome)) = self.answers.pop_first() else {
            return Ok(());
        };
        if !self.sent.contains_key(&rfq_key) {
            return Ok(());
        }
        let response = server
            .request(Method::POST, &format!("/api/v1/rfqs/{rfq_key}/outcome"))
            .json(&json!({ "outcome": outcome }))
            .send()
            .await
            .map_err(|error| format!("an outcome failed: {error}"))?;
        let status = response.status();
        if !(status.is_success() || status == StatusCode::CONFLICT) {
            let reply = response.text().await.unwrap_or_default();
            eprintln!(
                "{}",
                json!({"event": "outcome_refused", "rfq_key": rfq_key, "status": status.as_u16(), "reply": reply})
            );
        }
        Ok(())
    }

    /// Read each request whose final the topic has not shown well past its deadline.
    async fn read_missed(&mut self, server: &ServerClient) -> Result<(), String> {
        let now = Instant::now();
        let missed: Vec<String> = self
            .sent
            .iter()
            .filter(|(_, sent)| sent.deadline + FINAL_GRACE < now)
            .map(|(key, _)| key.clone())
            .collect();
        for rfq_key in missed {
            match server.rfq(&rfq_key).await? {
                RfqRead::Row(row) => self.saw(&row),
                RfqRead::Unknown => {}
                RfqRead::Withheld => {
                    return Err(format!(
                        "request {rfq_key} may not be read: the identity needs reader on its \
                         book, the instruments the book holds or its limits name, and its bond"
                    ))
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn venue() -> Venue {
        Venue {
            source_id: SourceId::new("venue-sim").unwrap(),
            books: ["BOOK-A", "BOOK-B"]
                .map(|book| ReferenceDataId::new(book).unwrap())
                .to_vec(),
            bonds: ["BOND-001", "BOND-002"]
                .map(|bond| InstrumentId::new(bond).unwrap())
                .to_vec(),
            clients: ["1", "2"]
                .into_iter()
                .enumerate()
                .map(|(index, tier)| TieredClient {
                    client_id: format!("CLIENT-{index:02}"),
                    client_tier: ClientTier::new(tier.to_owned()).unwrap(),
                })
                .collect(),
            run_id: "R1".into(),
        }
    }

    /// One seed draws the same requests, each within the server's bounds.
    #[test]
    fn a_seed_draws_the_same_requests_within_the_bounds() {
        let venue = venue();
        let (mut first, mut second) = (Draws(7), Draws(7));
        for sequence in 1..=500 {
            let a = venue.draw(&mut first, sequence).unwrap();
            let mut b = venue.draw(&mut second, sequence).unwrap();
            b.request.deadline = a.request.deadline;
            assert_eq!(a.request, b.request);
            assert_eq!((a.deadline, a.intent), (b.deadline, b.intent));
            let received = UtcTimestamp::from_datetime(Utc::now()).unwrap();
            assert_eq!(a.request.validate(received), Ok(()));
            let face = a.request.face.decimal();
            assert!(face >= Decimal::from(100_000) && face <= Decimal::from(25_000_000));
            match a.intent {
                ClientIntent::Withdraws { after } => {
                    assert!(after >= Duration::from_secs(1) && after <= LONGEST_WITHDRAWAL)
                }
                ClientIntent::Answers { after, .. } => {
                    assert!(after >= Duration::from_millis(500) && after <= Duration::from_secs(4))
                }
            }
        }
    }

    /// Over many draws each choice comes up about as often as its weight says.
    #[test]
    fn draws_follow_their_weights() {
        let mut draws = Draws(11);
        let mut counts = [0u32; 3];
        for _ in 0..10_000 {
            let outcome = draws.weighted(&OUTCOMES);
            let index = OUTCOMES
                .iter()
                .position(|(choice, _)| *choice == outcome)
                .unwrap();
            counts[index] += 1;
        }
        for ((_, weight), count) in OUTCOMES.iter().zip(counts) {
            let expected = f64::from(*weight) * 100.0;
            assert!(
                (f64::from(count) - expected).abs() < expected * 0.1,
                "{count} {expected}"
            );
        }
    }
}
