//! Two-way quotes from width schedules, checked exactly against the shared fixture.

use std::str::FromStr;

use convex_core::types::{Mark, QuoteWidthError, Spread, WidthSchedule, WidthUnit};
use rust_decimal::Decimal;
use serde_json::{json, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../tests/fixtures/quote-widths.json")).unwrap()
}

fn decimal(value: &Value) -> Decimal {
    Decimal::from_str_exact(value.as_str().unwrap()).unwrap()
}

fn inputs(case: &Value) -> (Mark, WidthSchedule, Option<Decimal>) {
    let mid = Mark::from_str(case["mid"].as_str().unwrap()).unwrap();
    let schedule = serde_json::from_value(case["schedule"].clone()).unwrap();
    let face = (!case["face"].is_null()).then(|| decimal(&case["face"]));
    (mid, schedule, face)
}

/// `mark` at another level, in the same form: a price's kind, a yield's frequency, a
/// spread's type and benchmark.
fn at_level(mark: &Mark, level: Decimal) -> Mark {
    match mark {
        Mark::Price { kind, .. } => Mark::Price {
            value: level,
            kind: *kind,
        },
        Mark::Yield { frequency, .. } => Mark::Yield {
            value: level,
            frequency: *frequency,
        },
        Mark::Spread { value, benchmark } => Mark::Spread {
            value: Spread::new(level, value.spread_type()),
            benchmark: benchmark.clone(),
        },
    }
}

fn refusal_code(error: &QuoteWidthError) -> &'static str {
    match error {
        QuoteWidthError::BucketCount => "bucket_count",
        QuoteWidthError::BucketBounds => "bucket_bounds",
        QuoteWidthError::WidthOutOfRange { .. } => "width_out_of_range",
        QuoteWidthError::FaceNotPositive => "face_not_positive",
        QuoteWidthError::FaceAboveSchedule => "face_above_schedule",
        QuoteWidthError::UnitMismatch { .. } => "unit_mismatch",
        QuoteWidthError::BidPriceNotPositive => "bid_price_not_positive",
        QuoteWidthError::Overflow => "overflow",
    }
}

#[test]
fn each_case_gives_its_width_bid_and_ask() {
    let fixture = fixture();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 15);
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let (mid, schedule, face) = inputs(case);
        let expected = &case["expected"];
        let width = schedule
            .width_for(face)
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(width.unit(), schedule.unit, "{id}: unit");
        assert_eq!(width.value(), decimal(&expected["width"]), "{id}: width");
        let two_way = width.two_way(&mid).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(
            two_way.bid,
            at_level(&mid, decimal(&expected["bid"])),
            "{id}: bid"
        );
        assert_eq!(
            two_way.ask,
            at_level(&mid, decimal(&expected["ask"])),
            "{id}: ask"
        );
    }
}

#[test]
fn each_refusal_gives_its_reason() {
    let fixture = fixture();
    let refusals = fixture["refusals"].as_array().unwrap();
    assert_eq!(refusals.len(), 18);
    for case in refusals {
        let id = case["id"].as_str().unwrap();
        let (mid, schedule, face) = inputs(case);
        let error = schedule
            .width_for(face)
            .and_then(|width| width.two_way(&mid))
            .expect_err(id);
        assert_eq!(
            refusal_code(&error),
            case["refused"].as_str().unwrap(),
            "{id}: {error}"
        );
    }
}

#[test]
fn a_refusal_names_the_unit_and_its_range() {
    let schedule: WidthSchedule = serde_json::from_value(json!({
        "unit": "thirty_seconds",
        "buckets": [{ "width": "641" }]
    }))
    .unwrap();
    assert_eq!(
        schedule.validate().unwrap_err().to_string(),
        "a width in 32nds is from 0 to 640"
    );
    let basis_points: WidthSchedule = serde_json::from_value(json!({
        "unit": "basis_points",
        "buckets": [{ "width": "2" }]
    }))
    .unwrap();
    assert_eq!(
        basis_points
            .width_for(None)
            .unwrap()
            .two_way(&Mark::from_str("99.5").unwrap())
            .unwrap_err()
            .to_string(),
        "a width in basis points does not widen a price level"
    );
}

#[test]
fn a_schedule_on_the_wire_is_exact_decimal_strings_and_known_fields() {
    let schedule = json!({
        "unit": "basis_points",
        "buckets": [
            { "max_face": "1000000", "width": "6" },
            { "width": "4.5" }
        ]
    });
    let decoded: WidthSchedule = serde_json::from_value(schedule.clone()).unwrap();
    assert_eq!(decoded.unit, WidthUnit::BasisPoints);
    assert_eq!(serde_json::to_value(&decoded).unwrap(), schedule);

    for refused in [
        json!({ "unit": "basis_points", "buckets": [{ "width": 6 }] }),
        json!({ "unit": "basis_points", "buckets": [{ "width": "6", "side": "bid" }] }),
        json!({ "unit": "basis_points", "buckets": [], "method": "yield" }),
        json!({ "unit": "percent", "buckets": [{ "width": "6" }] }),
    ] {
        assert!(
            serde_json::from_value::<WidthSchedule>(refused.clone()).is_err(),
            "{refused}"
        );
    }
}
