//! Independent dated payment-time controls over the actual native owners.
use convex_bonds::{
    pricing::{
        discount_time::{prepare_dated_cash, DatedYieldCash, DiscountTimeBasis},
        StandardYieldEngine, YieldEngine, YieldSolver,
    },
    traits::BondCashFlow,
    types::YieldCalculationRules,
};
use convex_core::{
    daycounts::DayCountConvention,
    types::{Date, Frequency},
};
use rust_decimal::Decimal;
use serde_json::Value;

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/dated-yield.json")).unwrap()
}
fn date(s: &str) -> Date {
    Date::parse(s).unwrap()
}

#[test]
fn explicit_discount_role_controls_both_price_and_inverse() {
    let root = reference();
    let row = &root["cases"][0];
    let settlement = date(row["settlement"].as_str().unwrap());
    let cash = [BondCashFlow::principal(
        date(row["payment"].as_str().unwrap()),
        Decimal::from(100),
    )];
    let mut rules = YieldCalculationRules::us_corporate();
    rules.accrual_day_count = DayCountConvention::Act365Fixed;
    rules.discount_day_count = DayCountConvention::Act360;
    rules.compounding = convex_bonds::types::CompoundingMethod::annual();
    rules.frequency = Frequency::Annual;
    let engine = StandardYieldEngine::new();
    let expected = row["expected_price"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    let actual = engine
        .price_from_yield(&cash, 0.05, settlement, &rules)
        .unwrap();
    assert!(
        (actual - expected).abs() <= 1e-10,
        "actual {actual}, independently expected {expected}"
    );
    let inverse = engine
        .yield_from_price(
            &cash,
            expected.to_string().parse().unwrap(),
            Decimal::ZERO,
            settlement,
            &rules,
        )
        .unwrap();
    assert!((inverse.yield_value - 0.05).abs() <= 1e-11);
}

#[test]
fn missing_intermediate_cash_does_not_delete_a_reference_period() {
    let root = reference();
    let row = &root["cases"][1];
    let settlement = date(row["settlement"].as_str().unwrap());
    let payment = date(row["payment"].as_str().unwrap());
    let cash = [BondCashFlow::principal(payment, Decimal::from(100))
        .with_accrual(date("2025-07-01"), payment)];
    let expected = row["expected_price"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    let basis: DiscountTimeBasis=serde_json::from_value(serde_json::json!({
        "kind":"icma", "schedule":{"start_date":"2025-01-01","end_date":"2026-01-01","frequency":"SemiAnnual","stub_type":"None","end_of_month":false,"first_regular_date":null,"penultimate_date":null},
        "reference_periods":[{"start":"2025-01-01","end":"2025-07-01"},{"start":"2025-07-01","end":"2026-01-01"}]})).unwrap();
    let actual = YieldSolver::new()
        .with_discount_basis(basis)
        .dirty_price_from_yield(
            &cash,
            0.05,
            settlement,
            DayCountConvention::ActActIcma,
            Frequency::SemiAnnual,
        )
        .unwrap();
    assert!(
        (actual - expected).abs() <= 1e-10,
        "actual {actual}, independently expected {expected}"
    );
}

#[test]
fn act365l_consumes_the_actual_coupon_end_year() {
    let root = reference();
    let row = &root["cases"][2];
    let settlement = date(row["settlement"].as_str().unwrap());
    let payment = date(row["payment"].as_str().unwrap());
    let cash =
        [BondCashFlow::principal(payment, Decimal::from(100)).with_accrual(settlement, payment)];
    let expected = row["expected_price"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    let basis = DiscountTimeBasis::Act365L {
        frequency: Frequency::SemiAnnual,
        coupon_periods: vec![convex_core::cashflows::ReferencePeriod {
            start: settlement,
            end: payment,
        }],
    };
    let actual = YieldSolver::new()
        .with_discount_basis(basis)
        .dirty_price_from_yield(
            &cash,
            0.05,
            settlement,
            DayCountConvention::Act365Leap,
            Frequency::SemiAnnual,
        )
        .unwrap();
    assert!(
        (actual - expected).abs() <= 1e-10,
        "actual {actual}, independently expected {expected}"
    );
}

#[test]
fn full_dated_grid_matches_original_fraction_decimal110_time_price_inverse_and_risk() {
    use convex_bonds::pricing::{
        dated_cash::dated_cash_yield_risk_with_basis,
        yield_cash::{price_cash, CashYieldCompounding},
    };
    use convex_bonds::types::CompoundingMethod;
    use convex_core::types::Compounding;
    let root: Value = serde_json::from_str(include_str!("fixtures/dated-yield-grid.json")).unwrap();
    for row in root["cases"].as_array().unwrap() {
        let basis: DiscountTimeBasis = serde_json::from_value(row["basis"].clone()).unwrap();
        let cash: Vec<DatedYieldCash> = serde_json::from_value(row["cash"].clone()).unwrap();
        let settle = date(row["settlement"].as_str().unwrap());
        let rate = row["annual_yield"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        let comp: Compounding = serde_json::from_value(row["compounding"].clone()).unwrap();
        let mathematical = match comp {
            Compounding::Continuous => CashYieldCompounding::Continuous,
            Compounding::Simple => CashYieldCompounding::Simple,
            _ => CashYieldCompounding::Periodic {
                frequency: f64::from(comp.periods_per_year()),
            },
        };
        let prepared = prepare_dated_cash(&cash, settle, &basis).unwrap();
        for (i, (t, amount)) in prepared.iter().enumerate() {
            let expected = row["discount_years"][i]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!((t - expected).abs() <= 1e-14, "{} payment {i}", row["id"]);
            assert_eq!(
                *amount,
                row["cash"][i]["amount"]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap()
            );
        }
        let expected = row["expected"]["present_value"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        let actual = price_cash(&prepared, rate, mathematical).unwrap();
        assert!(
            (actual - expected).abs() <= 1e-10,
            "{} price {actual} != {expected}",
            row["id"]
        );
        let inverse = YieldSolver::new()
            .solve_cash(&prepared, expected, mathematical, 0.03)
            .unwrap();
        assert!(
            (inverse.yield_value - rate).abs() <= 1e-11,
            "{} inverse {}",
            row["id"],
            inverse.yield_value
        );
        assert!(inverse.residual.abs() < 1e-10);
        let risk = dated_cash_yield_risk_with_basis(&cash, settle, rate, &basis, comp).unwrap();
        for (key, actual) in [
            ("present_value", risk.present_value),
            ("first_derivative", risk.first_derivative),
            ("second_derivative", risk.second_derivative),
            ("macaulay_duration", risk.macaulay_duration),
            ("modified_duration", risk.modified_duration),
            ("convexity", risk.convexity),
            ("dv01_loss", risk.dv01_loss),
        ] {
            let expected = row["expected"][key]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!(
                (actual - expected).abs() <= 1e-9,
                "{} {key}: {actual} != {expected}",
                row["id"]
            );
        }
        // Both public engine paths consume the discount role and exact context.
        // Accrual is deliberately a different independent role in every case.
        let original: Vec<_> = cash
            .iter()
            .map(|c| BondCashFlow::principal(c.date, c.amount.decimal()))
            .collect();
        let mut rules = YieldCalculationRules::us_corporate();
        rules.discount_day_count = basis.day_count();
        rules.accrual_day_count = if basis.day_count() == DayCountConvention::Act360 {
            DayCountConvention::Act365Fixed
        } else {
            DayCountConvention::Act360
        };
        rules.compounding = if comp == Compounding::Continuous {
            CompoundingMethod::Continuous
        } else {
            CompoundingMethod::Periodic {
                frequency: comp.periods_per_year(),
            }
        };
        let engine = StandardYieldEngine::new().with_discount_basis(basis);
        let actual = engine
            .price_from_yield(&original, rate, settle, &rules)
            .unwrap();
        assert!(
            (actual - expected).abs() <= 1e-10,
            "{} engine {actual}",
            row["id"]
        );
        let analytics = convex_analytics::yields::YieldEngine::price_from_yield(
            &engine, &original, rate, settle, &rules,
        )
        .unwrap();
        assert_eq!(actual, analytics);
        let inverse = convex_analytics::yields::YieldEngine::yield_from_price(
            &engine,
            &original,
            expected.to_string().parse().unwrap(),
            Decimal::ZERO,
            settle,
            &rules,
        )
        .unwrap();
        assert!(
            (inverse.yield_value - rate).abs() <= 1e-11,
            "{} engine inverse",
            row["id"]
        );
    }
}

#[test]
fn missing_context_mismatched_roles_and_direct_capacity_fail_without_inference() {
    let settle = date("2025-01-01");
    let payment = date("2026-01-01");
    let cash = [BondCashFlow::principal(payment, Decimal::from(100))];
    for dc in [
        DayCountConvention::ActActIcma,
        DayCountConvention::Act365Leap,
        DayCountConvention::Thirty360EIsda,
    ] {
        let mut rules = YieldCalculationRules::us_corporate();
        rules.discount_day_count = dc;
        assert!(StandardYieldEngine::new()
            .price_from_yield(&cash, 0.05, settle, &rules)
            .is_err());
        assert!(YieldSolver::new()
            .dirty_price_from_yield(&cash, 0.05, settle, dc, Frequency::SemiAnnual)
            .is_err());
    }
    let basis = DiscountTimeBasis::Calendar {
        day_count: DayCountConvention::Act360,
    };
    let engine = StandardYieldEngine::new().with_discount_basis(basis.clone());
    assert!(engine
        .price_from_yield(&cash, 0.05, settle, &YieldCalculationRules::us_corporate())
        .is_err());
    assert!(
        prepare_dated_cash(&vec![DatedYieldCash::from(&cash[0]); 4097], settle, &basis).is_err()
    );
}
