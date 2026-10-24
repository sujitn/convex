//! Actual full native envelopes and independently declared cash-account values.
#![allow(dead_code)]
use rust_decimal::Decimal;
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("reinvestment.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    case["request"].to_string()
}
fn native(input: &str) -> Value {
    match convex::library::decode_request(input).and_then(|r| convex::library::execute(&r)) {
        Ok(result) => json!({"ok":"true","schema_version":1,"result":result}),
        Err(error) => json!({"ok":"false","schema_version":1,"error":error}),
    }
}
fn number(value: &Value) -> Decimal {
    value.as_str().unwrap().parse().unwrap()
}
pub fn check(case: &Value, output: &str) {
    let actual: Value = serde_json::from_str(output).unwrap();
    assert_eq!(
        actual,
        native(&request(case)),
        "{} host changed native envelope",
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
    let expected = &case["expected"];
    match case["request"]["operation"]["action"]["kind"]
        .as_str()
        .unwrap()
    {
        "project" => {
            assert_eq!(data["cash_unit"], "input_cash");
            assert_eq!(data["return_unit"], "annual_decimal_rate");
            let p = &data["projection"];
            assert_eq!(p["horizon"], expected["horizon"]);
            for key in [
                "residual_balance",
                "reinvested_cash",
                "horizon_value",
                "annualized_return",
            ] {
                let bound = number(
                    &expected[if key == "annualized_return" {
                        "rate_absolute_bound"
                    } else {
                        "cash_absolute_bound"
                    }],
                );
                assert!(
                    (number(&p[key]) - number(&expected["comparison"][key])).abs() <= bound,
                    "{} {key}: {p}",
                    case["id"]
                );
            }
        }
        "growth" => {
            assert_eq!(data["unit"], "compound_factor");
            assert!(
                (number(&data["growth_factor"]) - number(&expected["growth_factor"])).abs()
                    <= number(&expected["absolute_bound"]),
                "{}: {data}",
                case["id"]
            );
        }
        "classify" => {
            let a = &case["request"]["operation"]["action"];
            for key in ["money_market_threshold", "short_date_threshold", "enabled"] {
                assert_eq!(data[key], a[key]);
            }
            for key in ["money_market_selected", "short_date_selected"] {
                assert_eq!(data[key], expected[key]);
            }
        }
        "remaining_years" => {
            assert_eq!(data["unit"], "actual365f_years");
            assert!(
                (data["remaining_years"].as_f64().unwrap()
                    - expected["remaining_years"].as_f64().unwrap())
                .abs()
                    <= expected["absolute_bound"].as_f64().unwrap()
            );
        }
        _ => panic!("undeclared financial operation"),
    }
}
pub fn run() -> Value {
    let rows:Vec<_>=cases().into_iter().map(|case| {
        let output=convex::library::execute_json(&request(&case));check(&case,&output);
        json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&output).unwrap()})
    }).collect();
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
