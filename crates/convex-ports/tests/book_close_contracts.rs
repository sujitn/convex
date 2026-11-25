//! A book's close record: its wire form and what it refuses on its own.

use convex_ports::book_close::BookCloseDefinition;
use convex_ports::reference_master::{MasterDefinition, MasterKey, MasterKind};
use convex_ports::reference_repository::ReferenceError;
use serde_json::{json, Value};

fn record() -> Value {
    json!({
        "book_id": "DESK",
        "close_date": "2026-10-05",
        "basis": "desk_close",
        "taken_by": {"kind": "desk", "subject": "access:3f9a"},
        "book_version": 4,
        "currency": "USD",
        "settlement_date": "2026-10-06",
        "snapshot_ids": ["snapshot-1", "snapshot-2"],
        "positions": [
            {"instrument_id": "BOND-001", "quantity": "1000000", "unit": "face",
             "value": {"status": "valued", "instrument_version": 1,
                       "clean_price_mid": "99.25", "accrued_interest": "1.1956521739130434"}},
            {"instrument_id": "BOND-002", "quantity": "-25", "unit": "units",
             "value": {"status": "unvalued", "reason": {"code": "unsupported_quantity_unit"}}},
            {"instrument_id": "BOND-003", "quantity": "500000", "unit": "face",
             "value": {"status": "unvalued",
                       "reason": {"code": "quote_refused", "detail": "stale"}}},
        ],
        "effective": {"from": "2026-10-05T21:30:00Z", "until": null},
        "provenance": {"source": "book-close", "evidence_reference": "book-close:close-1"},
    })
}

fn definition(value: Value) -> BookCloseDefinition {
    serde_json::from_value(value).unwrap()
}

fn refused(value: Value) -> ReferenceError {
    MasterDefinition::BookClose(Box::new(definition(value)))
        .validate()
        .unwrap_err()
}

#[test]
fn a_close_round_trips_and_is_keyed_by_its_book() {
    let close = definition(record());
    assert_eq!(serde_json::to_value(&close).unwrap(), record());
    assert_eq!(close.taken_at(), close.effective.from);
    let master = MasterDefinition::BookClose(Box::new(close));
    master.validate().unwrap();
    let key = master.key();
    assert_eq!(key.kind(), MasterKind::BookClose);
    assert_eq!(key.storage_key(), "book_close/DESK");
    assert!(matches!(key, MasterKey::BookClose(_)));
    let mut scheduled = record();
    scheduled["taken_by"] = json!({"kind": "schedule"});
    MasterDefinition::BookClose(Box::new(definition(scheduled)))
        .validate()
        .unwrap();
}

#[test]
fn a_close_refuses_unknown_fields() {
    let mut extra = record();
    extra["positions"][0]["value"]["dirty_price"] = json!("100.4456521739130434");
    assert!(serde_json::from_value::<BookCloseDefinition>(extra).is_err());
    let mut unknown = record();
    unknown["basis"] = json!("official");
    assert!(serde_json::from_value::<BookCloseDefinition>(unknown).is_err());
}

#[test]
fn a_close_refuses_what_it_cannot_hold() {
    let cases = [
        (
            "none valued",
            "/positions/0/value",
            json!({"status": "unvalued", "reason": {"code": "missing_result"}}),
        ),
        (
            "an instrument twice",
            "/positions/1/instrument_id",
            json!("BOND-001"),
        ),
        (
            "a negative dirty price",
            "/positions/0/value/accrued_interest",
            json!("-99.26"),
        ),
        (
            "a negative clean price",
            "/positions/0/value/clean_price_mid",
            json!("-0.01"),
        ),
        (
            "unsorted snapshots",
            "/snapshot_ids",
            json!(["snapshot-2", "snapshot-1"]),
        ),
        ("no snapshot", "/snapshot_ids", json!([])),
        (
            "a retirement",
            "/effective/until",
            json!("2026-10-06T00:00:00Z"),
        ),
        ("an unknown currency", "/currency", json!("XXZ")),
        (
            "a subject with a space",
            "/taken_by/subject",
            json!("a subject"),
        ),
        (
            "a long refusal",
            "/positions/2/value/reason/detail",
            json!("x".repeat(257)),
        ),
    ];
    for (case, pointer, replacement) in cases {
        let mut value = record();
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            matches!(refused(value), ReferenceError::InvalidInput(_)),
            "{case}"
        );
    }
}
