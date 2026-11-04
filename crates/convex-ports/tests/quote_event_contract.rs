use convex_ports::market_data::*;

fn event() -> QuoteEvent {
    serde_json::from_str(
        include_str!("../../convex-ext-file/tests/fixtures/sequenced-quotes.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn quote_wire_retains_decimal_units_metadata_and_rejects_numeric_tokens() {
    let e = event();
    e.validate().unwrap();
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.contains("-0.0012345678901234567890123456"));
    assert_eq!(serde_json::from_str::<QuoteEvent>(&json).unwrap(), e);
    let mut value = serde_json::to_value(&e).unwrap();
    value["event"]["fields"][0]["update"]["value"]["value"] = serde_json::json!(99.0);
    assert!(serde_json::from_value::<QuoteEvent>(value).is_err());
}

#[test]
fn unknown_fields_are_rejected_at_every_envelope_and_payload_boundary() {
    for path in [
        vec![],
        vec!["key"],
        vec!["event"],
        vec!["event", "fields", "0"],
        vec!["event", "fields", "0", "update"],
        vec!["event", "fields", "0", "update", "value"],
    ] {
        let mut value = serde_json::to_value(event()).unwrap();
        let mut node = &mut value;
        for name in path {
            node = if name == "0" {
                &mut node[0]
            } else {
                &mut node[name]
            };
        }
        node["unrecognized"] = serde_json::json!(true);
        assert!(serde_json::from_value::<QuoteEvent>(value).is_err());
    }
}

#[test]
fn image_delta_and_clear_are_distinct_and_bounded() {
    let mut e = event();
    e.event = QuoteEventKind::Image {
        fields: vec![QuoteChange {
            field: QuoteField::BidPrice,
            update: FieldUpdate::Clear,
        }],
    };
    assert!(e.validate().is_err());
    e.event = QuoteEventKind::Delta {
        fields: vec![QuoteChange {
            field: QuoteField::BidPrice,
            update: FieldUpdate::Clear,
        }],
    };
    e.validate().unwrap();
    e.event = QuoteEventKind::Delta {
        fields: vec![
            QuoteChange {
                field: QuoteField::BidPrice,
                update: FieldUpdate::Unchanged
            };
            MAX_QUOTE_FIELDS + 1
        ],
    };
    assert!(e.validate().is_err());
}
