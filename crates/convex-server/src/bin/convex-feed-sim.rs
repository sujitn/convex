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
//! with the market. A fixing for each weekday is published at 13:00 UTC on the next
//! weekday. `--ticks` stops after that many; 0 runs until stopped. Each stdout line is one
//! JSON event: `started`, `moved` each minute of ticks, `fixing`, and `stopped`.
//!
//! Under required access the identity, whose token is read from `CONVEX_SIM_TOKEN`, needs
//! to post quotes on the feed's source, mark the desk's levels, and publish fixings.
#![forbid(unsafe_code)]

#[path = "simulation/draws.rs"]
mod draws;
#[path = "simulation/server_client.rs"]
mod server_client;

use std::collections::HashMap;
use std::process::ExitCode;
use std::time::Duration;

use chrono::{Datelike, NaiveDate, TimeZone, Utc, Weekday};
use draws::Draws;
use reqwest::{Method, StatusCode};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use server_client::{send_until_answered, Arguments, ServerClient};

/// A tick's rates move, one standard deviation, in basis points.
const RATES_VOLATILITY_BP: f64 = 0.35;
/// A tick's credit move, one standard deviation, in basis points.
const CREDIT_VOLATILITY_BP: f64 = 0.25;
/// How credit moves with rates: spreads widen a little as yields fall.
const CORRELATION: f64 = -0.3;
/// The share of its distance from the open each walk gives back a tick.
const REVERSION: f64 = 0.01;
/// The UTC hour a weekday's fixing is published, on the next weekday.
const FIXING_HOUR_UTC: u32 = 13;
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
    field: String,
    credit_beta: f64,
}

/// The index whose fixings are published, and the last already published.
#[derive(Deserialize)]
struct FixingPlan {
    index_id: String,
    source: String,
    rate: Decimal,
    last_observation: NaiveDate,
}

/// Where the market stands, in basis points from the open.
#[derive(Default)]
struct Market {
    rates_bp: f64,
    credit_bp: f64,
}

impl Market {
    /// One tick: the rates and credit moves.
    fn step(&mut self, draws: &mut Draws) -> (f64, f64) {
        let (rates, other) = (draws.normal(), draws.normal());
        let credit = CORRELATION * rates + (1.0 - CORRELATION * CORRELATION).sqrt() * other;
        let rates_move = RATES_VOLATILITY_BP * rates - REVERSION * self.rates_bp;
        let credit_move = CREDIT_VOLATILITY_BP * credit - REVERSION * self.credit_bp;
        self.rates_bp += rates_move;
        self.credit_bp += credit_move;
        (rates_move, credit_move)
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
    let mut tick = 0;
    while ticks == 0 || tick < ticks {
        timer.tick().await;
        tick += 1;
        let (rates_move, credit_move) = market.step(&mut draws);
        post_prices(&server, &plan, &session, tick, &market).await?;
        move_marks(&server, &plan, rates_move, credit_move).await?;
        last_fixing = publish_due_fixings(&server, &plan.fixings, last_fixing).await?;
        if tick % REPORT_EVERY == 0 {
            println!(
                "{}",
                json!({"event": "moved", "tick": tick, "rates_bp": market.rates_bp,
                       "credit_bp": market.credit_bp})
            );
        }
    }
    println!("{}", json!({"event": "stopped", "ticks": tick}));
    Ok(())
}

/// Post each priced bond's mid at the market's level: its opening price moved by its
/// modified duration over the rates and credit moves since the open.
async fn post_prices(
    server: &ServerClient,
    plan: &FeedPlan,
    session: &str,
    sequence: u64,
    market: &Market,
) -> Result<(), String> {
    let now = Utc::now().timestamp_millis();
    let events: Vec<Value> = plan
        .prices
        .iter()
        .map(|bond| {
            let moved_bp = market.rates_bp + bond.credit_beta * market.credit_bp;
            let scale = Decimal::from_f64(1.0 - bond.modified_duration * moved_bp / 10_000.0)
                .ok_or("a price move is not finite")?;
            let price = (bond.price * scale).round_dp(6);
            Ok(json!({
                "schema_version": 1,
                "key": {"instrument_id": bond.instrument_id, "source": plan.source},
                "session": session, "sequence": sequence, "source_time_ms": now,
                "receive_time_ms": now, "quality": "live", "entitled": true,
                "entitlement_tags": [], "evidence_reference": null,
                "event": {"kind": "image", "fields": [{"field": "mid_price", "update":
                    {"action": "set", "value": {"value": price.to_string(), "unit": "price_per100"}}}]},
            }))
        })
        .collect::<Result<_, String>>()?;
    let response = send_until_answered(
        server
            .request(Method::POST, "/api/v1/market/quotes")
            .json(&json!({"events": events})),
    )
    .await?;
    answered(response, "quotes").await
}

/// Move each desk mark by the tick's change, from its level now; a bond not yet marked
/// stays unmarked.
async fn move_marks(
    server: &ServerClient,
    plan: &FeedPlan,
    rates_move: f64,
    credit_move: f64,
) -> Result<(), String> {
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
    let marked: HashMap<String, Decimal> = marks
        .levels
        .into_iter()
        .filter_map(|bond| Some((bond.instrument_id, bond.mark?.level)))
        .collect();
    let mut levels = Vec::new();
    for bond in &plan.levels {
        let Some(level) = marked.get(&bond.instrument_id) else {
            continue;
        };
        let spread_move = bond.credit_beta * credit_move;
        let moved = match bond.field.as_str() {
            "mid_spread" => (level + decimal(spread_move)?).round_dp(2),
            "mid_yield" => (level + decimal((rates_move + spread_move) / 10_000.0)?).round_dp(6),
            other => return Err(format!("a desk mark's field is {other}")),
        };
        levels.push(
            json!({"instrument_id": bond.instrument_id, "field": bond.field,
                           "level": moved.to_string()}),
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

/// Publish each weekday's fixing after `last` that is due, at 13:00 UTC on the next
/// weekday; the last one published.
async fn publish_due_fixings(
    server: &ServerClient,
    fixings: &FixingPlan,
    mut last: NaiveDate,
) -> Result<NaiveDate, String> {
    loop {
        let observation = next_weekday(last);
        let published = next_weekday(observation)
            .and_hms_opt(FIXING_HOUR_UTC, 0, 0)
            .map(|at| Utc.from_utc_datetime(&at))
            .ok_or("a fixing's publication time is not a time")?;
        if Utc::now() < published {
            return Ok(last);
        }
        let stamp = published.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let fixing = json!([{
            "schema_version": 1,
            "key": {"index_id": fixings.index_id, "observation_date": observation.to_string(),
                    "source": fixings.source},
            "version": 1, "rate": {"unit": "decimal_rate", "value": fixings.rate.to_string()},
            "published_at": stamp, "recorded_at": Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            "is_preliminary": false, "entitled": true, "evidence_reference": "synthetic",
        }]);
        let response = send_until_answered(
            server
                .request(Method::POST, "/api/v1/analytics/fixings")
                .json(&fixing),
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

fn next_weekday(day: NaiveDate) -> NaiveDate {
    let mut next = day.succ_opt().unwrap_or(day);
    while matches!(next.weekday(), Weekday::Sat | Weekday::Sun) {
        next = next.succ_opt().unwrap_or(next);
    }
    next
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
        assert!(market.rates_bp.abs() < bound, "{}", market.rates_bp);
        assert!(market.credit_bp.abs() < bound, "{}", market.credit_bp);
    }

    #[test]
    fn credit_moves_against_rates() {
        let mut market = Market::default();
        let mut draws = Draws(11);
        let moves: Vec<(f64, f64)> = (0..20_000).map(|_| market.step(&mut draws)).collect();
        let covariance: f64 = moves
            .iter()
            .map(|(rates, credit)| rates * credit)
            .sum::<f64>()
            / moves.len() as f64;
        let expected = CORRELATION * RATES_VOLATILITY_BP * CREDIT_VOLATILITY_BP;
        assert!(
            (covariance - expected).abs() < 0.01,
            "{covariance} against {expected}"
        );
    }

    #[test]
    fn a_weekday_fixing_is_published_on_the_next_weekday() {
        let friday = NaiveDate::from_ymd_opt(2026, 11, 27).unwrap();
        assert_eq!(
            next_weekday(friday),
            NaiveDate::from_ymd_opt(2026, 11, 30).unwrap()
        );
        let monday = NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        assert_eq!(
            next_weekday(monday),
            NaiveDate::from_ymd_opt(2026, 12, 1).unwrap()
        );
    }
}
