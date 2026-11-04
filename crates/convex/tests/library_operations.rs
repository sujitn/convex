//! Exact original conversions/views and computed bucket scope, beyond transport.
#[path = "../../../tests/fixtures/library/operations_contract.rs"]
mod contract;
use convex::library::*;
use convex_analytics::quote_risk as q;
use convex_curves::contracts::*;
use serde_json::{json, Value};
fn case(id: &str) -> Value {
    contract::cases()
        .into_iter()
        .find(|c| c["id"] == id)
        .unwrap()
}
fn output(id: &str) -> Value {
    let case = case(id);
    let result = contract::native(&contract::request(&case));
    assert_eq!(result["ok"], "true", "{id}: {result}");
    result["result"]["data"].clone()
}
#[test]
fn exact_operation_terms_roundtrip_and_all_original_guards_execute() {
    for case in contract::cases() {
        let input = contract::request(&case);
        let request = decode_request(&input).unwrap();
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            case["request"],
            "{}",
            case["id"]
        );
        contract::check(&case, &execute_json(&input));
    }
}
#[test]
fn dated_conversions_keep_leap_time_units_rounding_and_compounding() {
    let result = output("time.date.ActActIsda");
    assert_eq!(result["time_basis"], "ActActIsda");
    assert_eq!(result["value"].as_f64().unwrap(), 2.0 / 366.0);
    assert_eq!(
        output("time.date.Act360")["value"].as_f64().unwrap(),
        2.0 / 360.0
    );
    assert_eq!(
        output("time.inverse.ActActIsda.AtOrBefore")["value"],
        "2024-02-29"
    );
    assert_eq!(
        output("time.inverse.ActActIsda.AtOrAfter")["value"],
        "2024-03-01"
    );
    assert_eq!(output("time.inverse.origin")["value"], "2024-02-28");
    assert_eq!(output("conversion.zero_origin")["value"], 1.0);
    let c = output("conversion.discount.Simple");
    let fraction = c["year_fraction"].as_f64().unwrap();
    assert_eq!(fraction, 2.0 / 366.0);
    assert_eq!(c["unit"], "discount_factor");
    let df = c["value"].as_f64().unwrap();
    assert!(df > 1.0);
    assert!((df * (1.0 - 0.01 * fraction) - 1.0).abs() < 1e-15);
    assert_eq!(
        output("conversion.equivalent.Simple")["unit"],
        "decimal_rate"
    );
}
#[test]
fn roll_and_representation_views_preserve_exact_native_discount_functions() {
    let case = case("roll.StickyForward");
    let request = decode_request(&contract::request(&case)).unwrap();
    let native = build_curve_set(&request.inputs.curves, &CurveSet::new())
        .unwrap()
        .curves;
    let base = native.get(&request.inputs.curves[0].key).unwrap();
    let rolled = native.get(&request.inputs.curves[1].key).unwrap();
    let target = convex_core::Date::parse("2025-02-28").unwrap();
    let expected = base
        .query(&CurveQuery::DiscountRatio {
            start: CurvePoint::Date(rolled.recipe().reference_date),
            end: CurvePoint::Date(target),
        })
        .unwrap();
    assert_eq!(
        output("roll.StickyForward")["value"].as_f64().unwrap(),
        expected.value
    );
    assert_ne!(
        output("roll.StickyForward")["value"],
        output("roll.StickyZero")["value"]
    );
    assert_eq!(output("roll.StickyForward.origin")["value"], 1.0);
    assert_eq!(output("roll.StickyZero.origin")["value"], 1.0);
    assert_eq!(
        output("view.equivalent.df")["value"],
        output("view.equivalent.zero")["value"]
    );
}
#[test]
fn actual_node_membership_quantity_and_nonadditivity_are_preserved() {
    let long = output("risk.nodes.zero");
    let short = output("risk.nodes.zero.short");
    assert_eq!(long["buckets"].as_array().unwrap().len(), 4);
    // The distant node has no cash exposure but is still mandatory membership.
    assert_eq!(long["buckets"][3]["value"], 0.0);
    let l = long["parallel"]["value"].as_f64().unwrap();
    let s = short["parallel"]["value"].as_f64().unwrap();
    assert_eq!(s, -2.0 * l);
    let zero = output("risk.nodes.df.zero");
    assert_eq!(zero["parallel"]["value"], 0.0);
    assert_eq!(zero["buckets"].as_array().unwrap().len(), 3);
    assert_eq!(
        output("risk.nodes.nonadditive_diagnostic")["reconciliation"]["passed"],
        false
    );
    for id in [
        "risk.nodes.zero.missing_zero_risk",
        "risk.nodes.df.missing_zero_risk",
    ] {
        let case = case(id);
        let result = contract::native(&contract::request(&case));
        assert_eq!(result["error"]["code"], "invalid_input");
        assert!(result.get("result").is_none());
    }
}
#[test]
fn original_mixed_quote_units_and_native_reconciliation_remain_exact() {
    let result = output("risk.quotes.cds");
    let buckets: Vec<q::QuoteSensitivity> =
        serde_json::from_value(result["buckets"].clone()).unwrap();
    let parallel: q::QuoteSensitivity = serde_json::from_value(result["parallel"].clone()).unwrap();
    assert_eq!(buckets.len(), 6);
    assert_eq!(
        buckets[0].key.definition().native_unit,
        QuoteUnit::DecimalRate
    );
    assert_eq!(
        buckets[1].key.definition().native_unit,
        QuoteUnit::BasisPoints
    );
    assert_eq!(
        buckets[2].key.definition().native_unit,
        QuoteUnit::PercentRate
    );
    for bucket in &buckets {
        bucket.validate().unwrap();
    }
    let native = q::reconcile_quote_buckets(
        &buckets,
        &parallel,
        q::RiskTolerance {
            absolute: 0.1,
            relative: 1e-4,
        },
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(native).unwrap(),
        result["reconciliation"]
    );
    assert_eq!(result["reconciliation"]["passed"], true);
    assert_eq!(
        output("risk.nodes.hazard")["buckets"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
}
#[test]
fn new_nested_actions_reject_ignored_terms_and_invalid_wire_numbers() {
    let mut request = case("conversion.discount.Simple")["request"].clone();
    request["operation"]["period"]["invented_basis"] = json!("Act365");
    assert_eq!(
        decode_request(&request.to_string()).unwrap_err().code,
        LibraryErrorCode::InvalidInput
    );
    let mut request = case("risk.nodes.zero")["request"].clone();
    request["operation"]["action"]["input"]["position"]["quantity"] = json!(5000.0);
    assert_eq!(
        decode_request(&request.to_string()).unwrap_err().code,
        LibraryErrorCode::InvalidInput
    );
}
