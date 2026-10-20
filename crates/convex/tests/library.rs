//! Full economic reconstruction, strict ingress and pure dispatch parity.
#[path = "../../../tests/fixtures/library/contract.rs"]
mod contract;
use convex::library::*;
use serde_json::{json, Value};
#[test]
fn complete_requests_preserve_every_economic_field_and_execute_native_guards() {
    for case in contract::cases() {
        let input = contract::request(&case);
        let decoded = decode_request(&input).unwrap();
        assert_eq!(
            serde_json::to_value(&decoded).unwrap(),
            case["request"],
            "{}",
            case["id"]
        );
        contract::check(&case, &execute_json(&input));
    }
}
#[test]
fn exact_cash_and_unknown_nested_terms_cannot_be_rounded_or_discarded() {
    let case = contract::cases()
        .into_iter()
        .find(|c| c["id"] == "bond.construct")
        .unwrap();
    let mut request = case["request"].clone();
    request["operation"]["bond"]["terms"]["face_value"] = json!("99999999999999999.00000000001");
    let decoded = decode_request(&request.to_string()).unwrap();
    assert_eq!(
        serde_json::to_value(decoded).unwrap()["operation"]["bond"]["terms"]["face_value"],
        request["operation"]["bond"]["terms"]["face_value"]
    );
    for invalid in [
        serde_json::from_str::<Value>("12345678.123456789").unwrap(),
        json!("1.12345678901234567890123456789"),
    ] {
        request["operation"]["bond"]["terms"]["face_value"] = invalid;
        assert_eq!(
            decode_request(&request.to_string()).unwrap_err().code,
            LibraryErrorCode::InvalidInput
        );
    }
    request = case["request"].clone();
    request["operation"]["bond"]["terms"]["dates"]["invented_offset"] = json!(2);
    assert_eq!(
        decode_request(&request.to_string()).unwrap_err().code,
        LibraryErrorCode::InvalidInput
    );
    let duplicate = contract::request(&case).replacen(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
        1,
    );
    assert_eq!(
        decode_request(&duplicate).unwrap_err().code,
        LibraryErrorCode::InvalidInput
    );
    assert!(decode_request(&(contract::request(&case) + " true")).is_err());
}
#[test]
fn bounds_and_missing_data_have_typed_failures_and_no_partial_result() {
    let cases = contract::cases();
    let case = cases.iter().find(|c| c["id"] == "bond.construct").unwrap();
    let mut request = case["request"].clone();
    request["schema_version"] = json!(99);
    let result: Value = serde_json::from_str(&execute_json(&request.to_string())).unwrap();
    assert_eq!(result["error"]["code"], "invalid_input");
    assert!(result.get("result").is_none());
    assert_eq!(
        decode_request(&" ".repeat(2 * 1024 * 1024 + 1))
            .unwrap_err()
            .code,
        LibraryErrorCode::ResourceLimit
    );
    assert_eq!(
        decode_request(&format!("{}0{}", "[".repeat(66), "]".repeat(66)))
            .unwrap_err()
            .code,
        LibraryErrorCode::ResourceLimit
    );
    let pending = cases
        .iter()
        .find(|c| c["id"] == "credit.jtd_pending")
        .unwrap();
    assert!(
        contract::native(&contract::request(pending))["result"]["data"]
            .to_string()
            .contains("Pending")
    );
    let cds = cases.iter().find(|c| c["id"] == "cds.price").unwrap();
    let output = contract::native(&contract::request(cds));
    assert!(output["result"]["market_calibration"]["repricing"]
        .as_array()
        .is_some_and(|q| !q.is_empty()));
}
#[test]
#[cfg(feature = "schemars")]
fn actual_schema_contains_complete_profiles_models_histories_and_operations() {
    let schema = request_schema();
    let text = schema.to_string();
    for term in [
        "BondTerms",
        "BondProfiles",
        "reset_adjustment",
        "projection_day_count",
        "ExerciseTerms",
        "principal_prices",
        "CdsTerms",
        "SwapTerms",
        "CurveRecipe",
        "GlobalFitSettings",
        "SigmaCalibrationSettings",
        "FixingObservation",
        "LifecycleEvent",
    ] {
        assert!(text.contains(term), "missing {term}");
    }
    assert_eq!(schema["additionalProperties"], false);
}
