#[path = "../../../tests/fixtures/library/icma_risk_contract.rs"]
mod controls;
use convex::{library::*, workflow as w};
use serde_json::Value;

#[test]
fn direct_icma_full_quotes_units_positions_and_failures_match_independent_controls() {
    let mut failed = Vec::new();
    for case in controls::cases() {
        let result = std::panic::catch_unwind(|| {
            if let Ok(request) = decode_request(&controls::request(&case)) {
                controls::exact(&serde_json::to_value(&request).unwrap(), &case["request"]);
            }
            controls::check(&case, &execute_json(&controls::request(&case)));
        });
        if result.is_err() {
            failed.push(case["id"].clone());
        }
    }
    assert!(
        failed.is_empty(),
        "Complete original failed controls: {failed:?}"
    );
}
#[test]
fn direct_native_icma_risk_is_the_same_prepared_cash_owner() {
    for case in controls::cases()
        .into_iter()
        .filter(|c| c["expected_error"].is_null())
    {
        let request = decode_request(&controls::request(&case)).unwrap();
        let LibraryOperation::IcmaYieldRisk {
            input,
            convention,
            cash,
            action,
        } = &request.operation
        else {
            panic!("owner");
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
        let contract = b
            .terms
            .clone()
            .build_with_calendars(&convex::bond_contracts::BondCalendars {
                payment: calendar(&b.calendars.payment),
                reset: calendar(&b.calendars.reset),
                observation: calendar(&b.calendars.observation),
                exercise: calendar(&b.calendars.exercise),
            })
            .unwrap();
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
            reference_context: &input.reference_context,
            convention,
            position: &input.position,
            exercise_calendar: (
                &contract.terms().calendars.exercise,
                calendar(&b.calendars.exercise),
            ),
            purchase_calendar: (
                &contract.terms().calendars.payment,
                calendar(&b.calendars.payment),
            ),
        };
        let prepared = w::PreparedIcmaQuotation::new(&bound, cash).unwrap();
        let actual = match action {
            IcmaYieldRiskOperation::YieldSensitivity { input } => {
                serde_json::to_value(prepared.yield_sensitivity(input).unwrap()).unwrap()
            }
            IcmaYieldRiskOperation::HedgeVector { input, identity } => {
                serde_json::to_value(prepared.hedge_yield_risk(input, identity).unwrap()).unwrap()
            }
        };
        let shared: Value = serde_json::from_str(&execute_json(&controls::request(&case))).unwrap();
        assert_eq!(actual, shared["result"]["data"]);
    }
}
#[test]
fn actual_icma_columns_reach_full_native_hedge_with_pending_and_fx_guards() {
    for case in controls::cases().into_iter().filter(|c| {
        c["expected_error"].is_null()
            && c["request"]["operation"]["action"]["kind"] == "hedge_vector"
    }) {
        let output = execute_json(&controls::request(&case));
        controls::check(&case, &output);
        for hedge in controls::hedge_requests(&case, &output) {
            controls::check_hedge(&hedge, &execute_json(&controls::request(&hedge)));
        }
    }
}
