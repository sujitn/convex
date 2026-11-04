//! Complete native envelopes and independently declared future-cash references.
#![allow(dead_code)]
use serde_json::{json, Value};

pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("future-zero-time.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}

pub fn request(case: &Value) -> String {
    case["request"].to_string()
}

fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}

fn bounded(actual: &Value, expected: f64, bound: f64, id: &Value) {
    let value = actual.as_f64().unwrap();
    assert!(
        value.is_finite() && (value - expected).abs() <= bound,
        "{id}: {value} vs {expected}"
    );
}

pub fn check(case: &Value, text: &str) {
    let actual: Value = serde_json::from_str(text).unwrap();
    let native = convex::library::decode_request(&request(case))
        .and_then(|request| convex::library::execute(&request));
    let envelope = match native {
        Ok(result) => json!({"ok":"true", "schema_version":1, "result":result}),
        Err(error) => json!({"ok":"false", "schema_version":1, "error":error}),
    };
    assert_eq!(actual, envelope, "{} complete native envelope", case["id"]);
    let reference = &case["expected"];
    if !case["expected_error"].is_null() {
        assert_eq!(actual["ok"], "false", "{} {actual}", case["id"]);
        assert_eq!(actual["error"]["code"], case["expected_error"]);
        let kind = match reference["structural_root_classification"]
            .as_str()
            .unwrap()
        {
            "non_unique" => "YieldNonUnique",
            "no_root" => "YieldNoRoot",
            other => panic!("Undeclared root classification {other}"),
        };
        assert_eq!(
            actual["error"]["details"]["native_kind"], kind,
            "{} {actual}",
            case["id"]
        );
        return;
    }
    assert_eq!(actual["ok"], "true", "{} {actual}", case["id"]);
    let data = &actual["result"]["data"];
    let action = &case["request"]["operation"]["action"];
    match action["kind"].as_str().unwrap() {
        "dated_risk" => {
            assert_eq!(data["unit"], "input_cash");
            assert_eq!(data["risk"].as_object().unwrap().len(), 7);
            for (field, expected) in reference["risk"].as_object().unwrap() {
                bounded(
                    &data["risk"][field],
                    number(expected),
                    number(&reference["absolute_bound"]),
                    &case["id"],
                );
            }
        }
        kind => {
            let cash = data["discount_cash"].as_array().unwrap();
            let original = reference["discount_cash"].as_array().unwrap();
            assert_eq!(cash.len(), original.len());
            for (pair, expected) in cash.iter().zip(original) {
                bounded(&pair[0], expected[0].as_f64().unwrap(), 1e-14, &case["id"]);
                assert_eq!(pair[1].as_f64().unwrap(), expected[1].as_f64().unwrap());
            }
            if kind == "dated_price" {
                assert_eq!(data["unit"], "input_cash");
                bounded(
                    &data["present_value"],
                    number(&reference["present_value"]),
                    number(&reference["price_absolute_bound"]),
                    &case["id"],
                );
            } else {
                assert_eq!(kind, "dated_solve");
                assert_eq!(data["unit"], "annual_decimal_rate");
                assert_eq!(data["residual_unit"], "input_cash");
                bounded(
                    &data["yield_value"],
                    number(&reference["annual_yield"]),
                    number(&reference["absolute_yield_bound"]),
                    &case["id"],
                );
                assert!(
                    data["residual"].as_f64().unwrap().abs() < number(&reference["residual_bound"])
                );
                assert!(
                    data["iterations"].as_u64().unwrap()
                        <= action["max_iterations"].as_u64().unwrap()
                );
            }
        }
    }
}

pub fn run() -> Value {
    let rows: Vec<_> = cases().into_iter().map(|case| {
        let text = convex::library::execute_json(&request(&case));
        check(&case, &text);
        json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],
            "expected_envelope":serde_json::from_str::<Value>(&text).unwrap()})
    }).collect();
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
