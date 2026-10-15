//! Original independent future-cash values through the actual native owners.
use convex_bonds::{
    pricing::{
        dated_cash::dated_cash_yield_risk_with_basis,
        discount_time::{DatedYieldCash, DiscountTimeBasis, PreparedDatedCash},
        yield_cash::{price_cash, CashYieldCompounding},
        StandardYieldEngine, YieldEngine, YieldSolver,
    },
    traits::BondCashFlow,
    types::{CompoundingMethod, YieldCalculationRules, YieldConvention},
    BondError,
};
use convex_core::types::{Compounding, Date, Frequency};
use rust_decimal::Decimal;
use serde_json::Value;

fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!(
        "../../../tests/fixtures/library/future-zero-time.json"
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}
fn inputs(action: &Value) -> (Vec<DatedYieldCash>, Date, DiscountTimeBasis) {
    (
        serde_json::from_value(action["cash"].clone()).unwrap(),
        Date::parse(action["settlement"].as_str().unwrap()).unwrap(),
        serde_json::from_value(action["basis"].clone()).unwrap(),
    )
}
fn check_error(error: BondError, reference: &Value) {
    match reference["structural_root_classification"]
        .as_str()
        .unwrap()
    {
        "non_unique" => assert!(matches!(error, BondError::YieldNonUnique), "{error:?}"),
        "no_root" => assert!(matches!(error, BondError::YieldNoRoot), "{error:?}"),
        other => panic!("Undeclared root class {other}"),
    }
}

#[test]
fn prepared_native_prices_preserve_every_original_signed_cash_component() {
    let mut count = 0;
    for row in cases() {
        let action = &row["request"]["operation"]["action"];
        if action["kind"] != "dated_price" {
            continue;
        }
        let (cash, settlement, basis) = inputs(action);
        let prepared = PreparedDatedCash::new(&cash, settlement, &basis).unwrap();
        let compounding = serde_json::from_value(action["compounding"].clone()).unwrap();
        let actual = prepared
            .price(action["annual_yield"].as_f64().unwrap(), compounding)
            .unwrap();
        assert!(
            (actual - number(&row["expected"]["present_value"])).abs()
                <= number(&row["expected"]["price_absolute_bound"]),
            "{} {actual}",
            row["id"]
        );
        for (pair, reference) in prepared
            .discount_cash()
            .iter()
            .zip(row["expected"]["discount_cash"].as_array().unwrap())
        {
            assert!((pair.0 - reference[0].as_f64().unwrap()).abs() <= 1e-14);
            assert_eq!(pair.1, reference[1].as_f64().unwrap());
        }
        assert_eq!(prepared.discount_cash().len(), cash.len());
        count += 1;
    }
    assert_eq!(count, 144);
}

#[test]
fn native_yield_risk_retains_constant_cash_pv_and_all_seven_original_measures() {
    let mut count = 0;
    for row in cases() {
        let action = &row["request"]["operation"]["action"];
        if action["kind"] != "dated_risk" {
            continue;
        }
        let (cash, settlement, basis) = inputs(action);
        let compounding: Compounding =
            serde_json::from_value(action["compounding"].clone()).unwrap();
        let result = dated_cash_yield_risk_with_basis(
            &cash,
            settlement,
            action["annual_yield"].as_f64().unwrap(),
            &basis,
            compounding,
        )
        .unwrap();
        let actual = serde_json::to_value(result).unwrap();
        assert_eq!(actual.as_object().unwrap().len(), 7);
        for (field, expected) in row["expected"]["risk"].as_object().unwrap() {
            let value = actual[field].as_f64().unwrap();
            assert!(
                value.is_finite()
                    && (value - number(expected)).abs()
                        <= number(&row["expected"]["absolute_bound"]),
                "{} {field} {actual}",
                row["id"]
            );
        }
        count += 1;
    }
    assert_eq!(count, 96);
}

#[test]
fn native_inverse_reprices_complete_cash_and_distinguishes_structural_root_classes() {
    let mut count = 0;
    for row in cases() {
        let action = &row["request"]["operation"]["action"];
        if action["kind"] != "dated_solve" {
            continue;
        }
        let (cash, settlement, basis) = inputs(action);
        let prepared = PreparedDatedCash::new(&cash, settlement, &basis).unwrap();
        let compounding = serde_json::from_value(action["compounding"].clone()).unwrap();
        let target = action["target_dirty_price"].as_f64().unwrap();
        let result = YieldSolver::new()
            .with_tolerance(action["tolerance"].as_f64().unwrap())
            .with_max_iterations(action["max_iterations"].as_u64().unwrap() as u32)
            .solve_dated_cash(
                &prepared,
                target,
                compounding,
                action["initial_guess"].as_f64().unwrap(),
            );
        if row["expected_error"].is_null() {
            let result = result.unwrap();
            assert!(
                (result.yield_value - number(&row["expected"]["annual_yield"])).abs()
                    <= number(&row["expected"]["absolute_yield_bound"])
            );
            assert!(result.residual.abs() < number(&row["expected"]["residual_bound"]));
            assert!(
                (prepared.price(result.yield_value, compounding).unwrap() - target).abs()
                    < number(&row["expected"]["residual_bound"])
            );
        } else {
            check_error(result.unwrap_err(), &row["expected"]);
        }
        count += 1;
    }
    assert_eq!(count, 128);
}

fn rules(compounding: CashYieldCompounding, basis: &DiscountTimeBasis) -> YieldCalculationRules {
    let mut rules = YieldCalculationRules::us_treasury();
    rules.discount_day_count = basis.day_count();
    (rules.compounding, rules.frequency, rules.convention) = match compounding {
        CashYieldCompounding::Annual => (
            CompoundingMethod::Periodic { frequency: 1 },
            Frequency::Annual,
            YieldConvention::Annual,
        ),
        CashYieldCompounding::Periodic { frequency } => {
            assert_eq!(frequency, 2.0);
            (
                CompoundingMethod::Periodic { frequency: 2 },
                Frequency::SemiAnnual,
                YieldConvention::StreetConvention,
            )
        }
        CashYieldCompounding::Continuous => (
            CompoundingMethod::Continuous,
            Frequency::Annual,
            YieldConvention::Continuous,
        ),
        CashYieldCompounding::Simple => (
            CompoundingMethod::Simple,
            Frequency::Annual,
            YieldConvention::SimpleYield,
        ),
    };
    rules
}

#[test]
fn both_native_dated_engines_retain_the_complete_price_and_inverse_contract() {
    let mut count = 0;
    for row in cases() {
        let action = &row["request"]["operation"]["action"];
        if action["kind"] == "dated_risk" {
            continue;
        }
        let (cash, settlement, basis) = inputs(action);
        let compounding = serde_json::from_value(action["compounding"].clone()).unwrap();
        let rules = rules(compounding, &basis);
        let flows: Vec<_> = cash
            .iter()
            .map(|c| BondCashFlow::coupon(c.date, c.amount.decimal()))
            .collect();
        let engine = StandardYieldEngine::new()
            .with_tolerance(1e-10)
            .with_max_iterations(100)
            .with_discount_basis(basis.clone());
        let solver = YieldSolver::new()
            .with_tolerance(1e-10)
            .with_max_iterations(100)
            .with_convention(rules.convention)
            .with_discount_basis(basis.clone());
        if action["kind"] == "dated_price" {
            let rate = action["annual_yield"].as_f64().unwrap();
            for actual in [
                engine
                    .price_from_yield(&flows, rate, settlement, &rules)
                    .unwrap(),
                solver
                    .dirty_price_from_yield(
                        &flows,
                        rate,
                        settlement,
                        basis.day_count(),
                        rules.frequency,
                    )
                    .unwrap(),
            ] {
                assert!(
                    (actual - number(&row["expected"]["present_value"])).abs()
                        <= number(&row["expected"]["price_absolute_bound"]),
                    "{} {actual}",
                    row["id"]
                );
            }
        } else {
            let target =
                Decimal::from_f64_retain(action["target_dirty_price"].as_f64().unwrap()).unwrap();
            let a = engine.yield_from_price(&flows, target, Decimal::ZERO, settlement, &rules);
            let b = solver.solve(
                &flows,
                target,
                Decimal::ZERO,
                settlement,
                basis.day_count(),
                rules.frequency,
            );
            if row["expected_error"].is_null() {
                let a = a.unwrap();
                let b = b.unwrap();
                for (rate, residual) in [(a.yield_value, a.residual), (b.yield_value, b.residual)] {
                    assert!(
                        (rate - number(&row["expected"]["annual_yield"])).abs()
                            <= number(&row["expected"]["absolute_yield_bound"]),
                        "{} rate{rate}",
                        row["id"]
                    );
                    assert!(residual.abs() < number(&row["expected"]["residual_bound"]));
                }
            } else {
                check_error(a.unwrap_err(), &row["expected"]);
                check_error(b.unwrap_err(), &row["expected"]);
            }
        }
        count += 1;
    }
    assert_eq!(count, 272);
}

#[test]
fn dated_admission_does_not_widen_raw_times_or_bypass_domains_settings_and_cash_guards() {
    let all = cases();
    let action = &all[0]["request"]["operation"]["action"];
    let (cash, settlement, basis) = inputs(action);
    let prepared = PreparedDatedCash::new(&cash, settlement, &basis).unwrap();
    let laws = [
        CashYieldCompounding::Annual,
        CashYieldCompounding::Periodic { frequency: 2.0 },
        CashYieldCompounding::Continuous,
        CashYieldCompounding::Simple,
    ];
    for law in laws {
        assert!(price_cash(&[(0.0, 100.0)], 0.05, law).is_err());
        assert!(YieldSolver::new()
            .solve_cash(&[(0.0, 100.0)], 100.0, law, 0.05)
            .is_err());
        for rate in [f64::NAN, f64::INFINITY] {
            assert!(prepared.price(rate, law).is_err());
        }
        for (tolerance, iterations) in [(0.0, 100), (f64::NAN, 100), (1e-10, 0), (1e-10, 10001)] {
            assert!(matches!(
                YieldSolver::new()
                    .with_tolerance(tolerance)
                    .with_max_iterations(iterations)
                    .solve_dated_cash(&prepared, 100.0, law, 0.05),
                Err(BondError::InvalidSpec { .. })
            ));
        }
    }
    assert!(prepared.price(-1.0, CashYieldCompounding::Annual).is_err());
    assert!(prepared
        .price(-2.0, CashYieldCompounding::Periodic { frequency: 2.0 })
        .is_err());
    assert!(prepared
        .price(0.0, CashYieldCompounding::Periodic { frequency: 0.0 })
        .is_err());
    let mut invalid = cash.clone();
    invalid[0].date = settlement;
    assert!(PreparedDatedCash::new(&invalid, settlement, &basis).is_err());
    assert!(PreparedDatedCash::new(&vec![cash[0]; 4097], settlement, &basis).is_err());
    assert!(PreparedDatedCash::new(
        &cash,
        settlement,
        &DiscountTimeBasis::Calendar {
            day_count: convex_core::daycounts::DayCountConvention::ActActIcma
        }
    )
    .is_err());
    let signed = all
        .iter()
        .find(|r| r["id"].as_str().unwrap().ends_with("signed-components"))
        .unwrap();
    let (cash, settlement, basis) = inputs(&signed["request"]["operation"]["action"]);
    let prepared = PreparedDatedCash::new(&cash, settlement, &basis).unwrap();
    assert!(matches!(
        YieldSolver::new().solve_dated_cash(&prepared, 100.0, CashYieldCompounding::Annual, 0.05),
        Err(BondError::InvalidSpec { .. })
    ));
    assert!(
        dated_cash_yield_risk_with_basis(&cash, settlement, 0.05, &basis, Compounding::Annual)
            .is_err()
    );
    let (mut cash, settlement, basis) = inputs(action);
    let mut zero_variable = cash[0];
    zero_variable.date = Date::parse("2026-01-31").unwrap();
    zero_variable.amount = convex_core::contracts::ExactDecimal::new(Decimal::ZERO);
    cash.push(zero_variable);
    let prepared = PreparedDatedCash::new(&cash, settlement, &basis).unwrap();
    assert!(matches!(
        YieldSolver::new().solve_dated_cash(&prepared, 100.0, CashYieldCompounding::Annual, 0.05),
        Err(BondError::YieldNonUnique)
    ));
}
