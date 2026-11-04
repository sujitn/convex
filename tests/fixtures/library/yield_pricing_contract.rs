//! Full native envelopes and predeclared independent supplied-cash prices/yields.
#![allow(dead_code)]
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("yield-pricing.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
fn native(input: &str) -> Value {
    match convex::library::decode_request(input).and_then(|r| convex::library::execute(&r)) {
        Ok(result) => json!({"ok":"true", "schema_version":1, "result":result}),
        Err(error) => json!({"ok":"false", "schema_version":1, "error":error}),
    }
}
pub fn check(case: &Value, output: &str) {
    let actual: Value = serde_json::from_str(output).unwrap();
    assert_eq!(
        actual,
        native(&request(case)),
        "{} host changed native result",
        case["id"]
    );
    assert_eq!(
        actual["ok"],
        if case["expected_error"].is_null() {
            "true"
        } else {
            "false"
        },
        "{}: {actual}",
        case["id"]
    );
    if !case["expected_error"].is_null() {
        assert_eq!(
            actual["error"]["code"], case["expected_error"],
            "{}: {actual}",
            case["id"]
        );
        return;
    }
    let data = &actual["result"]["data"];
    let inverse = case["request"]["operation"]["action"]["kind"] == "solve";
    let field = if inverse {
        "yield_value"
    } else {
        "present_value"
    };
    assert_eq!(
        data["unit"],
        if inverse {
            "annual_decimal_rate"
        } else {
            "input_cash"
        }
    );
    assert_eq!(data.as_object().unwrap().len(), if inverse { 5 } else { 2 });
    let expected = case["expected"].as_str().unwrap().parse::<f64>().unwrap();
    let bound = case["absolute_bound"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!(
        (data[field].as_f64().unwrap() - expected).abs() <= bound,
        "{}: {data}",
        case["id"]
    );
    if inverse {
        assert_eq!(data["residual_unit"], "input_cash");
        assert!(
            data["residual"].as_f64().unwrap().abs()
                < case["request"]["operation"]["action"]["tolerance"]
                    .as_f64()
                    .unwrap()
        );
        assert!(
            data["iterations"].as_u64().unwrap()
                < case["request"]["operation"]["action"]["max_iterations"]
                    .as_u64()
                    .unwrap()
        );
    }
}
pub fn run() -> Value {
    let rows:Vec<_>=cases().into_iter().map(|case| {
        let output=convex::library::execute_json(&request(&case));check(&case,&output);
        json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&output).unwrap()})
    }).collect();
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
