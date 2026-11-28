//! A synthetic market for a desk demo. Once a tick it moves Treasury yields and credit
//! spreads together, posts each priced bond's new mid as a quote, shifts each desk mark by
//! the same moves, and publishes each SOFR fixing once it falls due.
//!
//! ```text
//! convex-feed-sim --server http://127.0.0.1:8080 --feed feed.json [--interval-ms 1000] \
//!     [--seed 7] [--ticks 0]
//! ```
//!
//! The market is two mean-reverting random walks in basis points: rates, and credit
//! correlated with it. A priced bond's mid is its opening price moved by its modified
//! duration times the rates move plus its credit beta times the credit move, posted as a
//! `live` image on the feed's source. A desk mark is read back and moved by the tick's
//! change (a spread by its credit beta times the credit change; a yield by the rates change
//! and its credit beta times the credit change), so a trader's own mark is kept and moves
//! with the market. A fixing for each SIFMA business day is published as its rate index
//! says, the business days later and the hour (UTC) the feed file gives. `--ticks` stops
//! after that many; 0 runs until stopped. Each stdout line is one JSON event: `started`,
//! `moved` each minute of ticks, `fixing`, and `stopped`.
//!
//! Under required access the identity, whose token is read from `CONVEX_SIM_TOKEN`, needs
//! to post quotes on the feed's source, mark the desk's levels, and publish fixings.
#![forbid(unsafe_code)]

#[path = "simulation/draws.rs"]
#[allow(dead_code)] // The feed draws normals only.
mod draws;
#[path = "simulation/server_client.rs"]
#[allow(dead_code)] // The feed takes no list argument and opens no stream.
mod server_client;

use std::collections::HashMap;
use std::process::ExitCode;
use std::time::Duration;

use chrono::{NaiveDate, TimeZone, Utc};
use convex_core::calendars::{Calendar, SIFMACalendar};
use convex_core::contracts::{
    BusinessDate, DecimalRate, ExactDecimal, IndexId, SourceId, UtcTimestamp, Version,
};
use convex_core::ids::InstrumentId;
use convex_core::Date;
use convex_engine::ports::fixing::{FixingKey, PublishedFixing};
use convex_engine::ports::quote_event::{
    FieldUpdate, QuoteChange, QuoteEvent, QuoteEventKind, QuoteField, QuoteKey, QuoteQuality,
    QuoteValue,
};
use draws::Draws;
use reqwest::{Method, StatusCode};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use server_client::{send_until_answered, Arguments, ServerClient};
use tokio::time::MissedTickBehavior;

/// A tick's rates move, one standard deviation, in basis points.
const RATES_VOLATILITY_BP: f64 = 0.35;
/// A tick's credit move, one standard deviation, in basis points.
const CREDIT_VOLATILITY_BP: f64 = 0.25;
/// How credit moves with rates: spreads widen a little as yields fall.
const CORRELATION: f64 = -0.3;
/// The share of its distance from the open each walk gives back a tick.
const REVERSION: f64 = 0.01;
/// Ticks between `moved` events at the default interval: one a minute.
const REPORT_EVERY: u64 = 60;

/// The feed's instruments, as the demo's seed writes them.
#[derive(Deserialize)]
struct FeedPlan {
    /// The source its quotes are posted on.
    source: String,
    /// The desk's source, whose marks it moves.
    level_source: String,
    prices: Vec<PricedBond>,
    levels: Vec<MarkedBond>,
    fixings: FixingPlan,
}

/// A bond quoted by price.
#[derive(Deserialize)]
struct PricedBond {
    instrument_id: String,
    /// Its opening clean price mid.
    price: Decimal,
    modified_duration: f64,
    /// Its price's move in credit spread per basis point of the market's.
    credit_beta: f64,
}

/// A bond the desk marks.
#[derive(Deserialize)]
struct MarkedBond {
    instrument_id: String,
    /// `mid_spread`, in basis points, or `mid_yield`, a decimal yield.
    field: QuoteField,
    credit_beta: f64,
}

/// The index whose fixings are published, when, and the last already published.
#[derive(Deserialize)]
struct FixingPlan {
    index_id: String,
    source: String,
    rate: Decimal,
    last_observation: Date,
    publication_lag_business_days: i32,
    publication_hour_utc: u32,
}

/// Rates and credit, in basis points: where the market stands from the open, or a tick's move.
#[derive(Debug, Default, Clone, Copy)]
struct BasisPoints {
    rates: f64,
    credit: f64,
}

/// Where the market stands from the open.
#[derive(Default)]
struct Market {
    level: BasisPoints,
}

impl Market {
    /// One tick's move.
    fn step(&mut self, draws: &mut Draws) -> BasisPoints {
        let (rates, other) = (draws.normal(), draws.normal());
        let credit = CORRELATION * rates + (1.0 - CORRELATION * CORRELATION).sqrt() * other;
        let moved = BasisPoints {
            rates: RATES_VOLATILITY_BP * rates - REVERSION * self.level.rates,
            credit: CREDIT_VOLATILITY_BP * credit - REVERSION * self.level.credit,
        };
        self.level.rates += moved.rates;
        self.level.credit += moved.credit;
        moved
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("convex-feed-sim: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let arguments = Arguments::parse(&["server", "feed", "interval-ms", "seed", "ticks"])?;
    let server = ServerClient::new(arguments.required("server")?)?;
    let plan: FeedPlan = serde_json::from_slice(
        &std::fs::read(arguments.required("feed")?)
            .map_err(|error| format!("the feed plan could not be read: {error}"))?,
    )
    .map_err(|error| format!("the feed plan is not valid: {error}"))?;
    let interval = Duration::from_millis(arguments.optional("interval-ms", 1000)?);
    let mut draws = Draws(arguments.optional("seed", 7)?);
    let ticks: u64 = arguments.optional("ticks", 0)?;
    let session = format!("feed-{}", Utc::now().timestamp_millis());
    let mut market = Market::default();
    let mut last_fixing = plan.fixings.last_observation;
    println!(
        "{}",
        json!({"event": "started", "session": session, "prices": plan.prices.len(),
               "levels": plan.levels.len()})
    );
    let mut timer = tokio::time::interval(interval);
    // Each tick posts absolute levels, so a tick missed after a stall needs no catch-up.
    timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut tick = 0;
    while ticks == 0 || tick < ticks {
        timer.tick().await;
        tick += 1;
        let moved = market.step(&mut draws);
        // The marks are read first, so the prices and the moved marks are posted back to
        // back and the runtime values a tick's moves together.
        let marks = read_marks(&server).await?;
        post_prices(&server, &plan, &session, tick, market.level).await?;
        move_marks(&server, &plan, &marks, moved).await?;
        last_fixing = publish_due_fixings(&server, &plan.fixings, last_fixing).await?;
        if tick % REPORT_EVERY == 0 {
            println!(
                "{}",
                json!({"event": "moved", "tick": tick, "rates_bp": market.level.rates,
                       "credit_bp": market.level.credit})
            );
        }
    }
    println!("{}", json!({"event": "stopped", "ticks": tick}));
    Ok(())
}

/// Post each priced bond's mid at the market's `level`: its opening price moved by its
/// modified duration over the rates and credit moves since the open.
async fn post_prices(
    server: &ServerClient,
    plan: &FeedPlan,
    session: &str,
    sequence: u64,
    level: BasisPoints,
) -> Result<(), String> {
    let now = Utc::now().timestamp_millis();
    let events = plan
        .prices
        .iter()
        .map(|bond| {
            let moved_bp = level.rates + bond.credit_beta * level.credit;
            let scale = decimal(1.0 - bond.modified_duration * moved_bp / 10_000.0)?;
            Ok(QuoteEvent {
                schema_version: 1,
                key: QuoteKey {
                    instrument_id: InstrumentId::new(&bond.instrument_id),
                    source: plan.source.clone(),
                },
                session: session.to_owned(),
                sequence,
                source_time_ms: now,
                receive_time_ms: now,
                quality: QuoteQuality::Live,
                entitled: true,
                entitlement_tags: Vec::new(),
                evidence_reference: None,
                event: QuoteEventKind::Image {
                    fields: vec![QuoteChange {
                        field: QuoteField::MidPrice,
                        update: FieldUpdate::Set(QuoteValue {
                            value: (bond.price * scale).round_dp(6),
                            unit: QuoteField::MidPrice.unit(),
                        }),
                    }],
                },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let response = send_until_answered(
        server
            .request(Method::POST, "/api/v1/market/quotes")
            .json(&json!({"events": events})),
    )
    .await?;
    answered(response, "quotes").await
}

/// The desk's marks now, by bond; a bond not yet marked has none.
async fn read_marks(server: &ServerClient) -> Result<HashMap<String, Decimal>, String> {
    #[derive(Deserialize)]
    struct Mark {
        level: Decimal,
    }
    #[derive(Deserialize)]
    struct Marked {
        instrument_id: String,
        mark: Option<Mark>,
    }
    #[derive(Deserialize)]
    struct Marks {
        levels: Vec<Marked>,
    }
    let response =
        send_until_answered(server.request(Method::GET, "/api/v1/market/levels")).await?;
    let marks: Marks = response
        .error_for_status()
        .map_err(|error| format!("the desk's marks could not be read: {error}"))?
        .json()
        .await
        .map_err(|error| format!("the desk's marks are unreadable: {error}"))?;
    Ok(marks
        .levels
        .into_iter()
        .filter_map(|bond| Some((bond.instrument_id, bond.mark?.level)))
        .collect())
}

/// Move each desk mark in `marks` by the tick's change; a bond not yet marked stays unmarked.
async fn move_marks(
    server: &ServerClient,
    plan: &FeedPlan,
    marks: &HashMap<String, Decimal>,
    moved: BasisPoints,
) -> Result<(), String> {
    let mut levels = Vec::new();
    for bond in &plan.levels {
        let Some(level) = marks.get(&bond.instrument_id) else {
            continue;
        };
        let spread_move = bond.credit_beta * moved.credit;
        let level = match bond.field {
            QuoteField::MidSpread => (level + decimal(spread_move)?).round_dp(2),
            QuoteField::MidYield => {
                (level + decimal((moved.rates + spread_move) / 10_000.0)?).round_dp(6)
            }
            other => {
                return Err(format!(
                    "a desk mark is in {other:?}, not a spread or yield"
                ))
            }
        };
        levels.push(
            json!({"instrument_id": bond.instrument_id, "field": bond.field,
                           "level": level.to_string()}),
        );
    }
    if levels.is_empty() {
        return Ok(());
    }
    let response = send_until_answered(
        server
            .request(Method::POST, "/api/v1/market/levels")
            .json(&json!({"source": plan.level_source, "levels": levels})),
    )
    .await?;
    answered(response, "marks").await
}

/// Publish each business day's fixing after `last` that is due; the last one published.
async fn publish_due_fixings(
    server: &ServerClient,
    fixings: &FixingPlan,
    mut last: Date,
) -> Result<Date, String> {
    let calendar = SIFMACalendar::global();
    loop {
        let observation = calendar.add_business_days(last, 1);
        let published = publication_time(
            calendar.add_business_days(observation, fixings.publication_lag_business_days),
            fixings.publication_hour_utc,
        )?;
        if Utc::now() < published {
            return Ok(last);
        }
        let fixing = PublishedFixing {
            schema_version: 1,
            key: FixingKey {
                index_id: IndexId::new(&fixings.index_id).map_err(|error| error.to_string())?,
                observation_date: BusinessDate::try_from(observation)
                    .map_err(|error| error.to_string())?,
                source: SourceId::new(&fixings.source).map_err(|error| error.to_string())?,
            },
            version: Version::new(1).map_err(|error| error.to_string())?,
            rate: DecimalRate::new(ExactDecimal::new(fixings.rate)),
            published_at: UtcTimestamp::from_datetime(published)
                .map_err(|error| error.to_string())?,
            recorded_at: UtcTimestamp::from_datetime(Utc::now())
                .map_err(|error| error.to_string())?,
            is_preliminary: false,
            entitled: true,
            evidence_reference: "synthetic".into(),
        };
        let response = send_until_answered(
            server
                .request(Method::POST, "/api/v1/analytics/fixings")
                .json(&[fixing]),
        )
        .await?;
        answered(response, "a fixing").await?;
        println!(
            "{}",
            json!({"event": "fixing", "observation_date": observation.to_string()})
        );
        last = observation;
    }
}

/// `day` at `hour`, UTC.
fn publication_time(day: Date, hour: u32) -> Result<chrono::DateTime<Utc>, String> {
    NaiveDate::from(day)
        .and_hms_opt(hour, 0, 0)
        .map(|at| Utc.from_utc_datetime(&at))
        .ok_or_else(|| format!("{hour}:00 is not a time"))
}

fn decimal(value: f64) -> Result<Decimal, String> {
    Decimal::from_f64(value).ok_or_else(|| "a move is not finite".to_owned())
}

/// A post's answer: taken, or refused, which ends the run; a server too busy for it
/// skips this tick.
async fn answered(response: reqwest::Response, what: &str) -> Result<(), String> {
    let status = response.status();
    if status.is_success() || status == StatusCode::TOO_MANY_REQUESTS {
        return Ok(());
    }
    let body = response.text().await.unwrap_or_default();
    Err(format!("{what} refused ({status}): {}", truncated(&body)))
}

fn truncated(text: &str) -> &str {
    text.get(..text.len().min(400)).unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_market_reverts_toward_its_open() {
        let mut market = Market::default();
        let mut draws = Draws(7);
        for _ in 0..20_000 {
            market.step(&mut draws);
        }
        // Mean reversion keeps each walk near its open: a few volatilities over the
        // reversion, never a random walk's drift.
        let bound = 6.0 * RATES_VOLATILITY_BP / (2.0 * REVERSION).sqrt();
        assert!(market.level.rates.abs() < bound, "{:?}", market.level);
        assert!(market.level.credit.abs() < bound, "{:?}", market.level);
    }

    #[test]
    fn credit_moves_against_rates() {
        let mut market = Market::default();
        let mut draws = Draws(11);
        let moves: Vec<BasisPoints> = (0..20_000).map(|_| market.step(&mut draws)).collect();
        let covariance = moves
            .iter()
            .map(|moved| moved.rates * moved.credit)
            .sum::<f64>()
            / moves.len() as f64;
        let expected = CORRELATION * RATES_VOLATILITY_BP * CREDIT_VOLATILITY_BP;
        assert!(
            (covariance - expected).abs() < 0.01,
            "{covariance} against {expected}"
        );
    }

    #[test]
    fn a_fixing_is_published_the_business_day_after_its_observation() {
        let calendar = SIFMACalendar::global();
        let day = |y, m, d| Date::from_ymd(y, m, d).unwrap();
        // Friday's is published on Monday; Thanksgiving's Wednesday on Friday.
        assert_eq!(
            calendar.add_business_days(day(2026, 11, 27), 1),
            day(2026, 11, 30)
        );
        assert_eq!(
            calendar.add_business_days(day(2026, 11, 25), 1),
            day(2026, 11, 27)
        );
        let at = publication_time(day(2026, 11, 30), 13).unwrap();
        assert_eq!(at.to_rfc3339(), "2026-11-30T13:00:00+00:00");
    }
}
