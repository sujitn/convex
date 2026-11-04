#![allow(dead_code)]
use convex::library::*;
use serde_json::{json, Value};

pub fn controls() -> Value {
    let mut data: Value = serde_json::from_str(include_str!(
        "../curves/product-parametric-conditioned.json"
    ))
    .unwrap();
    let starts: Value =
        serde_json::from_str(include_str!("../curves/product-parametric-starts.json")).unwrap();
    for case in data["cases"].as_array_mut().unwrap() {
        if let Some(initial) = starts["initial_parameters"].get(case["id"].as_str().unwrap()) {
            case["inputs"]["calibration"]["cash"][0]["recipe"]["curve"]["calibration"]
                ["Parametric"]["initial"] = initial.clone();
        }
    }
    data
}
pub fn cases() -> Vec<Value> {
    controls()["cases"].as_array().unwrap().clone()
}
pub fn recipe(case: &Value) -> &Value {
    let g = &case["inputs"]["calibration"];
    if g["rates"].as_array().unwrap().is_empty() {
        &g["cash"][0]["recipe"]
    } else {
        &g["rates"][0]["recipe"]
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
    let cash_case = cases()
        .into_iter()
        .find(|v| v["id"] == "nelson_siegel.government.positive")
        .unwrap();
    let helper = &cash_case["inputs"]["calibration"]["cash"][0]["recipe"]["helpers"][3]["input"];
    json!({"kind":"product_risk",
        "product":{"kind":"bond","bond":{"terms":helper["terms"],"calendars":cash_case["inputs"]["calibration"]["cash"][0]["calendars"][3]["pins"]},
            "context":{"settlement":"2026-10-07","on_payment":"Exclude","fixing":case["inputs"]["calibration"]["rate_context"]["cashflows"]["fixings"],"overnight_accrual_cutoffs":[]},
            "projection":null,"method":{"kind":"discount_spread","curve":rate["recipe"]["curve"]["key"],"role":{"kind":"collateral"},"decimal_spread":0.0}},
        "key":{"coordinate":{"kind":"quote","curve":rate["recipe"]["curve"]["key"],"helper_id":rate["recipe"]["helpers"][1]["id"],"quote_version":rate["quote_versions"][1]},
            "factor":{"family":"interest_rate","target":{"kind":"curve","curve_id":rate["recipe"]["curve"]["key"]["id"]},"currency":"USD","tenor":"1Y","quote_type":"deposit_rate","basis":"market_quote_recalibrated","convention_version":7,"model_version":"fictional.NS.product.v1"},
            "native_unit":rate["recipe"]["helpers"][1]["quote"]["unit"],"response":"rebuild_dependents","unit":"currency_per_bp"},
        "position":{"currency":"USD","quantity":"5000","quantity_unit":"notional","reference_quantity":"5000"},"snapshot_id":"fictional-parametric-snapshot",
        "settings":{"bump":0.0001,"policy_version":7,"stability":{"absolute":0.05,"relative":0.0001}}})
}
pub fn check_transport(case: &Value, actual: &str) {
    let request = request(case, json!({"kind":"build"}));
    let expected: Value = serde_json::from_str(&execute_json(&request)).unwrap();
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
pub fn transport_cases() -> Vec<Value> {
    let mut out = cases();
    let mut bad = out[0].clone();
    bad["id"] = json!("original-decay-domain-failure");
    bad["expected_error"] = json!("invalid_input");
    bad["inputs"]["calibration"]["rates"][0]["recipe"]["curve"]["calibration"]["Parametric"]
        ["initial"]["tau"] = json!(0);
    out.push(bad);
    out
}
