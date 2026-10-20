#[path = "../../../tests/fixtures/library/calibration_replay.rs"]
mod fixture;
use convex::library::*;
use serde_json::{json, Value};

#[test]
fn original_complete_calibration_operations_keep_native_results_and_failures() {
    let mut successes = 0;
    let mut failures = 0;
    for case in fixture::original_cases() {
        let original = decode_request(&case["request"].to_string()).unwrap();
        let LibraryOperation::Calibration { action, .. } = &original.operation else {
            panic!("wrong original owner")
        };
        let input = decode_calibration_inputs(&fixture::original_inputs(&case["request"])).unwrap();
        let result = CalibrationReplay::build(&input).and_then(|built| built.evaluate(action));
        let before = execute(&original).map(|v| v["data"].clone());
        assert_eq!(result, before, "{}", case["id"]);
        if result.is_ok() {
            successes += 1
        } else {
            failures += 1
        }
    }
    assert!(successes > 0 && failures > 0);
}
#[test]
fn strict_original_input_and_product_action_admission_remains_lossless() {
    let inputs = fixture::runtime_inputs();
    let parsed = decode_calibration_inputs(&inputs).unwrap();
    let built = CalibrationReplay::build(&parsed).unwrap();
    assert_eq!(
        built.diagnostics()["calibrations"][0]["recipe"]["helpers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        built.diagnostics()["calibrations"][0]["repriced_helpers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        built.diagnostics()["calibrations"][0]["solver"]["roots"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for action in [
        r#"{"kind":"build","ignored_coupon":"0.1"}"#,
        r#"{"kind":"build","kind":"build"}"#,
    ] {
        assert!(decode_calibration_action(&parsed, action).is_err());
    }
    let mut unknown = inputs.clone();
    unknown["calibration"]["rates"][0]["recipe"]["helpers"][0]["terms"]["terms"]["unused_coupon"] =
        json!("0.1");
    assert!(decode_calibration_inputs(&unknown).is_err());
    let mut incomplete = inputs.clone();
    incomplete["calibration"] = Value::Null;
    assert!(decode_calibration_inputs(&incomplete).is_err());
    incomplete = inputs;
    incomplete["calibration"]["rates"] = json!([]);
    assert!(fixture::pure(&incomplete).is_err());
}
