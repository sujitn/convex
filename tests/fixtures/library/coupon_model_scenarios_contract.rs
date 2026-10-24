//! Independent full-contract scenario values plus exact shared adapter envelopes.
#![allow(dead_code)]
#[path = "contract.rs"]
mod original;
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("coupon-model-scenarios.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    original::request(case)
}
pub fn check(case: &Value, output: &str) {
    original::check(case, output);
    if !case["expected_error"].is_null() {
        return;
    }
    let envelope: Value = serde_json::from_str(output).unwrap();
    let data = &envelope["result"]["data"];
    for (actual, expected) in [
        (&data["base"]["present_value"], &case["expected"]["base_pv"]),
        (
            &data["scenario"]["present_value"],
            &case["expected"]["scenario_pv"],
        ),
    ] {
        assert!(
            (actual.as_f64().unwrap() - expected.as_f64().unwrap()).abs() <= 1e-8,
            "{}: {data}",
            case["id"]
        );
    }
    assert_eq!(
        data["coordinate"],
        case["request"]["operation"]["coordinate"]
    );
    assert_eq!(data["delta"], case["request"]["operation"]["delta"]);
    assert_eq!(
        data["original_parameter"],
        case["expected"]["original_parameter"]
    );
    assert!(
        (data["scenario_parameter"].as_f64().unwrap()
            - case["expected"]["scenario_parameter"].as_f64().unwrap())
        .abs()
            <= 1e-15
    );
    for field in ["terms", "context", "discount_curve"] {
        assert_eq!(data["base"][field], data["scenario"][field]);
    }
    assert_eq!(
        data["base"]["terms"],
        case["request"]["operation"]["input"]["bond"]["terms"]
    );
    let original_input: convex::library::TermCouponOptionInput =
        serde_json::from_value(case["request"]["operation"]["input"].clone()).unwrap();
    let original_options = serde_json::to_value(original_input.options).unwrap();
    let mut expected_options = original_options.clone();
    let c = &data["coordinate"];
    let whole = c["parameter"]["kind"] == "normal_factor_loading";
    let points = if whole {
        &mut expected_options["whole_coupon"]["factors"]
    } else {
        &mut expected_options["models"]
    };
    let p = points
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| {
            p["index_id"] == c["index_id"]
                && p["observation_date"] == c["observation_date"]
                && p["payment_date"] == c["payment_date"]
        })
        .unwrap();
    assert_eq!(p["source_id"], c["source_id"]);
    assert_eq!(p["version"], c["source_version"]);
    if whole {
        p["loadings"][c["parameter"]["factor"].as_u64().unwrap() as usize] =
            data["scenario_parameter"].clone();
    } else {
        let field = if c["parameter"]["kind"] == "lognormal_displacement" {
            "displacement"
        } else {
            "volatility"
        };
        p["model"][field] = data["scenario_parameter"].clone();
    }
    assert_eq!(data["scenario"]["options"], expected_options);
    assert_eq!(data["base"]["options"], original_options);
    for (base, shocked) in data["base"]["segments"]
        .as_array()
        .unwrap()
        .iter()
        .zip(data["scenario"]["segments"].as_array().unwrap())
    {
        for key in [
            "start",
            "end",
            "payment_date",
            "request",
            "rate",
            "principal",
            "year_fraction",
            "entitled",
        ] {
            assert_eq!(base[key], shocked[key]);
        }
    }
    if data["delta"] == 0.0 {
        assert_eq!(data["base"], data["scenario"]);
    }
}
pub fn run() -> Value {
    let rows: Vec<_> = cases().into_iter().map(|case| {
        let output = convex::library::execute_json(&request(&case));
        check(&case, &output);
        json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&output).unwrap()})
    }).collect();
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
