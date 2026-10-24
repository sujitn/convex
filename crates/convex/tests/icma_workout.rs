//! Full source roundtrip, independent all-path finance and pure-owner parity.
#[path = "../../../tests/fixtures/library/icma_workout_contract.rs"]
mod controls;
use convex::library::{IcmaWorkoutOperation, LibraryOperation};

#[test]
fn full_icma_workout_paths_sources_invoices_yields_and_failures() {
    for case in controls::cases() {
        let wire = controls::request(&case);
        if let Ok(decoded) = convex::library::decode_request(&wire) {
            assert_eq!(serde_json::to_value(&decoded).unwrap(), case["request"]);
        }
        controls::check(&case, &convex::library::execute_json(&wire));
    }
}

#[test]
fn every_contextual_path_is_admitted_before_any_inverse() {
    for case in controls::cases().into_iter().filter(|c| {
        c["id"].as_str().unwrap().contains("before_inverse") && c["expected_error"] != "no_root"
    }) {
        let output = convex::library::execute_json(&controls::request(&case));
        controls::check(&case, &output);
        assert!(
            !output.contains("no_root"),
            "missing/unmodeled put cash must fail before the otherwise rootless maturity inverse"
        );
    }
}

#[test]
fn pure_native_icma_workout_matches_shared_original_owner() {
    use convex::workflow as w;
    for case in controls::cases()
        .into_iter()
        .filter(|c| c["expected_error"].is_null())
    {
        let request = convex::library::decode_request(&controls::request(&case)).unwrap();
        let LibraryOperation::IcmaWorkout {
            input,
            convention,
            context,
            action,
        } = &request.operation
        else {
            panic!("wrong scope")
        };
        let bond = &input.security.bond;
        let calendar = |pin: &convex::rates::ReferencePin| {
            request
                .inputs
                .calendars
                .iter()
                .find(|c| &c.pin == pin)
                .unwrap()
        };
        let contract = bond
            .terms
            .clone()
            .build_with_calendars(&convex::bond_contracts::BondCalendars {
                payment: calendar(&bond.calendars.payment),
                reset: calendar(&bond.calendars.reset),
                observation: calendar(&bond.calendars.observation),
                exercise: calendar(&bond.calendars.exercise),
            })
            .unwrap();
        let sources = w::IcmaQuotationSources {
            security: input.security.reference.clone(),
            quote_revision: input.security.quote_revision.clone(),
            quotation_policy: input.quotation_policy.clone(),
            calendars: w::IcmaCalendarPins {
                payment: bond.calendars.payment.clone(),
                reset: bond.calendars.reset.clone(),
                observation: bond.calendars.observation.clone(),
                exercise: bond.calendars.exercise.clone(),
            },
        };
        let bound = w::IcmaQuotationInputs {
            contract: &contract,
            sources: &sources,
            reference_context: &input.reference_context,
            convention,
            position: &input.position,
            exercise_calendar: (
                &contract.terms().calendars.exercise,
                calendar(&bond.calendars.exercise),
            ),
            purchase_calendar: (
                &contract.terms().calendars.payment,
                calendar(&bond.calendars.payment),
            ),
        };
        let native = match action {
            IcmaWorkoutOperation::PricePaths {
                decimal_yield,
                settings,
            } => serde_json::to_value(
                w::quote_icma_workout_prices(&bound, context, *decimal_yield, *settings).unwrap(),
            )
            .unwrap(),
            IcmaWorkoutOperation::Worst { target, settings } => serde_json::to_value(
                w::quote_icma_workout_worst(&bound, context, *target, *settings).unwrap(),
            )
            .unwrap(),
        };
        let shared: serde_json::Value =
            serde_json::from_str(&convex::library::execute_json(&controls::request(&case)))
                .unwrap();
        assert_eq!(native, shared["result"]["data"]);
    }
}
