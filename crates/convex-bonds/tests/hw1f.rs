use convex_bonds::options::hw1f::*;
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/hw1f-options.json")).unwrap()
}
fn parameter(a: f64, s: f64) -> Hw1fParameters {
    Hw1fParameters {
        mean_reversion: a,
        volatility: s,
    }
}

#[test]
fn european_options_match_independent_formula_and_put_call_parity() {
    let f = fixture();
    for c in f["cases"].as_array().unwrap() {
        let params = serde_json::from_value(c["parameters"].clone()).unwrap();
        let side = serde_json::from_value(c["side"].clone()).unwrap();
        let get = |k: &str| c[k].as_f64().unwrap();
        let price = bond_option(
            params,
            get("expiry"),
            get("maturity"),
            get("expiry_df"),
            get("maturity_df"),
            get("strike"),
            side,
        )
        .unwrap();
        let expected = get("expected_per_unit");
        assert!(
            (price - expected).abs() <= 2e-7 + 1e-11 * expected.abs(),
            "{}: actual={price} expected={expected}",
            c["id"]
        );
        let call = bond_option(
            params,
            get("expiry"),
            get("maturity"),
            get("expiry_df"),
            get("maturity_df"),
            get("strike"),
            OptionSide::Call,
        )
        .unwrap();
        let put = bond_option(
            params,
            get("expiry"),
            get("maturity"),
            get("expiry_df"),
            get("maturity_df"),
            get("strike"),
            OptionSide::Put,
        )
        .unwrap();
        assert!(
            (call - put - (get("maturity_df") - get("strike") * get("expiry_df"))).abs() < 1e-13
        );
    }
}
#[test]
fn event_grid_preserves_close_dates_and_rejects_invalid_budgets() {
    let times = event_grid(&[0.0, 0.4, 0.40000001, 0.7, 1.0], 16).unwrap();
    for t in [0.4, 0.40000001, 0.7, 1.0] {
        assert!(times.binary_search_by(|x| x.total_cmp(&t)).is_ok());
    }
    for bad in [
        vec![],
        vec![f64::NAN],
        vec![-0.1, 1.0],
        vec![151.0],
        vec![0.0],
    ] {
        assert!(event_grid(&bad, 16).is_err());
    }
    assert!(event_grid(&[1.0], 0).is_err());
    assert!(event_grid(&[1.0], MAX_TREE_STEPS + 1).is_err());
}
#[test]
fn supplied_curve_fit_and_cash_rollback_hold_across_uneven_negative_rate_grids() {
    let mut seed = 0x59_u64;
    for case in 0..128 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let event = 0.1 + (seed % 700) as f64 / 1000.0;
        let times = event_grid(&[event, 1.0, 2.5], 12 + (case % 12)).unwrap();
        let rate = -0.02 + case as f64 * 0.0003;
        let discounts: Vec<_> = times
            .iter()
            .map(|t| (-rate * t - 0.001 * t * t).exp())
            .collect();
        let tree = Hw1fTree::build(
            parameter(
                if case % 3 == 0 { 0.0 } else { 0.03 },
                if case % 5 == 0 { 0.0 } else { 0.012 },
            ),
            &times,
            &discounts,
        )
        .unwrap();
        assert!(tree.diagnostics().minimum_probability >= 0.0);
        assert!(tree.diagnostics().maximum_discount_residual < 1e-11);
        for (i, &df) in discounts.iter().enumerate() {
            let value = tree.cash_value_at(0, &[(i, 100.0)], 0.00123).unwrap()[0];
            assert!((value - 100.0 * df * (-0.00123 * times[i]).exp()).abs() < 1e-10);
        }
    }
}
#[test]
fn checked_tree_has_finite_dimension_probability_and_work_failures() {
    for params in [
        parameter(-0.01, 0.01),
        parameter(0.03, -0.01),
        parameter(f64::NAN, 0.01),
        parameter(0.03, 1.01),
    ] {
        assert!(Hw1fTree::build(params, &[0.0, 1.0], &[1.0, 0.95]).is_err());
    }
    for (times, dfs) in [
        (vec![0.0, 1.0, 1.0], vec![1.0, 0.98, 0.95]),
        (vec![0.1, 1.0], vec![1.0, 0.95]),
        (vec![0.0, 1.0], vec![1.0, 0.0]),
        (vec![0.0, 1.0], vec![1.0, f64::INFINITY]),
    ] {
        assert!(Hw1fTree::build(parameter(0.03, 0.01), &times, &dfs).is_err());
    }
    let tree = Hw1fTree::build(parameter(0.03, 0.01), &[0.0, 1.0], &[1.0, 0.98]).unwrap();
    assert!(tree.rollback(0, &[], 0.0).is_err());
    assert!(tree.rollback(0, &[f64::NAN; 3], 0.0).is_err());
    assert!(tree.cash_value_at(1, &[(0, 1.0)], 0.0).is_err());
    assert!(tree.cash_value_at(0, &[(2, 1.0)], 0.0).is_err());
    assert!(tree.layer_at(0.5).is_err());
    // A near-coincident event expands the next spatial row; reject before allocation.
    assert!(Hw1fTree::build(
        parameter(0.0, 0.01),
        &[0.0, 1.0, 1.0 + 1e-12],
        &[1.0, 0.95, 0.95]
    )
    .is_err());
}
#[test]
fn dated_swaption_rejects_negative_coefficients_and_preserves_parity() {
    let params = parameter(0.03, 0.012);
    let payments = [
        SwaptionPayment {
            time: 2.0,
            accrual: 0.7,
            discount: (-0.08_f64).exp(),
        },
        SwaptionPayment {
            time: 3.0,
            accrual: 1.2,
            discount: (-0.12_f64).exp(),
        },
    ];
    let expiry_df = (-0.04_f64).exp();
    let payer = swaption(params, 1.0, expiry_df, &payments, 0.05, OptionSide::Put).unwrap();
    let receiver = swaption(params, 1.0, expiry_df, &payments, 0.05, OptionSide::Call).unwrap();
    let fixed =
        0.05 * (0.7 * payments[0].discount + 1.2 * payments[1].discount) + payments[1].discount;
    assert!((payer - receiver - (expiry_df - fixed)).abs() < 1e-12);
    assert!(swaption(params, 1.0, expiry_df, &payments, -0.01, OptionSide::Put).is_err());
    assert!(swaption(params, 1.0, expiry_df, &[], 0.04, OptionSide::Put).is_err());
    let extreme_cash = [SwaptionPayment {
        time: 2.0,
        accrual: f64::MAX,
        discount: 1.0,
    }];
    for volatility in [0.0, 0.012] {
        assert!(swaption(
            parameter(0.03, volatility),
            1.0,
            expiry_df,
            &extreme_cash,
            10.0,
            OptionSide::Call
        )
        .is_err());
    }
}
