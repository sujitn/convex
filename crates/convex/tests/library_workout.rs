//! Complete supplied workouts retain native economic and inverse diagnostics.
#[path = "../../../tests/fixtures/library/workout_contract.rs"]
mod contract;
use serde_json::{json, Value};

#[test]
fn every_complete_workout_request_and_failure_roundtrips() {
    for case in contract::cases() {
        let request = contract::request(&case);
        let typed = convex::library::decode_request(&request).unwrap();
        assert_eq!(
            serde_json::to_value(&typed).unwrap(),
            case["request"],
            "{}",
            case["id"]
        );
        let output = convex::library::execute_json(&request);
        contract::check(&case, &output);
        let output: Value = serde_json::from_str(&output).unwrap();
        if case["expected_error"].is_null() {
            let data = &output["result"]["data"];
            assert_eq!(
                data["reference"],
                case["request"]["operation"]["input"]["reference"]
            );
            assert_eq!(
                data["quote_revision"],
                case["request"]["operation"]["input"]["quote_revision"]
            );
            assert_eq!(
                data["calendars"],
                case["request"]["operation"]["input"]["bond"]["calendars"]
            );
            let q = &data["quotation"];
            let source = if q.get("selected").is_some() {
                &q["selected"]["source"]
            } else {
                &q["source"]
            };
            assert_eq!(
                source,
                &case["request"]["operation"]["input"]["bond"]["terms"]
            );
        } else {
            assert!(
                output.get("result").is_none(),
                "partial financial result leaked"
            );
        }
    }
}
#[test]
fn actual_notice_continuous_dates_delayed_cash_and_holder_put_survive() {
    let cases = contract::cases();
    let c = cases
        .iter()
        .find(|c| c["id"] == "workout.worst.calls_and_maturity")
        .unwrap();
    let output: Value =
        serde_json::from_str(&convex::library::execute_json(&contract::request(c))).unwrap();
    let q = &output["result"]["data"]["quotation"];
    assert_eq!(q["quotes"].as_array().unwrap().len(), 13);
    assert_eq!(q["examined_dates"], 14);
    assert_eq!(q["selected_index"], 1);
    assert!(!q["forced_by_notice"].as_bool().unwrap());
    assert!(
        q["quotes"][2]["decimal_yield"].as_f64().unwrap()
            < q["quotes"][1]["decimal_yield"].as_f64().unwrap()
    );
    assert_eq!(q["quotes"][4]["selection"]["date"], "2027-06-22");
    let c = cases
        .iter()
        .find(|c| c["id"] == "workout.price.delayed_claims")
        .unwrap();
    let output: Value =
        serde_json::from_str(&convex::library::execute_json(&contract::request(c))).unwrap();
    let q = &output["result"]["data"]["quotation"];
    assert_eq!(q["exercise"]["dates"]["settlement"], "2027-01-02");
    let principal = q["quotation"]["value"]["cash"]["principal"]
        .as_array()
        .unwrap();
    assert!(principal
        .iter()
        .any(|p| p["payment_date"] == "2027-01-11" && p["amount"] == "300"));
    assert_eq!(q["exercise"]["outstanding_principal"], "700");
}
#[test]
fn incomplete_cached_workout_and_numeric_contract_cash_are_rejected() {
    let base = contract::cases()
        .into_iter()
        .find(|c| c["id"] == "workout.price.SemiAnnual.0.04125.exercise.0")
        .unwrap()["request"]
        .clone();
    for (path, value) in [
        ("cached_yield", json!(0.04)),
        ("call_schedule", json!([])),
        ("notice_state", json!({"default":true})),
    ] {
        let mut request = base.clone();
        request["operation"]["input"][path] = value;
        assert!(convex::library::decode_request(&request.to_string()).is_err());
    }
    let mut request = base.clone();
    request["operation"]["input"]["bond"]["terms"]["exercise"][0]["price_per_100"] = json!(101.25);
    assert!(convex::library::decode_request(&request.to_string()).is_err());
    let mut request = base;
    request["operation"]["context"]
        .as_object_mut()
        .unwrap()
        .remove("notice");
    assert!(convex::library::decode_request(&request.to_string()).is_err());
}
