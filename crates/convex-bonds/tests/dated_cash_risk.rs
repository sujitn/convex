//! Independent literal dated cash formulas; no candidate-generated oracle.
use convex_bonds::{pricing::dated_cash::dated_cash_yield_risk, traits::BondCashFlow};
use convex_core::{
    daycounts::DayCountConvention,
    types::{Compounding, Date},
};
use rust_decimal::Decimal;
use serde_json::Value;

fn cash() -> Vec<BondCashFlow> {
    vec![
        BondCashFlow::coupon(Date::from_ymd(2027, 6, 15).unwrap(), Decimal::from(5)),
        BondCashFlow::coupon_and_principal(
            Date::from_ymd(2028, 6, 15).unwrap(),
            Decimal::from(5),
            Decimal::from(100),
        ),
    ]
}
fn settlement() -> Date {
    Date::from_ymd(2026, 6, 15).unwrap()
}
fn compare(actual: f64, row: &Value, field: &str, bound: f64) {
    let expected = row[field].as_str().unwrap().parse::<f64>().unwrap();
    assert!(
        (actual - expected).abs() <= bound,
        "{} {field}: {actual:.17} != {expected:.17}",
        row["id"]
    );
}

#[test]
fn independent_dated_cash_price_and_derivatives() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/dated-cash-risk.json")).unwrap();
    for row in fixture["cases"].as_array().unwrap() {
        let compounding: Compounding = serde_json::from_value(row["compounding"].clone()).unwrap();
        let rate = row["rate"].as_str().unwrap().parse().unwrap();
        let result = dated_cash_yield_risk(
            &cash(),
            settlement(),
            rate,
            DayCountConvention::Thirty360US,
            compounding,
        )
        .unwrap();
        for (field, actual) in [
            ("present_value", result.present_value),
            ("first_derivative", result.first_derivative),
            ("second_derivative", result.second_derivative),
            ("macaulay_duration", result.macaulay_duration),
            ("modified_duration", result.modified_duration),
            ("convexity", result.convexity),
            ("dv01_loss", result.dv01_loss),
        ] {
            compare(
                actual,
                row,
                field,
                fixture["absolute_bounds"][field].as_f64().unwrap(),
            );
        }
    }
}

#[test]
fn currency_scale_and_same_day_components_are_conserved() {
    let rate = 0.04;
    let original = cash();
    let a = dated_cash_yield_risk(
        &original,
        settlement(),
        rate,
        DayCountConvention::Thirty360US,
        Compounding::Annual,
    )
    .unwrap();
    let scale: Decimal = "10.00625".parse().unwrap();
    let mut scaled = original.clone();
    for c in &mut scaled {
        c.amount *= scale;
    }
    let b = dated_cash_yield_risk(
        &scaled,
        settlement(),
        rate,
        DayCountConvention::Thirty360US,
        Compounding::Annual,
    )
    .unwrap();
    assert!((b.present_value - a.present_value * 10.00625).abs() < 1e-10);
    assert!((b.dv01_loss - a.dv01_loss * 10.00625).abs() < 1e-12);
    assert!((b.modified_duration - a.modified_duration).abs() < 1e-11);
    assert!((b.convexity - a.convexity).abs() < 1e-11);
    let mut split = original;
    split[1].amount = Decimal::from(100);
    split.push(BondCashFlow::coupon(split[1].date, Decimal::from(5)));
    let c = dated_cash_yield_risk(
        &split,
        settlement(),
        rate,
        DayCountConvention::Thirty360US,
        Compounding::Annual,
    )
    .unwrap();
    assert!((c.present_value - a.present_value).abs() < 1e-10);
    assert!((c.first_derivative - a.first_derivative).abs() < 1e-10);
    assert!((c.second_derivative - a.second_derivative).abs() < 1e-10);
}

#[test]
fn derivatives_match_actual_same_price_bumps() {
    for compounding in [
        Compounding::Annual,
        Compounding::SemiAnnual,
        Compounding::Quarterly,
        Compounding::Monthly,
        Compounding::Daily,
        Compounding::Continuous,
        Compounding::Simple,
    ] {
        let run = |rate| {
            dated_cash_yield_risk(
                &cash(),
                settlement(),
                rate,
                DayCountConvention::Thirty360US,
                compounding,
            )
            .unwrap()
        };
        let p = run(0.04);
        let h = 1e-5;
        let up = run(0.04 + h);
        let down = run(0.04 - h);
        assert!(
            ((up.present_value - down.present_value) / (2.0 * h) - p.first_derivative).abs() < 1e-6
        );
        // Cancellation in a second difference has a separately declared1e-3 bound.
        assert!(
            ((up.present_value - 2.0 * p.present_value + down.present_value) / (h * h)
                - p.second_derivative)
                .abs()
                < 1e-3
        );
    }
}

#[test]
fn invalid_cash_context_and_rate_domains_fail() {
    let run = |cash: &[BondCashFlow], rate, dc, comp| {
        dated_cash_yield_risk(cash, settlement(), rate, dc, comp)
    };
    let flows = cash();
    for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, -2.0] {
        assert!(run(
            &flows,
            rate,
            DayCountConvention::Thirty360US,
            Compounding::Annual
        )
        .is_err());
    }
    assert!(run(
        &flows,
        -0.5,
        DayCountConvention::Thirty360US,
        Compounding::Simple
    )
    .is_err());
    for dc in [
        DayCountConvention::ActActIcma,
        DayCountConvention::Thirty360EIsda,
    ] {
        assert!(run(&flows, 0.04, dc, Compounding::Annual).is_err());
    }
    assert!(run(
        &[],
        0.04,
        DayCountConvention::Thirty360US,
        Compounding::Annual
    )
    .is_err());
    assert!(run(
        &vec![flows[0].clone(); 4097],
        0.04,
        DayCountConvention::Thirty360US,
        Compounding::Annual
    )
    .is_err());
    let mut bad = flows.clone();
    bad.reverse();
    assert!(run(
        &bad,
        0.04,
        DayCountConvention::Thirty360US,
        Compounding::Annual
    )
    .is_err());
    bad = flows.clone();
    bad[0].date = settlement();
    assert!(run(
        &bad,
        0.04,
        DayCountConvention::Thirty360US,
        Compounding::Annual
    )
    .is_err());
    bad = flows.clone();
    bad[0].amount = -Decimal::ONE;
    assert!(run(
        &bad,
        0.04,
        DayCountConvention::Thirty360US,
        Compounding::Annual
    )
    .is_err());
    bad = flows;
    for c in &mut bad {
        c.amount = Decimal::ZERO;
    }
    assert!(run(
        &bad,
        0.04,
        DayCountConvention::Thirty360US,
        Compounding::Annual
    )
    .is_err());
}
