#[path = "../../../tests/fixtures/library/icma_helpers_contract.rs"]
mod controls;
use convex::{cash_calibration as c, library::*};
use serde_json::{json, Value};

#[test]
fn contextual_helpers_preserve_full_cash_quotes_fits_risk_and_failures() {
    let mut failed = Vec::new();
    for case in controls::cases() {
        let checked = std::panic::catch_unwind(|| {
            let wire = controls::request(&case);
            if let Ok(decoded) = decode_request(&wire) {
                controls::exact(
                    &serde_json::to_value(decoded).unwrap(),
                    &case["request"],
                    case["id"].as_str().unwrap(),
                );
            }
            controls::check(&case, &execute_json(&wire));
        });
        if checked.is_err() {
            failed.push(case["id"].clone());
        }
    }
    assert!(
        failed.is_empty(),
        "Failed original complete cases: {failed:?}"
    );
}
#[test]
fn native_contextual_helper_conversion_requires_actual_independent_pins() {
    for case in controls::cases().into_iter().filter(|c| {
        c["expected_error"].is_null()
            && c["request"]["operation"]["kind"] == "helper"
            && c["request"]["operation"]["input"]["kind"] == "cash_quote"
    }) {
        let request = decode_request(&controls::request(&case)).unwrap();
        let LibraryOperation::Helper { input } = &request.operation else {
            panic!("wrong owner")
        };
        let HelperInput::CashQuote {
            helper,
            calendars,
            reference_date,
        } = input.as_ref()
        else {
            panic!("wrong helper")
        };
        let calendar = |pin: &convex::rates::ReferencePin| {
            request
                .inputs
                .calendars
                .iter()
                .find(|c| &c.pin == pin)
                .unwrap()
        };
        let actual = convex::workflow::IcmaCalendarPins {
            payment: calendars.payment.clone(),
            reset: calendars.reset.clone(),
            observation: calendars.observation.clone(),
            exercise: calendars.exercise.clone(),
        };
        let resource = c::CashHelperResources::IcmaBond {
            ids: &helper.terms.calendars,
            calendars: convex::bond_contracts::BondCalendars {
                payment: calendar(&calendars.payment),
                reset: calendar(&calendars.reset),
                observation: calendar(&calendars.observation),
                exercise: calendar(&calendars.exercise),
            },
            pins: actual,
        };
        let native = c::convert_cash_quote(helper, &resource, *reference_date).unwrap();
        let shared: Value = serde_json::from_str(&execute_json(&controls::request(&case))).unwrap();
        assert_eq!(
            serde_json::to_value(native).unwrap(),
            shared["result"]["data"]
        );
        let unversioned = c::CashHelperResources::Bond {
            ids: &helper.terms.calendars,
            calendars: convex::bond_contracts::BondCalendars {
                payment: calendar(&calendars.payment),
                reset: calendar(&calendars.reset),
                observation: calendar(&calendars.observation),
                exercise: calendar(&calendars.exercise),
            },
        };
        assert!(c::convert_cash_quote(helper, &unversioned, *reference_date).is_err());
    }
}
#[test]
fn contextual_graph_recipes_and_risk_rebuild_after_lossless_json_replay() {
    for case in controls::cases().into_iter().filter(|c| {
        c["expected_error"].is_null() && c["request"]["operation"]["kind"] == "calibration"
    }) {
        let mut inputs = case["request"]["inputs"].clone();
        inputs["calibration"] = case["request"]["operation"]["input"].clone();
        let parsed = decode_calibration_inputs(&inputs).unwrap();
        let rebuilt = CalibrationReplay::build(&parsed).unwrap();
        controls::exact(
            &serde_json::to_value(rebuilt.inputs()).unwrap(),
            &inputs,
            case["id"].as_str().unwrap(),
        );
        let action =
            decode_calibration_action(&parsed, &case["request"]["operation"]["action"].to_string())
                .unwrap();
        let data = rebuilt.evaluate(&action).unwrap();
        controls::check(
            &case,
            &json!({"ok":"true","result":{"data":data}}).to_string(),
        );
        assert_eq!(
            rebuilt.diagnostics()["calibrations"]
                .as_array()
                .unwrap()
                .len(),
            case["request"]["operation"]["input"]["cash"]
                .as_array()
                .unwrap()
                .len()
        );
    }
}
