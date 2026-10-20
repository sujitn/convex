//! Actual contextual government composition and independent full financial controls.
#[path = "../../../tests/fixtures/library/icma_government_contract.rs"]
mod controls;
#[test]
fn full_original_contextual_government_quotes_inverses_sources_and_failures() {
    for case in controls::cases() {
        let wire = controls::request(&case);
        let decoded = convex::library::decode_request(&wire).unwrap();
        let mut original = case["request"].clone();
        let settings = &mut original["operation"]["settings"];
        let common = if settings.get("government").is_some() {
            &mut settings["government"]
        } else {
            settings
        };
        // These two declared negative domains use JSON integer1 for a native
        // f64 tolerance. Normalize only that exact, lossless1 ->1.0 spelling.
        if common["inverse"]["price_tolerance"].as_i64() == Some(1) {
            common["inverse"]["price_tolerance"] = serde_json::json!(1.0);
        }
        assert_eq!(
            serde_json::to_value(&decoded).unwrap(),
            original,
            "full source roundtrip {}",
            case["id"]
        );
        controls::check(&case, &convex::library::execute_json(&wire));
    }
}
#[test]
fn all_original_sources_are_admitted_before_any_inverse() {
    let cases = controls::cases();
    let mut r = cases
        .iter()
        .find(|c| c["id"] == "icma_gov.regular.LinearMaturity.spread")
        .unwrap()["request"]
        .clone();
    r["operation"]["target"]["helper"]["quote"]["value"] = serde_json::json!(10000.0);
    r["operation"]["benchmarks"][0]["icma"]["reference_context"]["periods"] = serde_json::json!([]);
    let v: serde_json::Value =
        serde_json::from_str(&convex::library::execute_json(&r.to_string())).unwrap();
    assert_eq!(v["error"]["code"], "invalid_input");
    assert!(v.get("result").is_none());
}
#[test]
fn absent_context_or_common_policy_cannot_be_replaced_by_a_proxy() {
    let original = controls::cases()[0]["request"].clone();
    for field in [
        "reference_context",
        "convention",
        "quotation_policy",
        "calendars",
        "position",
    ] {
        let mut r = original.clone();
        r["operation"]["target"]["icma"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(convex::library::decode_request(&r.to_string()).is_err());
    }
    let mut r = original.clone();
    r["operation"]["settings"]["compounding"] = serde_json::json!("Street");
    assert!(convex::library::decode_request(&r.to_string()).is_err());
    let mut r = original;
    r["operation"]["settings"]["government_curve"] = serde_json::json!({});
    assert!(convex::library::decode_request(&r.to_string()).is_err());
}
#[test]
fn pure_native_composition_is_the_shared_owner_without_provider_or_runtime() {
    use convex::library::{LibraryOperation, LibraryRequest};
    use convex::{cash_calibration as c, cash_quotation as q};
    fn resources<'a>(
        terms: &'a convex::bond_contracts::BondTerms,
        pins: &convex::workflow::IcmaCalendarPins,
        request: &'a LibraryRequest,
    ) -> c::CashHelperResources<'a> {
        let calendar = |p: &convex::rates::ReferencePin| {
            request
                .inputs
                .calendars
                .iter()
                .find(|c| &c.pin == p)
                .unwrap()
        };
        c::CashHelperResources::Bond {
            ids: &terms.calendars,
            calendars: convex::bond_contracts::BondCalendars {
                payment: calendar(&pins.payment),
                reset: calendar(&pins.reset),
                observation: calendar(&pins.observation),
                exercise: calendar(&pins.exercise),
            },
        }
    }
    for case in controls::cases()
        .into_iter()
        .filter(|c| c["expected_error"].is_null())
    {
        let request = convex::library::decode_request(&controls::request(&case)).unwrap();
        let (benchmarks, reference) = match &request.operation {
            LibraryOperation::IcmaGovernmentSpread {
                benchmarks,
                reference_date,
                ..
            }
            | LibraryOperation::IcmaGovernmentPrice {
                benchmarks,
                reference_date,
                ..
            } => (benchmarks, *reference_date),
            _ => panic!("wrong scope"),
        };
        let bound: Vec<_> = benchmarks
            .iter()
            .map(|b| resources(&b.quote.helper.terms, &b.icma.calendars, &request))
            .collect();
        let inputs: Vec<_> = benchmarks
            .iter()
            .zip(&bound)
            .map(|(quote, resources)| q::IcmaGovernmentSecurityInputs { quote, resources })
            .collect();
        let actual = match &request.operation {
            LibraryOperation::IcmaGovernmentSpread {
                target, settings, ..
            } => {
                let r = resources(&target.helper.terms, &target.icma.calendars, &request);
                serde_json::to_value(
                    q::icma_government_spread(target, &r, &inputs, reference, settings).unwrap(),
                )
                .unwrap()
            }
            LibraryOperation::IcmaGovernmentPrice {
                target,
                settings,
                quote,
                ..
            } => {
                let r = resources(&target.target.terms, &target.icma.calendars, &request);
                serde_json::to_value(
                    q::price_icma_government_spread(
                        target, &r, &inputs, reference, settings, quote,
                    )
                    .unwrap(),
                )
                .unwrap()
            }
            _ => panic!("wrong scope"),
        };
        let shared: serde_json::Value =
            serde_json::from_str(&convex::library::execute_json(&controls::request(&case)))
                .unwrap();
        assert_eq!(actual, shared["result"]["data"]);
    }
}
