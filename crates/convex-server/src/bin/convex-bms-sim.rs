//! The books' owning system, for a desk demo and tests. The server records a `done`
//! request for quote and books nothing; this follows the `rfq` topic on its books and sends
//! each book a new snapshot carrying each done trade, through `PUT /api/v1/books/:book_id`.
//!
//! ```text
//! convex-bms-sim --server http://127.0.0.1:8080 --books BOOK-IG [--trades 10]
//! ```
//!
//! `--trades` stops after that many trades are applied. A trade changes the face the book
//! holds of the bond by the client's side: a client buy shortens the book. A position held
//! in units is not changed, nor is a retired book, which a snapshot would revive; each such
//! trade is reported unapplied. A position the owning system named by an identifier is
//! sent by it again. Each trade's snapshot is sent over the version it was read from,
//! under an idempotency key made from the request's key, and sent again as it was while no
//! reply comes or the server is too busy for it, after the wait it asks for, so a commit
//! whose reply was lost is answered from the receipt the server keeps; one refused because
//! another writer committed meanwhile is built again. A snapshot sent and never answered
//! ends the run, so it is not reported unapplied. A `done` recorded while this was not
//! running is not applied.
//! Each stdout line is one JSON event: `following` once it follows the books, then
//! `applied` or `unapplied` for each trade.
//!
//! Under required access the identity, whose token is read from `CONVEX_SIM_TOKEN`, needs
//! `book_writer` and `reader` on the books, every instrument they hold or their limits
//! name, and the bonds traded.
#![forbid(unsafe_code)]

#[path = "simulation/server_client.rs"]
mod server_client;

use std::process::ExitCode;

use convex_core::contracts::ExactDecimal;
use convex_engine::ports::reference_master::{BookPosition, QuantityUnit};
use convex_engine::ports::rfq::RfqState;
use reqwest::{Method, StatusCode};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use server_client::{send_until_answered, Arguments, RfqRow, ServerClient};

/// Times a snapshot is built again after another writer committed one.
const ATTEMPTS: usize = 5;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("convex-bms-sim: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let arguments = Arguments::parse(&["server", "books", "trades"])?;
    let server = ServerClient::new(arguments.required("server")?)?;
    let trades: usize = arguments.optional("trades", usize::MAX)?;
    let books = arguments.list("books")?;
    let mut topic = server.rfq_topic(&books).await?;
    println!(
        "{}",
        json!({"event": "following", "books": books, "open": topic.open_count()})
    );
    let mut applied = 0usize;
    while applied < trades {
        let row = topic.next(&server).await?;
        if row.state == RfqState::Done && apply(&server, &row).await? {
            applied += 1;
        }
    }
    Ok(())
}

/// A book as read: when it was retired, what its snapshot carries besides its positions,
/// and its positions.
#[derive(Deserialize)]
struct Book {
    effective_until: Option<String>,
    name: Option<String>,
    currency: Option<String>,
    description: Option<String>,
    positions: Vec<BookPosition>,
}

/// Send the done trade's book a snapshot carrying it; whether it is applied.
async fn apply(server: &ServerClient, trade: &RfqRow) -> Result<bool, String> {
    let book_id = trade.request.book_id.as_str();
    let path = format!("/api/v1/books/{book_id}");
    let idempotency_key = format!("rfq-done-{}", trade.rfq_key);
    let change = trade
        .request
        .side
        .position_change(trade.request.face.decimal());
    let print_event = |event: &str, detail: Value| {
        println!(
            "{}",
            json!({"event": event, "rfq_key": trade.rfq_key, "book_id": book_id,
                   "instrument_id": trade.instrument_id, "position_change": change.to_string(),
                   "detail": detail})
        );
    };
    for _ in 0..ATTEMPTS {
        let response = send_until_answered(server.request(Method::GET, &path)).await?;
        if !response.status().is_success() {
            print_event("unapplied", json!({"status": response.status().as_u16()}));
            return Ok(false);
        }
        let version = response
            .headers()
            .get("etag")
            .and_then(|etag| etag.to_str().ok())
            .ok_or("a book has no version")?
            .to_owned();
        let book: Book = response
            .json()
            .await
            .map_err(|error| format!("a book is unreadable: {error}"))?;
        if book.effective_until.is_some() {
            print_event("unapplied", json!({"reason": "the book is retired"}));
            return Ok(false);
        }
        let positions = match traded(&book.positions, &trade.instrument_id, change) {
            Ok(positions) => positions,
            Err(reason) => {
                print_event("unapplied", json!({"reason": reason}));
                return Ok(false);
            }
        };
        let snapshot = json!({
            "name": book.name,
            "currency": book.currency,
            "description": book.description,
            "positions": positions,
        });
        let response = send_until_answered(
            server
                .request(Method::PUT, &path)
                .header("if-match", &version)
                .header("idempotency-key", &idempotency_key)
                .json(&snapshot),
        )
        .await?;
        match response.status() {
            status if status.is_success() => {
                let written: Value = response.json().await.unwrap_or(Value::Null);
                print_event("applied", json!({"version": written["version"]}));
                return Ok(true);
            }
            // Another writer committed a version meanwhile: build the snapshot again.
            StatusCode::CONFLICT => continue,
            status => {
                let reply = response.text().await.unwrap_or_default();
                print_event(
                    "unapplied",
                    json!({"status": status.as_u16(), "reply": reply}),
                );
                return Ok(false);
            }
        }
    }
    print_event("unapplied", json!({"reason": "the book kept moving"}));
    Ok(false)
}

/// `positions` as snapshot rows, with `change` in face added to `instrument_id`'s; that
/// position is left out if it nets to zero. Each row names its bond as the owning system
/// did: by the identifier it gave, or by the instrument.
fn traded(
    positions: &[BookPosition],
    instrument_id: &str,
    change: Decimal,
) -> Result<Vec<Value>, String> {
    let mut rows = Vec::with_capacity(positions.len() + 1);
    let mut held = false;
    for position in positions {
        let mut quantity = position.quantity.decimal();
        if position.instrument_id.as_str() == instrument_id {
            if position.unit != QuantityUnit::Face {
                return Err("the book holds the bond in units".into());
            }
            held = true;
            quantity += change;
            if quantity.is_zero() {
                continue;
            }
        }
        rows.push(json!({
            "instrument_id": position.source_identifier.is_none().then_some(&position.instrument_id),
            "identifier": position.source_identifier,
            "quantity": ExactDecimal::new(quantity.normalize()),
            "unit": position.unit,
            "reference_version": position.reference_version,
            "sector": position.sector,
            "rating": position.rating,
        }));
    }
    if !held {
        rows.push(json!({
            "instrument_id": instrument_id,
            "quantity": ExactDecimal::new(change.normalize()),
            "unit": QuantityUnit::Face,
        }));
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn positions() -> Vec<BookPosition> {
        serde_json::from_value(json!([
            {"instrument_id": "BOND-001", "quantity": "1000000", "unit": "face", "sector": "IG"},
            {"instrument_id": "BOND-002", "quantity": "250", "unit": "units"},
            {"instrument_id": "BOND-003", "quantity": "0", "unit": "face",
             "source_identifier": {"kind": "isin", "value": "US0000000001"}},
        ]))
        .unwrap()
    }

    #[test]
    fn a_trade_changes_the_bonds_face_and_keeps_the_other_positions() {
        let rows = traded(&positions(), "BOND-001", dec!(-400000)).unwrap();
        assert_eq!(rows[0]["instrument_id"], "BOND-001");
        assert_eq!(rows[0]["quantity"], "600000");
        assert_eq!(rows[0]["sector"], "IG");
        assert_eq!(rows[1]["quantity"], "250");
        assert_eq!(rows[1]["unit"], "units");
        // A position named by an identifier is sent by it, and one the trade does not
        // touch is kept even at zero.
        assert_eq!(rows[2]["instrument_id"], Value::Null);
        assert_eq!(rows[2]["identifier"]["value"], "US0000000001");
        assert_eq!(rows.len(), 3);
        // A bond not held is added; the traded one netting to zero is left out.
        let rows = traded(&positions(), "BOND-004", dec!(5000000)).unwrap();
        assert_eq!(
            (
                &rows[3]["instrument_id"],
                &rows[3]["quantity"],
                &rows[3]["unit"]
            ),
            (&json!("BOND-004"), &json!("5000000"), &json!("face"))
        );
        let rows = traded(&positions(), "BOND-001", dec!(-1000000)).unwrap();
        assert_eq!(rows.len(), 2);
        // A position in units is not changed.
        assert!(traded(&positions(), "BOND-002", dec!(1)).is_err());
    }
}
