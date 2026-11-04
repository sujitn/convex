//! Independent full-contract directional controls precede actual adapter parity.
#![allow(dead_code)]
#[path = "contract.rs"]
mod original;
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("coupon-parameter-directional.json")).unwrap()
        ["cases"]
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
    let actual: Value = serde_json::from_str(output).unwrap();
    let data = &actual["result"]["data"];
    let operation = &case["request"]["operation"];
    for field in ["first", "half_first", "second", "half_second"] {
        let bound = case[if field.contains("second") {
            "absolute_second_bound"
        } else {
            "absolute_first_bound"
        }]
        .as_f64()
        .unwrap();
        assert!(
            (data[field].as_f64().unwrap()
                - case["expected"][field]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap())
            .abs()
                <= bound,
            "{}/{field}",
            case["id"]
        );
    }
    assert_eq!(data["coordinate"], operation["coordinate"]);
    assert_eq!(data["settings"], operation["settings"]);
    assert_eq!(data["price_evaluations"], 6);
    assert_eq!(
        data["original_parameter"].as_f64().unwrap(),
        case["expected"]["original_parameter"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap()
    );
    let prices = data["prices"].as_array().unwrap();
    assert_eq!(prices.len(), 6);
    assert!(data["first_resolution"].as_f64().unwrap() > 0.);
    assert!(data["second_resolution"].as_f64().unwrap() > 0.);
    let input: convex::library::TermCouponOptionInput =
        serde_json::from_value(operation["input"].clone()).unwrap();
    let original_options = serde_json::to_value(input.options).unwrap();
    for (price, expected) in prices
        .iter()
        .zip(case["expected"]["prices"].as_array().unwrap())
    {
        assert_eq!(
            price["multiplier"].as_f64().unwrap(),
            expected["multiplier"]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap()
        );
        assert!(
            (price["parameter"].as_f64().unwrap()
                - expected["parameter"]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap())
            .abs()
                < 1e-14
        );
        assert!(
            (price["valuation"]["present_value"].as_f64().unwrap()
                - expected["present_value"]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap())
            .abs()
                <= case["absolute_pv_bound"].as_f64().unwrap()
        );
        let coordinate = &operation["coordinate"];
        let whole = coordinate["parameter"]["kind"] == "normal_factor_loading";
        let mut options = original_options.clone();
        let rows = if whole {
            &mut options["whole_coupon"]["factors"]
        } else {
            &mut options["models"]
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
                price["parameter"].clone();
        } else if coordinate["parameter"]["kind"] == "lognormal_displacement" {
            point["model"]["displacement"] = price["parameter"].clone();
        } else {
            point["model"]["volatility"] = price["parameter"].clone();
        }
        assert_eq!(price["valuation"]["options"], options);
        for (out, expected) in [
            ("terms", &operation["input"]["bond"]["terms"]),
            ("context", &operation["input"]["context"]),
            ("discount_curve", &operation["input"]["discount"]),
        ] {
            assert_eq!(&price["valuation"][out], expected);
        }
        let mut refresh = case["request"].clone();
        refresh["operation"] = json!({"kind":"term_coupon_options","input":operation["input"]});
        refresh["operation"]["input"]["options"] = options;
        let refreshed = original::native(&refresh.to_string());
        assert_eq!(refreshed["ok"], "true");
        assert_eq!(refreshed["result"]["data"], price["valuation"]);
    }
}
pub fn run() -> Value {
    let rows:Vec<_> = cases().into_iter().map(|case| {
        let output = convex::library::execute_json(&request(&case)); check(&case, &output);
        json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&output).unwrap()})
    }).collect();
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
