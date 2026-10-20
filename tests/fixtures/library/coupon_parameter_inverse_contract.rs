//! Independent full currency targets/parameters precede shared transport parity.
#![allow(dead_code)]
#[path = "contract.rs"]
mod original;
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("coupon-parameter-inverse.json")).unwrap()["cases"]
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
    let operation = &case["request"]["operation"];
    assert!(
        (data["parameter"].as_f64().unwrap() - case["expected_parameter"].as_f64().unwrap()).abs()
            <= case["absolute_parameter_bound"].as_f64().unwrap(),
        "{}",
        case["id"]
    );
    for (field, expected) in [("base", "expected_base_pv"), ("valuation", "expected_pv")] {
        assert!(
            (data[field]["present_value"].as_f64().unwrap() - case[expected].as_f64().unwrap())
                .abs()
                <= case["absolute_pv_bound"].as_f64().unwrap()
        );
        for (output, input) in [
            ("terms", &operation["input"]["bond"]["terms"]),
            ("context", &operation["input"]["context"]),
            ("discount_curve", &operation["input"]["discount"]),
        ] {
            assert_eq!(&data[field][output], input);
        }
    }
    assert_eq!(data["coordinate"], operation["coordinate"]);
    assert_eq!(data["target"], operation["target"]);
    assert_eq!(data["globally_unique"], false);
    let settings: &Value = &operation["settings"];
    let actual = data["valuation"]["present_value"].as_f64().unwrap();
    assert_eq!(
        data["native_residual"].as_f64().unwrap(),
        actual - operation["target"]["value"].as_f64().unwrap()
    );
    assert!(
        data["native_residual"].as_f64().unwrap().abs()
            <= settings["price_tolerance"].as_f64().unwrap()
    );
    assert!(data["price_resolution"].as_f64().unwrap() > 0.);
    let points = data["evaluations"].as_array().unwrap();
    assert_eq!(
        points.len(),
        data["price_evaluations"].as_u64().unwrap() as usize
    );
    assert!(points.len() <= settings["max_price_evaluations"].as_u64().unwrap() as usize);
    assert_eq!(points[0]["parameter"], data["original_parameter"]);
    for point in points.iter().skip(1) {
        let p = point["parameter"].as_f64().unwrap();
        assert!(
            p >= settings["lower"].as_f64().unwrap() && p <= settings["upper"].as_f64().unwrap()
        );
        assert_eq!(
            point["residual"].as_f64().unwrap(),
            point["present_value"].as_f64().unwrap()
                - operation["target"]["value"].as_f64().unwrap()
        );
    }
    assert_eq!(points.last().unwrap()["parameter"], data["parameter"]);
    assert_eq!(
        points.last().unwrap()["present_value"],
        data["valuation"]["present_value"]
    );
    let original_input: convex::library::TermCouponOptionInput =
        serde_json::from_value(operation["input"].clone()).unwrap();
    let mut expected_options = serde_json::to_value(original_input.options).unwrap();
    let coordinate = &operation["coordinate"];
    let whole = coordinate["parameter"]["kind"] == "normal_factor_loading";
    let rows = if whole {
        &mut expected_options["whole_coupon"]["factors"]
    } else {
        &mut expected_options["models"]
    };
    let point = rows
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| {
            p["index_id"] == coordinate["index_id"]
                && p["observation_date"] == coordinate["observation_date"]
                && p["payment_date"] == coordinate["payment_date"]
        })
        .unwrap();
    if whole {
        point["loadings"][coordinate["parameter"]["factor"].as_u64().unwrap() as usize] =
            data["parameter"].clone();
    } else if coordinate["parameter"]["kind"] == "lognormal_displacement" {
        point["model"]["displacement"] = data["parameter"].clone();
    } else {
        point["model"]["volatility"] = data["parameter"].clone();
    }
    assert_eq!(data["valuation"]["options"], expected_options);
    let mut refresh = case["request"].clone();
    refresh["operation"] = json!({"kind":"term_coupon_options","input":operation["input"]});
    refresh["operation"]["input"]["options"] = expected_options;
    let fresh = original::native(&refresh.to_string());
    assert_eq!(fresh["ok"], "true");
    assert_eq!(fresh["result"]["data"], data["valuation"]);
}
pub fn run() -> Value {
    let rows:Vec<_>=cases().into_iter().map(|case| {
        let output=convex::library::execute_json(&request(&case));check(&case,&output);
        json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&output).unwrap()})
    }).collect();
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
