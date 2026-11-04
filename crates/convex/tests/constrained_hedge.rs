//! Independent controls, strict native/shared reachability and failed proposal guards.
#[path = "../../../tests/fixtures/library/constrained_hedge.rs"]
mod controls;
use convex::{constrained_hedge as h, library::*};
use serde_json::Value;

#[test]
fn complete_requests_results_and_typed_failures_survive_shared_json() {
    for case in controls::cases() {
        let wire = controls::request(&case);
        if case["expected_error"].is_null() {
            let input = decode_request(&wire).unwrap();
            assert_eq!(
                serde_json::to_value(&input).unwrap(),
                case["request"],
                "{}",
                case["id"]
            );
            let LibraryOperation::Hedge { problem, action } = &input.operation else {
                panic!("wrong operation")
            };
            let native = match action {
                HedgeOperation::Aggregate {} => {
                    serde_json::to_value(h::aggregate_hedge_risk(problem).unwrap()).unwrap()
                }
                HedgeOperation::Solve {} => {
                    serde_json::to_value(h::solve_hedge_proposal(problem).unwrap()).unwrap()
                }
                HedgeOperation::SolveBook { .. }
                | HedgeOperation::SolveLifecycle { .. }
                | HedgeOperation::SolveIssuerLifecycle { .. } => {
                    panic!("unexpected lifecycle operation in original hedge controls")
                }
            };
            let expected = controls::native(&wire);
            assert_eq!(native, expected["result"]["data"]);
        }
        controls::check(&case, &execute_json(&wire));
    }
}
fn compare_solution(actual: &Value, expected: &Value, bound: f64, quantity_bound: f64) {
    assert_eq!(actual.is_null(), expected.is_null());
    if actual.is_null() {
        return;
    }
    for key in [
        "quantities",
        "reference_multiples",
        "final_holdings",
        "after",
        "residual",
        "candidate_costs",
    ] {
        let a = actual[key].as_array().unwrap();
        let b = expected[key].as_array().unwrap();
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(b) {
            controls::close(
                a,
                b,
                if matches!(key, "quantities" | "final_holdings") {
                    quantity_bound
                } else {
                    bound
                },
            );
        }
    }
    for key in ["total_cost", "tracking_objective", "objective"] {
        controls::close(&actual[key], &expected[key], bound);
    }
    assert!(actual["constraints"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["satisfied"] == true));
}
#[test]
fn independent_signed_factor_quantity_cost_and_lot_controls_match() {
    let cases = controls::cases();
    for reference in controls::references() {
        let case = cases.iter().find(|c| c["id"] == reference["id"]).unwrap();
        let value = controls::native(&controls::request(case));
        assert_eq!(value["ok"], "true", "{}: {value}", case["id"]);
        let data = &value["result"]["data"];
        let expected = &reference["expected"];
        let aggregate = case["request"]["operation"]["action"]["kind"] == "aggregate";
        let exposure = if aggregate { data } else { &data["exposure"] };
        assert_eq!(exposure["problem"], case["request"]["operation"]["problem"]);
        let bound = controls::number(&reference["absolute"]);
        for key in ["current", "pending", "before"] {
            for (a, b) in exposure[key]
                .as_array()
                .unwrap()
                .iter()
                .zip(expected[key].as_array().unwrap())
            {
                controls::close(a, b, bound);
            }
        }
        if !aggregate {
            compare_solution(
                &data["continuous"],
                &expected["continuous"],
                bound,
                controls::number(&reference["quantity_absolute"]),
            );
            compare_solution(
                &data["proposal"],
                &expected["proposal"],
                bound,
                controls::number(&reference["quantity_absolute"]),
            );
            assert!(
                data["work"]["active_sets"].as_u64().unwrap()
                    + data["work"]["discrete_points"].as_u64().unwrap()
                    <= data["exposure"]["problem"]["settings"]["max_work"]
                        .as_u64()
                        .unwrap()
            );
            assert_eq!(data["rank"]["rank"], data["rank"]["columns"]);
        }
    }
}
#[test]
fn incomplete_discrete_search_never_promotes_an_incumbent() {
    let original = controls::cases()
        .into_iter()
        .find(|c| c["id"] == "hedge.lot_tie")
        .unwrap();
    let wire = controls::request(&original);
    let input = decode_request(&wire).unwrap();
    let LibraryOperation::Hedge { mut problem, .. } = input.operation else {
        panic!("wrong operation")
    };
    let mut found = false;
    for budget in 1..300 {
        problem.settings.max_work = budget;
        let report = h::solve_hedge_proposal(&problem).unwrap();
        if report.status == h::HedgeSolveStatus::WorkExhausted
            && report.work.discrete_points > 0
            && report.continuous.is_some()
        {
            assert!(report.proposal.is_none());
            assert_eq!(
                report.work.active_sets + report.work.discrete_points,
                budget
            );
            found = true;
            break;
        }
    }
    assert!(
        found,
        "must exercise actual post-continuous exhausted lot search"
    );
}

#[test]
fn empty_later_lot_axis_terminates_before_a_huge_outer_range() {
    let case = controls::cases()
        .into_iter()
        .find(|c| c["id"] == "hedge.empty_later_lot")
        .unwrap();
    let result = controls::native(&controls::request(&case));
    let data = &result["result"]["data"];
    assert_eq!(data["status"], "discrete_infeasible");
    assert_eq!(data["work"]["discrete_points"], 0);
    assert!(data["proposal"].is_null());
    assert!(!data["continuous"].is_null());
}

#[test]
fn native_lot_boundary_remainder_cannot_disappear_in_quotient_rounding() {
    let case = controls::cases()
        .into_iter()
        .find(|c| c["id"] == "hedge.fixed_trade")
        .unwrap();
    let request = decode_request(&controls::request(&case)).unwrap();
    let LibraryOperation::Hedge { mut problem, .. } = request.operation else {
        panic!("wrong operation")
    };
    problem.candidates[0].trade_lower =
        serde_json::from_str("\"3.0000000000000000000000000001\"").unwrap();
    problem.candidates[0].trade_upper = serde_json::from_str("\"6\"").unwrap();
    problem.candidates[0].lot = Some(serde_json::from_str("\"3\"").unwrap());
    let result = h::solve_hedge_proposal(&problem).unwrap();
    assert_eq!(result.status, h::HedgeSolveStatus::Feasible);
    let proposal = result.proposal.unwrap();
    assert_eq!(
        proposal.quantities[0].decimal(),
        rust_decimal::Decimal::from(6)
    );
    assert!(proposal.constraints.iter().all(|c| c.satisfied));
}

#[test]
fn missing_nullable_terms_cannot_choose_implicit_solver_economics() {
    let case = controls::cases()
        .into_iter()
        .find(|c| c["id"] == "hedge.mixed")
        .unwrap();
    for (field, path) in [
        ("fx", vec!["candidates", "0", "risk"]),
        ("lot", vec!["candidates", "0"]),
        ("credit", vec!["factors", "0"]),
    ] {
        let mut request = case["request"].clone();
        let mut owner = &mut request["operation"]["problem"];
        for part in path {
            owner = if part == "0" {
                &mut owner[0]
            } else {
                &mut owner[part]
            };
        }
        owner.as_object_mut().unwrap().remove(field);
        let error = decode_request(&request.to_string()).unwrap_err();
        assert_eq!(
            error.code,
            LibraryErrorCode::InvalidInput,
            "missing {field}"
        );
    }
}

#[test]
fn nonzero_native_quantity_resolution_failure_cannot_become_zero() {
    let case = controls::cases()
        .into_iter()
        .find(|c| c["id"] == "hedge.zero_budget")
        .unwrap();
    let request = decode_request(&controls::request(&case)).unwrap();
    let LibraryOperation::Hedge { mut problem, .. } = request.operation else {
        panic!("wrong operation")
    };
    problem.factors[0].target = 1e-30;
    let result = h::solve_hedge_proposal(&problem).unwrap();
    assert_eq!(result.status, h::HedgeSolveStatus::NumericalFailure);
    assert!(result.proposal.is_none());
}
