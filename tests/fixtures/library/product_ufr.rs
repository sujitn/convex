#![allow(dead_code)]
use convex::library::*;
use serde_json::{json, Value};

pub fn controls() -> Value {
    serde_json::from_str(include_str!("../curves/product-ufr-admitted.json")).unwrap()
}
pub fn cases() -> Vec<Value> {
    controls()["cases"].as_array().unwrap().clone()
}
pub fn recipe(case: &Value) -> &Value {
    let graph = &case["inputs"]["calibration"];
    if graph["rates"].as_array().unwrap().is_empty() {
        &graph["cash"][0]["recipe"]
    } else {
        &graph["rates"][0]["recipe"]
    }
}
pub fn pure(case: &Value) -> LibraryResult<CalibrationReplay> {
    CalibrationReplay::build(&decode_calibration_inputs(&case["inputs"])?)
}
pub fn request(case: &Value, action: Value) -> String {
    let mut inputs = case["inputs"].clone();
    let graph = inputs["calibration"].take();
    json!({"schema_version":1,"inputs":inputs,"operation":{"kind":"calibration","input":graph,"action":action}}).to_string()
}
pub fn risk_action(case: &Value) -> Value {
    let rate = &case["inputs"]["calibration"]["rates"][0];
    let cash = cases()
        .into_iter()
        .find(|v| v["id"] == "flat_forward.government.positive")
        .unwrap();
    let helper = &cash["inputs"]["calibration"]["cash"][0]["recipe"]["helpers"][1]["input"];
    json!({"kind":"product_risk",
      "product":{"kind":"bond","bond":{"terms":helper["terms"],"calendars":cash["inputs"]["calibration"]["cash"][0]["calendars"][1]["pins"]},
        "context":{"settlement":"2026-10-07","on_payment":"Exclude","fixing":case["inputs"]["calibration"]["rate_context"]["cashflows"]["fixings"],"overnight_accrual_cutoffs":[]},
        "projection":null,"method":{"kind":"discount_spread","curve":rate["recipe"]["curve"]["key"],"role":{"kind":"collateral"},"decimal_spread":0.0}},
      "key":{"coordinate":{"kind":"quote","curve":rate["recipe"]["curve"]["key"],"helper_id":rate["recipe"]["helpers"][0]["id"],"quote_version":rate["quote_versions"][0]},
        "factor":{"family":"interest_rate","target":{"kind":"curve","curve_id":rate["recipe"]["curve"]["key"]["id"]},"currency":"USD","tenor":"7Y","quote_type":"deposit_rate","basis":"market_quote_recalibrated","convention_version":7,"model_version":"fictional.exponential.tail.v1"},
        "native_unit":"PercentRate","response":"rebuild_dependents","unit":"currency_per_bp"},
      "position":{"currency":"USD","quantity":"5000","quantity_unit":"notional","reference_quantity":"5000"},"snapshot_id":"fictional-ufr-snapshot",
      "settings":{"bump":0.0001,"policy_version":7,"stability":{"absolute":0.05,"relative":0.0001}}})
}
pub fn transport_cases() -> Vec<Value> {
    let mut result = cases();
    for (suffix, value) in [
        ("initial/alpha", json!(0.0)),
        ("parameter_scales", json!([0.01])),
        ("weights", json!([1.0, 0.0])),
        ("base/version", json!("absent.version")),
    ] {
        let mut bad = result[0].clone();
        bad["id"] = json!(suffix);
        bad["expected_error"] = json!(true);
        let path = "/inputs/calibration/rates/0/recipe/curve/calibration/Ufr/".to_owned() + suffix;
        *bad.pointer_mut(&path).unwrap() = value;
        result.push(bad);
    }
    result
}
pub fn check_transport(case: &Value, actual: &str) {
    let expected: Value =
        serde_json::from_str(&execute_json(&request(case, json!({"kind":"build"})))).unwrap();
    let actual: Value = serde_json::from_str(actual).unwrap();
    assert_eq!(actual, expected, "{}", case["id"]);
    assert_eq!(
        actual["ok"],
        if case.get("expected_error").is_some() {
            "false"
        } else {
            "true"
        },
        "{}",
        case["id"]
    );
}
