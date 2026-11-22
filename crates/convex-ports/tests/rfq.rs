//! The RFQ model: what a source sends, the closed state table and the sign of a done trade.

use convex_core::contracts::UtcTimestamp;
use convex_ports::rfq::*;
use rust_decimal::Decimal;
use serde_json::{json, Value};

fn received_at() -> UtcTimestamp {
    UtcTimestamp::parse("2026-11-22T12:00:00Z").unwrap()
}

fn request() -> Value {
    json!({
        "source_id": "venue-sim",
        "rfq_id": "RFQ-0001",
        "client_id": "CLIENT-17",
        "client_tier": "2",
        "instrument_id": "BOND-001",
        "side": "buy",
        "face": "5000000",
        "settlement_date": "2026-11-24",
        "book_id": "BOOK-IG",
        "deadline": "2026-11-22T12:00:30Z",
    })
}

/// The request naming its bond by `identifier` in place of its instrument id.
fn by_identifier(identifier: Value) -> Value {
    let mut wire = request();
    wire.as_object_mut().unwrap().remove("instrument_id");
    wire["identifier"] = identifier;
    wire
}

fn decoded(wire: Value) -> RfqRequest {
    serde_json::from_value(wire).unwrap()
}

/// Every move the table allows, written out apart from the function under test.
const ALLOWED: [(RfqState, &str, RfqState); 12] = [
    (RfqState::Received, "priced", RfqState::Priced),
    (RfqState::Priced, "quoted", RfqState::Quoted),
    (RfqState::Priced, "passed", RfqState::Passed),
    (RfqState::Quoted, "done", RfqState::Done),
    (RfqState::Quoted, "traded_away", RfqState::TradedAway),
    (RfqState::Quoted, "covered", RfqState::Covered),
    (RfqState::Received, "expired", RfqState::Expired),
    (RfqState::Priced, "expired", RfqState::Expired),
    (RfqState::Quoted, "expired", RfqState::Expired),
    (RfqState::Received, "cancelled", RfqState::Cancelled),
    (RfqState::Priced, "cancelled", RfqState::Cancelled),
    (RfqState::Quoted, "cancelled", RfqState::Cancelled),
];

fn event_name(event: RfqEvent) -> String {
    serde_json::to_value(event).unwrap()["event"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn every_state_and_event_moves_as_the_table_says_and_no_other_way() {
    for state in RfqState::ALL {
        for event in RfqEvent::ALL {
            let name = event_name(event);
            let allowed = ALLOWED
                .iter()
                .find(|(from, by, _)| *from == state && *by == name)
                .map(|(_, _, to)| *to);
            match (transition(state, event), allowed) {
                (Ok(to), Some(expected)) => assert_eq!(to, expected, "{state} {name}"),
                (Err(refused), None) => {
                    let (expected, code) = if state.is_final() {
                        (RfqTransitionError::Final(state), "RFQ_FINAL")
                    } else {
                        (RfqTransitionError::NotAllowed(state), "RFQ_TRANSITION")
                    };
                    assert_eq!(refused, expected, "{state} {name}");
                    assert_eq!(refused.code(), code);
                }
                (outcome, expected) => {
                    panic!("{state} {name}: {outcome:?}, expected {expected:?}")
                }
            }
        }
    }
    let open: Vec<_> = RfqState::ALL
        .into_iter()
        .filter(|state| !state.is_final())
        .collect();
    assert_eq!(
        open,
        [RfqState::Received, RfqState::Priced, RfqState::Quoted]
    );
    let expired = RfqEvent::Expired {
        reason: ExpiryReason::Deadline,
    };
    let refused = transition(
        transition(RfqState::Quoted, expired).unwrap(),
        RfqEvent::Done,
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "the RFQ is final: expired",
        "a source's outcome after the server expired it"
    );
}

#[test]
fn each_event_belongs_to_one_actor_and_a_source_sends_only_its_own() {
    for event in RfqEvent::ALL {
        let name = event_name(event);
        let expected = match name.as_str() {
            "priced" | "expired" => RfqActor::Server,
            "quoted" | "passed" => RfqActor::Trader,
            "done" | "traded_away" | "covered" | "cancelled" => RfqActor::Source,
            other => panic!("{other}"),
        };
        assert_eq!(event.actor(), expected, "{name}");
        let as_outcome = serde_json::from_value::<SourceOutcome>(json!(name));
        assert_eq!(as_outcome.is_ok(), expected == RfqActor::Source, "{name}");
        if let Ok(outcome) = as_outcome {
            assert_eq!(RfqEvent::from(outcome), event);
            assert_eq!(serde_json::to_value(outcome).unwrap(), json!(name));
        }
    }
    let restart = serde_json::to_value(RfqEvent::Expired {
        reason: ExpiryReason::Restart,
    })
    .unwrap();
    assert_eq!(restart, json!({"event": "expired", "reason": "restart"}));
}

#[test]
fn a_client_buy_lifts_the_desk_ask_and_shortens_its_position() {
    let face = Decimal::from(5_000_000);
    assert_eq!(ClientSide::Buy.desk_side(), DeskSide::Ask);
    assert_eq!(ClientSide::Buy.position_change(face), -face);
    assert_eq!(ClientSide::Sell.desk_side(), DeskSide::Bid);
    assert_eq!(ClientSide::Sell.position_change(face), face);
}

#[test]
fn a_state_is_named_on_the_wire_as_in_its_messages() {
    for state in RfqState::ALL {
        assert_eq!(serde_json::to_value(state).unwrap(), json!(state.as_str()));
        assert_eq!(state.to_string(), state.as_str());
    }
}

#[test]
fn a_request_on_the_wire_is_exact_and_names_only_known_fields() {
    let wire = request();
    let request = decoded(wire.clone());
    assert_eq!(serde_json::to_value(&request).unwrap(), wire);
    assert_eq!(request.validate(received_at()), Ok(()));
    let by_isin = decoded(by_identifier(
        json!({"kind": "isin", "value": "US0000000001"}),
    ));
    assert_eq!(by_isin.validate(received_at()), Ok(()));
    for (field, value) in [
        ("face", json!(5000000)),
        ("side", json!("bid")),
        ("deadline", json!(1_795_000_030_000_i64)),
        ("settlement_date", json!("2026-11-31")),
        ("price", json!("99.5")),
    ] {
        let mut refused = wire.clone();
        refused[field] = value;
        assert!(
            serde_json::from_value::<RfqRequest>(refused).is_err(),
            "{field}"
        );
    }
}

#[test]
fn a_retry_with_the_same_content_is_the_same_request() {
    let first = decoded(request());
    let mut wire = request();
    wire["face"] = json!("5000000.00");
    assert_eq!(decoded(wire), first, "a face compares by value");
    let mut other = request();
    other["side"] = json!("sell");
    assert_ne!(decoded(other), first);
}

#[test]
fn each_refusal_names_its_reason() {
    let with = |field: &str, value: Value| {
        let mut wire = request();
        wire[field] = value;
        wire
    };
    let long = "x".repeat(MAX_RFQ_ID_BYTES + 1);
    let cases = [
        (with("rfq_id", json!("")), RfqRefusal::RfqId),
        (with("rfq_id", json!(long)), RfqRefusal::RfqId),
        (with("rfq_id", json!("RFQ\n0001")), RfqRefusal::RfqId),
        (with("rfq_id", json!("RFQ-0001 ")), RfqRefusal::RfqId),
        (with("client_id", json!("")), RfqRefusal::ClientId),
        (
            with("client_id", json!("CLIENT\u{7}17")),
            RfqRefusal::ClientId,
        ),
        (
            with("client_tier", json!("x".repeat(MAX_CLIENT_TIER_BYTES + 1))),
            RfqRefusal::ClientTier,
        ),
        (
            with(
                "identifier",
                json!({"kind": "isin", "value": "US0000000001"}),
            ),
            RfqRefusal::Instrument,
        ),
        (
            by_identifier(json!({"kind": "cusip", "value": "9".repeat(129)})),
            RfqRefusal::Instrument,
        ),
        (with("face", json!("0")), RfqRefusal::Face),
        (
            with("deadline", json!("2026-11-22T12:00:00.999999999Z")),
            RfqRefusal::Deadline,
        ),
        (
            with("deadline", json!("2026-11-22T12:05:00.000000001Z")),
            RfqRefusal::Deadline,
        ),
        (
            with("deadline", json!("2026-11-22T11:59:59Z")),
            RfqRefusal::Deadline,
        ),
    ];
    for (wire, expected) in cases {
        let refusal = decoded(wire.clone()).validate(received_at()).unwrap_err();
        assert_eq!(refusal, expected, "{wire}");
        assert!(refusal.code().starts_with("RFQ_"));
    }
    assert_eq!((MIN_RFQ_DEADLINE_MS, MAX_RFQ_DEADLINE_MS), (1_000, 300_000));
    for deadline in ["2026-11-22T12:00:01Z", "2026-11-22T12:05:00Z"] {
        let wire = with("deadline", json!(deadline));
        assert_eq!(decoded(wire).validate(received_at()), Ok(()), "{deadline}");
    }
    assert!(RfqRefusal::Deadline.reason().contains("1 to 300 seconds"));
}

#[test]
fn a_full_batch_of_the_largest_requests_fits_one_ingest_body() {
    // Every field at its widest: quotes in the free text double on the wire, and a
    // provider identifier is the longest way to name a bond.
    let token = "X".repeat(128);
    let quoted = |bytes: usize| "\"".repeat(bytes);
    let mut largest = by_identifier(json!({
        "kind": "provider",
        "namespace": token,
        "value": quoted(128),
    }));
    largest["source_id"] = json!(token);
    largest["rfq_id"] = json!(quoted(MAX_RFQ_ID_BYTES));
    largest["client_id"] = json!(quoted(MAX_RFQ_ID_BYTES));
    largest["client_tier"] = json!(quoted(MAX_CLIENT_TIER_BYTES));
    largest["side"] = json!("sell");
    largest["face"] = json!("79228162514264337593543950335");
    largest["book_id"] = json!(token);
    largest["deadline"] = json!("2026-11-22T12:00:30.123456789Z");
    let largest = decoded(largest);
    assert_eq!(largest.validate(received_at()), Ok(()));
    let batch = RfqBatch {
        rfqs: vec![largest; MAX_RFQ_BATCH],
    };
    let wire = serde_json::to_vec(&batch).unwrap();
    assert!(wire.len() <= MAX_RFQ_BODY_BYTES, "{}", wire.len());
    assert_eq!(serde_json::from_slice::<RfqBatch>(&wire).unwrap(), batch);
}
