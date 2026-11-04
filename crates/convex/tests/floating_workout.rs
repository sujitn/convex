//! Full floating exercise cash, unchanged native inverse guards and shared parity.
#[path = "../../../tests/fixtures/library/floating_workout_contract.rs"]
mod contract;
use serde_json::Value;
#[test]
fn direct_pure_floating_owners_agree_with_complete_shared_results() {
    use convex::library::{FloatingWorkoutOperation as A, LibraryOperation};
    use convex::workflow as w;
    for c in contract::cases()
        .into_iter()
        .filter(|c| c["expected_error"].is_null())
    {
        let request = convex::library::decode_request(&contract::request(&c)).unwrap();
        let LibraryOperation::FloatingWorkout {
            input,
            context,
            action,
        } = &request.operation
        else {
            panic!("wrong corpus operation");
        };
        let b = &input.security.bond;
        let calendar = |pin: &convex::rates::ReferencePin| {
            request
                .inputs
                .calendars
                .iter()
                .find(|cal| &cal.pin == pin)
                .unwrap()
        };
        let calendars = convex::bond_contracts::BondCalendars {
            payment: calendar(&b.calendars.payment),
            reset: calendar(&b.calendars.reset),
            observation: calendar(&b.calendars.observation),
            exercise: calendar(&b.calendars.exercise),
        };
        let bond = b.terms.clone().build_with_calendars(&calendars).unwrap();
        let curves = convex::curve_contracts::build_curve_set(
            &request.inputs.curves,
            &convex::curve_contracts::CurveSet::new(),
        )
        .unwrap()
        .curves;
        let history = convex::fixings::FixingSeries::new(request.inputs.fixings.clone()).unwrap();
        let publications =
            w::PublicationSchedule::new(request.inputs.publications.clone()).unwrap();
        let bound = w::FloatingWorkoutInputs {
            workout: w::WorkoutInputs {
                contract: &bond,
                context,
                exercise_calendar: (&bond.terms().calendars.exercise, calendars.exercise),
                purchase_calendar: (&bond.terms().calendars.payment, calendars.payment),
            },
            projection_context: &input.projection_context,
            market: w::SuppliedMarket {
                curves: &curves,
                fixings: &history,
                publications: &publications,
            },
            projection: &input.projection,
            discount: &input.discount,
            position: &input.position,
        };
        let pure = match action {
            A::Price {
                selection,
                decimal_dm,
            } => serde_json::to_value(
                w::quote_floating_workout_price(&bound, selection, *decimal_dm).unwrap(),
            )
            .unwrap(),
            A::DiscountMargin {
                selection,
                target,
                settings,
            } => serde_json::to_value(
                w::quote_floating_workout_dm(&bound, selection, *target, *settings).unwrap(),
            )
            .unwrap(),
            A::Paths {
                target,
                settings,
                shifted_prefixes,
            } => serde_json::to_value(
                w::quote_floating_workout_paths(&bound, *target, *settings, shifted_prefixes)
                    .unwrap(),
            )
            .unwrap(),
        };
        let shared: Value =
            serde_json::from_str(&convex::library::execute_json(&contract::request(&c))).unwrap();
        assert_eq!(pure, shared["result"]["data"]["quotation"], "{}", c["id"]);
    }
}
#[test]
fn original_complete_floating_requests_and_failures_reach_shared_dispatch() {
    for c in contract::cases() {
        let wire = contract::request(&c);
        let typed = convex::library::decode_request(&wire).unwrap();
        let mut original = c["request"].clone();
        // f64 target values serialize integral JSON numbers as 200.0. Preserve
        // the original numerical value while checking every economic field.
        if let Some(v) = original.pointer_mut("/operation/action/target/value") {
            *v = serde_json::json!(v.as_f64().unwrap());
        }
        assert_eq!(
            serde_json::to_value(typed).unwrap(),
            original,
            "{}",
            c["id"]
        );
        let output = convex::library::execute_json(&wire);
        contract::check(&c, &output);
        let output: Value = serde_json::from_str(&output).unwrap();
        if c["expected_error"].is_null() {
            let data = &output["result"]["data"];
            let input = &c["request"]["operation"]["input"];
            assert_eq!(data["reference"], input["security"]["reference"]);
            assert_eq!(data["quote_revision"], input["security"]["quote_revision"]);
            assert_eq!(data["quotation_policy"], input["quotation_policy"]);
            assert_eq!(data["calendars"], input["security"]["bond"]["calendars"]);
            assert_eq!(
                data["quotation"]["source"]["terms"],
                input["security"]["bond"]["terms"]
            );
            assert_eq!(
                data["quotation"]["source"]["projection_context"],
                input["projection_context"]
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
fn independently_declared_full_cash_price_dm_and_invoice_controls() {
    for c in contract::cases() {
        let output: Value =
            serde_json::from_str(&convex::library::execute_json(&contract::request(&c))).unwrap();
        contract::assert_financial(&c, &output);
    }
}
#[test]
fn partial_shifted_choice_must_be_explicit_and_unknown_optional_terms_fail() {
    let original = contract::cases()
        .into_iter()
        .find(|c| c["id"] == "floating.shifted.price")
        .unwrap()["request"]
        .clone();
    let mut request = original.clone();
    request["operation"]["action"]["selection"]
        .as_object_mut()
        .unwrap()
        .remove("weight_end");
    assert!(convex::library::decode_request(&request.to_string()).is_err());
    for name in ["cap", "floor", "make_whole", "trigger"] {
        let mut request = original.clone();
        request["operation"]["input"]["security"]["bond"]["terms"]["coupon"][name] =
            serde_json::json!(0.05);
        assert!(convex::library::decode_request(&request.to_string()).is_err());
    }
    let mut request = original;
    request["operation"]["action"]["kind"] = serde_json::json!("worst");
    assert!(convex::library::decode_request(&request.to_string()).is_err());
}
