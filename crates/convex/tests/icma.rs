//! Full original contextual ICMA native/shared control and failure tests.
#[path = "../../../tests/fixtures/library/icma_contract.rs"]
mod controls;
#[test]
fn original_full_icma_terms_price_inverse_cash_times_invoice_and_failures() {
    for case in controls::cases() {
        let wire = controls::request(&case);
        let decoded = convex::library::decode_request(&wire).unwrap();
        assert_eq!(
            serde_json::to_value(&decoded).unwrap(),
            case["request"],
            "full original term roundtrip"
        );
        controls::check(&case, &convex::library::execute_json(&wire));
    }
}
#[test]
fn native_prepared_icma_cash_is_the_shared_owner() {
    use convex::library::{IcmaQuotationOperation as A, LibraryOperation};
    use convex::workflow as w;
    for case in controls::cases()
        .into_iter()
        .filter(|c| c["expected_error"].is_null())
    {
        let request = convex::library::decode_request(&controls::request(&case)).unwrap();
        let LibraryOperation::IcmaQuotation {
            input,
            convention,
            cash,
            action,
        } = &request.operation
        else {
            panic!("wrong scope")
        };
        let b = &input.security.bond;
        let calendar = |pin: &convex::rates::ReferencePin| {
            request
                .inputs
                .calendars
                .iter()
                .find(|c| &c.pin == pin)
                .unwrap()
        };
        let calendars = convex::bond_contracts::BondCalendars {
            payment: calendar(&b.calendars.payment),
            reset: calendar(&b.calendars.reset),
            observation: calendar(&b.calendars.observation),
            exercise: calendar(&b.calendars.exercise),
        };
        let contract = b.terms.clone().build_with_calendars(&calendars).unwrap();
        let sources = w::IcmaQuotationSources {
            security: input.security.reference.clone(),
            quote_revision: input.security.quote_revision.clone(),
            quotation_policy: input.quotation_policy.clone(),
            calendars: w::IcmaCalendarPins {
                payment: b.calendars.payment.clone(),
                reset: b.calendars.reset.clone(),
                observation: b.calendars.observation.clone(),
                exercise: b.calendars.exercise.clone(),
            },
        };
        let bound = w::IcmaQuotationInputs {
            contract: &contract,
            sources: &sources,
            convention,
            reference_context: &input.reference_context,
            position: &input.position,
            exercise_calendar: (&contract.terms().calendars.exercise, calendars.exercise),
            purchase_calendar: (&contract.terms().calendars.payment, calendars.payment),
        };
        let prepared = w::PreparedIcmaQuotation::new(&bound, cash).unwrap();
        let actual = match action {
            A::Price { decimal_yield } => prepared.quote_price(*decimal_yield),
            A::Yield { target, settings } => prepared.quote_yield(*target, *settings),
        }
        .unwrap();
        let shared: serde_json::Value =
            serde_json::from_str(&convex::library::execute_json(&controls::request(&case)))
                .unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            shared["result"]["data"]
        );
    }
}
#[test]
fn unsupported_or_absent_context_never_routes_to_an_approximate_convention() {
    let original = controls::cases()[0]["request"].clone();
    for path in ["reference_context", "quotation_policy"] {
        let mut r = original.clone();
        r["operation"]["input"]
            .as_object_mut()
            .unwrap()
            .remove(path);
        assert!(convex::library::decode_request(&r.to_string()).is_err());
    }
    let mut r = original.clone();
    r["operation"]["convention"]["compounding"] = serde_json::json!("Street");
    assert!(convex::library::decode_request(&r.to_string()).is_err());
    let mut r = original;
    r["operation"]["action"]["kind"] = serde_json::json!("oas");
    assert!(convex::library::decode_request(&r.to_string()).is_err());
}
