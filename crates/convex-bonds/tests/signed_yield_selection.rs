//! Independent algebraic roots and adverse branch-selection controls.
use convex_bonds::{
    pricing::{
        discount_time::{DatedYieldCash, DiscountTimeBasis, PreparedDatedCash},
        yield_cash::{price_cash, CashYieldCompounding},
        yield_selection::YieldBranch,
        YieldSolver,
    },
    BondError,
};
use convex_core::{daycounts::DayCountConvention, types::Date};
use serde_json::Value;

const CASH: [(f64, f64); 2] = [(1.0, 230.0), (2.0, -132.0)];

fn branch(lower: f64, upper: f64) -> YieldBranch {
    YieldBranch::new(lower, upper, 1e-11).unwrap()
}
fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}
fn dated(entries: &[(&str, &str)]) -> PreparedDatedCash {
    let cash: Vec<DatedYieldCash> = entries
        .iter()
        .map(|(date, amount)| {
            serde_json::from_value(serde_json::json!({"date":date,"amount":amount})).unwrap()
        })
        .collect();
    PreparedDatedCash::new(
        &cash,
        Date::parse("2025-01-30").unwrap(),
        &DiscountTimeBasis::Calendar {
            day_count: DayCountConvention::Thirty360E,
        },
    )
    .unwrap()
}

#[test]
fn both_roots_of_every_independent_signed_cash_case_are_selected_and_repriced() {
    let pack: Value =
        serde_json::from_str(include_str!("fixtures/signed-yield-selection.json")).unwrap();
    let rows = pack["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 510);
    let mut captured = Vec::with_capacity(rows.len());
    for row in rows {
        let cash: Vec<_> = row["cash"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| (number(&pair[0]), number(&pair[1])))
            .collect();
        let law = serde_json::from_value(row["compounding"].clone()).unwrap();
        let selected = YieldSolver::new()
            .solve_cash_selected(
                &cash,
                number(&row["target"]),
                law,
                YieldBranch::new(
                    number(&row["lower"]),
                    number(&row["upper"]),
                    number(&row["rate_bound"]),
                )
                .unwrap(),
            )
            .unwrap_or_else(|error| panic!("{}: {error:?}", row["id"]));
        let error = (selected.yield_value - number(&row["root"])).abs();
        assert!(
            error <= number(&row["rate_bound"]),
            "{}: {selected:?}",
            row["id"]
        );
        assert!(
            error <= selected.rate_error_bound,
            "{}: {selected:?}",
            row["id"]
        );
        let price = price_cash(&cash, selected.yield_value, law).unwrap();
        assert!((price - number(&row["target"])).abs() < number(&row["price_bound"]));
        assert_eq!(selected.residual, price - number(&row["target"]));
        assert!(selected.derivative_bounds[0] <= number(&row["independent_derivative_bounds"][0]));
        assert!(selected.derivative_bounds[1] >= number(&row["independent_derivative_bounds"][1]));
        assert!(selected.derivative_bounds[0] > 0.0 || selected.derivative_bounds[1] < 0.0);
        assert_eq!(selected.value_evaluations, 7 + selected.iterations);
        assert_eq!(
            selected.discount_term_evaluations,
            10 + 2 * u64::from(selected.iterations)
        );
        captured.push(serde_json::json!({"id":row["id"],"cash":cash,"compounding":row["compounding"],"target":number(&row["target"]),"selection":selected}));
    }
    println!(
        "{}",
        serde_json::json!({"kind":"complete_signed_yield_reference_results","cases":captured})
    );
}

#[test]
fn old_nonnegative_inverse_rejections_and_raw_zero_time_guards_stay() {
    let solver = YieldSolver::new();
    for seed in [0.1, 0.2] {
        assert!(solver
            .solve_cash(&CASH, 100.0, CashYieldCompounding::Annual, seed)
            .is_err());
    }
    for cash in [
        vec![(0.0, 100.0)],
        vec![(1.0, -100.0)],
        vec![(1.0, 0.0)],
        vec![],
    ] {
        assert!(solver
            .solve_cash_selected(&cash, 100.0, CashYieldCompounding::Annual, branch(0.0, 0.3))
            .is_err());
    }
    assert!(solver
        .solve_cash_selected(
            &vec![(1.0, 1.0); 4097],
            100.0,
            CashYieldCompounding::Annual,
            branch(0.0, 0.3)
        )
        .is_err());
}

#[test]
fn two_root_or_stationary_branches_are_unresolved_instead_of_globally_rootless() {
    for (cash, lower, upper) in [
        (&CASH[..], 0.0, 0.3),
        (&[(1.0, 200.0), (2.0, -100.0)][..], -0.001, 0.001),
    ] {
        assert!(matches!(
            YieldSolver::new().solve_cash_selected(
                cash,
                100.0,
                CashYieldCompounding::Annual,
                branch(lower, upper)
            ),
            Err(BondError::YieldSelectionUnresolved { .. })
        ));
    }
}

#[test]
fn no_root_is_local_to_the_selected_monotonic_branch() {
    assert!(matches!(
        YieldSolver::new().solve_cash_selected(
            &CASH,
            101.0,
            CashYieldCompounding::Annual,
            branch(0.099, 0.101)
        ),
        Err(BondError::YieldBranchNoRoot)
    ));
    assert!(YieldSolver::new()
        .solve_cash_selected(
            &CASH,
            100.0,
            CashYieldCompounding::Annual,
            branch(0.199, 0.201)
        )
        .is_ok());
}

#[test]
fn branch_validation_and_wire_decoding_have_no_defaults_or_unknown_fields() {
    for (lo, hi, tol) in [
        (0.1, 0.1, 1e-11),
        (0.2, 0.1, 1e-11),
        (f64::NAN, 0.2, 1e-11),
        (0.1, f64::INFINITY, 1e-11),
        (0.1, 0.2, 0.0),
        (0.1, 0.2, -1.0),
        (0.1, 0.2, f64::NAN),
    ] {
        assert!(YieldBranch::new(lo, hi, tol).is_err());
    }
    for input in [
        r#"{"lower":0.1,"upper":0.2}"#,
        r#"{"lower":0.1,"upper":0.2,"rate_tolerance":1e-11,"seed":0.1}"#,
        r#"{"lower":0.2,"upper":0.1,"rate_tolerance":1e-11}"#,
    ] {
        assert!(serde_json::from_str::<YieldBranch>(input).is_err());
    }
}

#[test]
fn every_discount_domain_is_admitted_before_root_selection() {
    for (law, lo, hi) in [
        (CashYieldCompounding::Annual, -1.0, 0.0),
        (CashYieldCompounding::Periodic { frequency: 2.0 }, -2.0, 0.0),
        (CashYieldCompounding::Simple, -0.5, 0.0),
        (CashYieldCompounding::Continuous, -1e308, 0.0),
    ] {
        assert!(YieldSolver::new()
            .solve_cash_selected(&CASH, 100.0, law, branch(lo, hi))
            .is_err());
    }
    assert!(YieldSolver::new()
        .solve_cash_selected(
            &CASH,
            100.0,
            CashYieldCompounding::Periodic { frequency: 0.0 },
            branch(0.099, 0.101)
        )
        .is_err());
}

#[test]
fn work_exhaustion_and_unresolvable_precision_never_return_a_seed_yield() {
    assert!(matches!(
        YieldSolver::new()
            .with_max_iterations(1)
            .solve_cash_selected(
                &CASH,
                100.0,
                CashYieldCompounding::Annual,
                branch(0.099, 0.1003)
            ),
        Err(BondError::YieldConvergenceFailed { iterations: 1 })
    ));
    assert!(matches!(
        YieldSolver::new()
            .with_tolerance(1e-20)
            .solve_cash_selected(
                &CASH,
                100.0,
                CashYieldCompounding::Annual,
                branch(0.099, 0.101)
            ),
        Err(BondError::YieldSelectionUnresolved { .. })
    ));
    assert!(YieldSolver::new()
        .solve_cash_selected(
            &CASH,
            100.0,
            CashYieldCompounding::Annual,
            YieldBranch::new(0.099, 0.101, 1e-20).unwrap()
        )
        .is_err());
    for limit in [0, 10001] {
        assert!(YieldSolver::new()
            .with_max_iterations(limit)
            .solve_cash_selected(
                &CASH,
                100.0,
                CashYieldCompounding::Annual,
                branch(0.099, 0.101)
            )
            .is_err());
    }
}

#[test]
fn cancellation_cannot_be_mistaken_for_a_resolved_derivative() {
    let cash = [(1.0, 1e16), (1.0, -1e16), (1.0, 100.0)];
    assert!(matches!(
        YieldSolver::new().solve_cash_selected(
            &cash,
            100.0,
            CashYieldCompounding::Annual,
            branch(-0.001, 0.001)
        ),
        Err(BondError::YieldSelectionUnresolved { .. })
    ));
}

#[test]
fn exact_boundary_roots_stay_inside_the_original_selected_branch() {
    for (lo, hi, root) in [
        (0.1, 0.101, 0.1),
        (0.099, 0.1, 0.1),
        (0.2, 0.201, 0.2),
        (0.199, 0.2, 0.2),
    ] {
        let target = price_cash(&CASH, root, CashYieldCompounding::Annual).unwrap();
        let result = YieldSolver::new()
            .solve_cash_selected(&CASH, target, CashYieldCompounding::Annual, branch(lo, hi))
            .unwrap();
        assert_eq!(result.yield_value, root);
        assert_eq!(result.iterations, 0);
        assert_eq!(result.residual, 0.0);
    }
}

#[test]
fn actual_future_zero_time_cash_remains_in_the_complete_selected_target() {
    let prepared = dated(&[
        ("2025-01-31", "100"),
        ("2026-01-30", "230"),
        ("2027-01-30", "-132"),
    ]);
    assert_eq!(
        prepared.discount_cash(),
        &[(0.0, 100.0), (1.0, 230.0), (2.0, -132.0)]
    );
    for root in [0.1, 0.2] {
        let result = YieldSolver::new()
            .solve_dated_cash_selected(
                &prepared,
                200.0,
                CashYieldCompounding::Annual,
                branch(root - 0.001, root + 0.001),
            )
            .unwrap();
        assert!((result.yield_value - root).abs() <= 1e-11);
        assert_eq!(
            result.residual,
            prepared
                .price(result.yield_value, CashYieldCompounding::Annual)
                .unwrap()
                - 200.0
        );
        assert!(YieldSolver::new()
            .solve_cash_selected(
                prepared.discount_cash(),
                200.0,
                CashYieldCompounding::Annual,
                branch(root - 0.001, root + 0.001)
            )
            .is_err());
    }
}

#[test]
fn actual_constant_future_cash_keeps_global_structural_outcomes() {
    let prepared = dated(&[("2025-01-31", "100")]);
    assert!(matches!(
        YieldSolver::new().solve_dated_cash_selected(
            &prepared,
            100.0,
            CashYieldCompounding::Annual,
            branch(0.0, 0.3)
        ),
        Err(BondError::YieldNonUnique)
    ));
    assert!(matches!(
        YieldSolver::new().solve_dated_cash_selected(
            &prepared,
            101.0,
            CashYieldCompounding::Annual,
            branch(0.0, 0.3)
        ),
        Err(BondError::YieldNoRoot)
    ));
}

#[test]
fn positive_cash_and_explicit_unit_scaling_use_the_same_full_price_owner() {
    let positive = [(1.0, 110.0)];
    let result = YieldSolver::new()
        .solve_cash_selected(
            &positive,
            100.0,
            CashYieldCompounding::Annual,
            branch(0.09, 0.11),
        )
        .unwrap();
    assert!((result.yield_value - 0.1).abs() <= 1e-11);
    let scaled: Vec<_> = CASH.iter().map(|(t, a)| (*t, *a * 10000.0)).collect();
    let result = YieldSolver::new()
        .with_tolerance(1e-6)
        .solve_cash_selected(
            &scaled,
            1e6,
            CashYieldCompounding::Annual,
            branch(0.099, 0.101),
        )
        .unwrap();
    assert!((result.yield_value - 0.1).abs() <= 1e-11);
    assert_eq!(
        result.residual,
        price_cash(&scaled, result.yield_value, CashYieldCompounding::Annual).unwrap() - 1e6
    );
    assert!(result.residual.abs() < 1e-6);
}

#[test]
fn actual_outward_rounding_overflow_returns_a_typed_failure() {
    // Exact native input captured before this stricter finite-after-rounding fix.
    let amount = f64::from_bits(4640529086980093452);
    let result = YieldSolver::new().solve_cash_selected(
        &[(1e306, amount)],
        amount,
        CashYieldCompounding::Continuous,
        branch(0.0, 1e-308),
    );
    assert!(
        matches!(result, Err(BondError::YieldSelectionUnresolved { .. })),
        "{result:?}"
    );
}
