//! Checked supplied-cash numerical domains; dated conventions remain separate.
use convex_bonds::pricing::yield_cash::{price_cash, CashYieldCompounding};
use convex_bonds::pricing::YieldSolver;
use convex_bonds::types::YieldConvention;
use serde_json::Value;

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/yield-pricing.json")).unwrap()
}

#[test]
fn actual_native_price_and_inverse_match_predeclared_decimal110_references() {
    for row in reference()["cases"].as_array().unwrap() {
        let cash: Vec<(f64, f64)> = serde_json::from_value(row["cash"].clone()).unwrap();
        let convention: YieldConvention =
            serde_json::from_value(row["convention"].clone()).unwrap();
        let solver = YieldSolver::new().with_convention(convention);
        let yield_value = row["annual_yield"].as_f64().unwrap();
        let frequency = row["frequency"].as_f64().unwrap();
        let expected = row["expected_price"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        let bound = row["price_absolute_bound"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        let actual = solver
            .price_primitive(&cash, yield_value, frequency)
            .unwrap();
        assert!(
            (actual - expected).abs() <= bound,
            "{} price {actual} != {expected}",
            row["id"]
        );
        if row["inverse_admitted"] == true {
            let result = solver
                .solve_primitive(
                    &cash,
                    expected,
                    frequency,
                    row["initial_guess"].as_f64().unwrap(),
                )
                .unwrap();
            let yield_bound = row["yield_absolute_bound"]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!(
                (result.yield_value - yield_value).abs() <= yield_bound,
                "{}",
                row["id"]
            );
            let residual = solver
                .price_primitive(&cash, result.yield_value, frequency)
                .unwrap()
                - expected;
            assert_eq!(result.residual, residual);
            assert!(residual.abs() < 1e-10);
        }
    }
}

#[test]
fn direct_fallible_dates_and_both_engine_paths_use_the_same_cash_basis() {
    use convex_bonds::{pricing::StandardYieldEngine, traits::BondCashFlow};
    use convex_core::{
        daycounts::DayCountConvention,
        types::{Date, Frequency},
    };
    use rust_decimal::Decimal;
    let root = reference();
    let row = &root["dated_control"];
    let settlement = Date::parse(row["settlement"].as_str().unwrap()).unwrap();
    let payment = Date::parse(row["payment"].as_str().unwrap()).unwrap();
    let cash = [BondCashFlow::principal(payment, Decimal::from(100))];
    let expected = row["expected_price"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    let solver = YieldSolver::new();
    assert!(
        (solver
            .dirty_price_from_yield(
                &cash,
                0.05,
                settlement,
                DayCountConvention::Thirty360US,
                Frequency::SemiAnnual
            )
            .unwrap()
            - expected)
            .abs()
            < 1e-10
    );
    assert!(
        (solver
            .clean_price_from_yield(
                &cash,
                0.05,
                Decimal::ONE,
                settlement,
                DayCountConvention::Thirty360US,
                Frequency::SemiAnnual
            )
            .unwrap()
            - (expected - 1.0))
            .abs()
            < 1e-10
    );
    let rules = convex_bonds::types::YieldCalculationRules::us_corporate();
    let engine = StandardYieldEngine::new();
    let native = convex_bonds::pricing::YieldEngine::price_from_yield(
        &engine, &cash, 0.05, settlement, &rules,
    )
    .unwrap();
    let analytics = convex_analytics::yields::YieldEngine::price_from_yield(
        &engine, &cash, 0.05, settlement, &rules,
    )
    .unwrap();
    assert_eq!(native, analytics);
    assert!((native - expected).abs() < 1e-10);
    assert!(solver
        .dirty_price_from_yield(
            &cash,
            -3.0,
            settlement,
            DayCountConvention::Thirty360US,
            Frequency::SemiAnnual
        )
        .is_err());
    assert!(solver
        .dirty_price_from_yield(
            &cash,
            0.05,
            payment,
            DayCountConvention::Thirty360US,
            Frequency::SemiAnnual
        )
        .is_err());
}

#[test]
fn finite_domain_arithmetic_work_settings_and_explicit_conventions_are_enforced() {
    let basis = CashYieldCompounding::Periodic { frequency: 2.0 };
    for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -2.0, -3.0] {
        assert!(price_cash(&[(2.0, 100.0)], rate, basis).is_err());
    }
    for cash in [
        vec![],
        vec![(0.0, 100.0)],
        vec![(f64::NAN, 100.0)],
        vec![(1.0, f64::INFINITY)],
        vec![(1.0, 0.0)],
        vec![(1.0, 1.0); 4097],
    ] {
        assert!(price_cash(&cash, 0.05, basis).is_err());
    }
    for solver in [
        YieldSolver::new().with_tolerance(0.0),
        YieldSolver::new().with_tolerance(f64::NAN),
        YieldSolver::new().with_max_iterations(0),
        YieldSolver::new().with_max_iterations(10001),
    ] {
        assert!(solver
            .solve_primitive(&[(1.0, 100.0)], 95.0, 2.0, 0.05)
            .is_err());
    }
    for convention in [
        YieldConvention::ISMA,
        YieldConvention::DiscountYield,
        YieldConvention::BondEquivalentYield,
        YieldConvention::MunicipalYield,
        YieldConvention::Moosmuller,
        YieldConvention::BraessFangmeyer,
    ] {
        assert!(YieldSolver::new()
            .with_convention(convention)
            .price_primitive(&[(1.0, 100.0)], 0.05, 2.0)
            .is_err());
    }
    assert!(price_cash(&[(1.0, 100.0)], 1000.0, CashYieldCompounding::Continuous).is_err());
    assert!(price_cash(&[(2.0, f64::from_bits(1))], 0.5, basis).is_err());
    assert_eq!(
        price_cash(&[(1.0, 0.0), (1.0, 100.0)], 0.0, basis).unwrap(),
        100.0
    );
    assert!(price_cash(&[(2.0, 100.0)], -3.0, CashYieldCompounding::Continuous).is_ok());
}

#[test]
fn periodic_discount_domain_is_checked_before_a_positive_even_power_price() {
    assert!(YieldSolver::new()
        .price_primitive(&[(2.0, 100.0)], -3.0, 2.0)
        .is_err());
    assert!(YieldSolver::new()
        .with_convention(YieldConvention::TrueYield)
        .price_primitive(&[(2.0, 100.0)], -1.5, 2.0)
        .is_err());
}

#[test]
fn signed_cash_inverse_needs_an_explicit_root_selection_contract() {
    // 230/(1+y)-132/(1+y)^2=100 has both y=.1 and y=.2.
    let cash = [(1.0, 230.0), (2.0, -132.0)];
    for guess in [0.05, 0.3] {
        assert!(YieldSolver::new()
            .solve_primitive(&cash, 100.0, 1.0, guess)
            .is_err());
    }
}
