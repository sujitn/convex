//! Two-way quotes from width schedules and client tier factors, checked exactly against
//! the shared fixture.

use std::str::FromStr;

use convex_core::types::{
    Mark, QuoteBasis, QuoteWidth, QuoteWidthError, WidthFactor, WidthSchedule, WidthUnit,
    MAX_SIZE_BUCKETS, MAX_WIDTH_FACTOR,
};
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
    let face = case["face"]
        .as_str()
        .map(|face| Decimal::from_str_exact(face).unwrap());
    (mid, schedule, face)
}

/// The case's width for its trade's size, scaled by its client tier's factor when it has one.
fn width(
    case: &Value,
    schedule: &WidthSchedule,
    face: Option<Decimal>,
) -> Result<QuoteWidth, QuoteWidthError> {
    let width = schedule.width_for(face)?;
    match case.get("factor").and_then(Value::as_str) {
        Some(factor) => width.scaled(WidthFactor::new(Decimal::from_str_exact(factor).unwrap())?),
        None => Ok(width),
    }
}

fn refusal_code(error: QuoteWidthError) -> &'static str {
    match error {
        QuoteWidthError::NoBuckets => "no_buckets",
        QuoteWidthError::TooManyBuckets => "too_many_buckets",
        QuoteWidthError::BucketBounds => "bucket_bounds",
        QuoteWidthError::WidthOutOfRange => "width_out_of_range",
        QuoteWidthError::FaceNotPositive => "face_not_positive",
        QuoteWidthError::FaceAboveSchedule => "face_above_schedule",
        QuoteWidthError::UnitMismatch => "unit_mismatch",
        QuoteWidthError::BidPriceNotPositive => "bid_price_not_positive",
        QuoteWidthError::Unrepresentable => "unrepresentable",
        QuoteWidthError::FactorOutOfRange => "factor_out_of_range",
    }
}

#[test]
fn each_case_gives_its_width_bid_and_ask() {
    let fixture = fixture();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 22);
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let (mid, schedule, face) = inputs(case);
        let expected = &case["expected"];
        let width = width(case, &schedule, face).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(width.unit(), schedule.unit, "{id}: a factor keeps the unit");
        assert_eq!(width.value(), decimal(&expected["width"]), "{id}: width");
        let two_way = width.two_way(&mid).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(
            two_way.bid,
            mid.at_level(decimal(&expected["bid"])),
            "{id}: bid"
        );
        assert_eq!(
            two_way.ask,
            mid.at_level(decimal(&expected["ask"])),
            "{id}: ask"
        );
    }
}

#[test]
fn each_refusal_gives_its_reason() {
    let fixture = fixture();
    let refusals = fixture["refusals"].as_array().unwrap();
    assert_eq!(refusals.len(), 23);
    for case in refusals {
        let id = case["id"].as_str().unwrap();
        let (mid, schedule, face) = inputs(case);
        let error = width(case, &schedule, face)
            .and_then(|width| width.two_way(&mid))
            .expect_err(id);
        assert_eq!(
            refusal_code(error),
            case["refused"].as_str().unwrap(),
            "{id}: {error}"
        );
    }
}

#[test]
fn a_reason_names_the_bounds_it_enforces() {
    assert!(QuoteWidthError::TooManyBuckets
        .reason()
        .contains(&format!("at most {MAX_SIZE_BUCKETS} ")));
    let reason = QuoteWidthError::WidthOutOfRange.reason();
    for (unit, max_width, text) in [
        (WidthUnit::PricePoints, 20, "20 price points"),
        (WidthUnit::ThirtySeconds, 640, "640 32nds"),
        (WidthUnit::BasisPoints, 1000, "1,000 basis points"),
    ] {
        assert!(reason.contains(text), "{unit:?}");
        assert_eq!(unit.max_width(), Decimal::from(max_width), "{unit:?}");
    }
    assert_eq!(
        QuoteWidthError::UnitMismatch.to_string(),
        QuoteWidthError::UnitMismatch.reason()
    );
}

#[test]
fn price_units_quote_a_price_and_basis_points_a_yield_or_spread() {
    for (unit, price, other) in [
        (WidthUnit::PricePoints, true, false),
        (WidthUnit::ThirtySeconds, true, false),
        (WidthUnit::BasisPoints, false, true),
    ] {
        assert_eq!(unit.quotes(QuoteBasis::Price), price, "{unit:?}");
        assert_eq!(unit.quotes(QuoteBasis::Yield), other, "{unit:?}");
        assert_eq!(unit.quotes(QuoteBasis::Spread), other, "{unit:?}");
    }
    for (text, basis) in [
        ("99-16+", QuoteBasis::Price),
        ("4.65%@SA", QuoteBasis::Yield),
        ("125 G@USD.TSY.10Y", QuoteBasis::Spread),
    ] {
        let mark = Mark::from_str(text).unwrap();
        assert_eq!(mark.quote_basis(), basis, "{text}");
        assert_eq!(mark.at_level(mark.level()), mark, "{text}");
    }
}

#[test]
fn a_width_factor_is_from_zero_to_ten_and_an_exact_decimal_string_on_the_wire() {
    assert_eq!(MAX_WIDTH_FACTOR, Decimal::TEN);
    assert!(QuoteWidthError::FactorOutOfRange
        .reason()
        .contains("from 0 to 10"));
    let factor: WidthFactor = serde_json::from_value(json!("1.25")).unwrap();
    assert_eq!(factor.value(), Decimal::from_str_exact("1.25").unwrap());
    assert_eq!(serde_json::to_value(factor).unwrap(), json!("1.25"));
    for refused in [json!(1.25), json!("10.01"), json!("-0.5"), json!("one")] {
        assert!(
            serde_json::from_value::<WidthFactor>(refused.clone()).is_err(),
            "{refused}"
        );
    }
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
