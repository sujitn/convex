//! Independent actual zero-coupon cash, rate, date and domain controls.
use convex::zero_yield;
use convex_bonds::{
    instruments::{convert_yield, Compounding, ZeroCouponBond},
    traits::Bond,
    BondError,
};
use convex_core::{daycounts::DayCountConvention, types::Date};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

fn d(value: &str) -> Decimal {
    Decimal::from_str(value)
        .or_else(|_| Decimal::from_scientific(value))
        .unwrap()
}
fn comp(value: &str) -> Compounding {
    serde_json::from_value(json!(value)).unwrap()
}
fn instrument(c: &Value) -> ZeroCouponBond {
    ZeroCouponBond::builder()
        .cusip_unchecked("ZERO-CONTROL")
        .issue_date(Date::parse(c["issue"].as_str().unwrap()).unwrap())
        .maturity(Date::parse(c["maturity"].as_str().unwrap()).unwrap())
        .face_value(d(c["face"].as_str().unwrap()))
        .redemption_value(d(c["redemption"].as_str().unwrap()))
        .day_count(serde_json::from_value(c["day_count"].clone()).unwrap())
        .compounding(comp(c["compounding"].as_str().unwrap()))
        .build()
        .unwrap()
}
fn compare(actual: Decimal, expected: &str) {
    let expected = d(expected);
    assert!(
        (actual - expected).abs() <= d("1e-12") + d("5e-13") * expected.abs(),
        "{actual} differs from {expected}"
    );
    if expected != Decimal::ZERO {
        assert_eq!(actual.is_sign_positive(), expected.is_sign_positive());
        assert_ne!(actual, Decimal::ZERO);
    } else {
        assert_eq!(actual, Decimal::ZERO);
    }
}

pub fn run() -> Value {
    let pack: Value = serde_json::from_str(include_str!("zero-methods-reference.json")).unwrap();
    for c in pack["price_cases"].as_array().unwrap() {
        let bond = instrument(c);
        let before = serde_json::to_value(&bond).unwrap();
        let settlement = Date::parse(c["settlement"].as_str().unwrap()).unwrap();
        let rate = d(c["rate"].as_str().unwrap());
        let price = bond.price_from_yield(rate, settlement).unwrap();
        compare(price, c["expected"].as_str().unwrap());
        if rate != Decimal::ZERO {
            assert_ne!(price, bond.redemption_value());
            assert_eq!(price < bond.redemption_value(), rate > Decimal::ZERO);
        }
        compare(
            zero_yield::price_from_yield(
                bond.redemption_value(),
                rate,
                c["year_fraction"].as_str().unwrap().parse().unwrap(),
                bond.compounding(),
            )
            .unwrap(),
            c["expected"].as_str().unwrap(),
        );
        compare(
            bond.yield_from_price(price, settlement).unwrap(),
            c["rate"].as_str().unwrap(),
        );
        assert_eq!(serde_json::to_value(&bond).unwrap(), before);
        let roundtrip: ZeroCouponBond = serde_json::from_value(before.clone()).unwrap();
        assert_eq!(serde_json::to_value(roundtrip).unwrap(), before);
    }
    for c in pack["inverse_cases"].as_array().unwrap() {
        let bond = instrument(c);
        let settlement = Date::parse(c["settlement"].as_str().unwrap()).unwrap();
        let price = d(c["price"].as_str().unwrap());
        let rate = bond.yield_from_price(price, settlement).unwrap();
        compare(rate, c["expected"].as_str().unwrap());
        compare(
            bond.continuous_rate(price, settlement).unwrap(),
            c["expected_continuous"].as_str().unwrap(),
        );
        compare(
            bond.price_from_yield(rate, settlement).unwrap(),
            c["price"].as_str().unwrap(),
        );
    }
    for c in pack["conversion_cases"].as_array().unwrap() {
        let source = comp(c["source"].as_str().unwrap());
        let target = comp(c["target"].as_str().unwrap());
        let rate = d(c["rate"].as_str().unwrap());
        let converted = convert_yield(rate, source, target).unwrap();
        compare(converted, c["expected"].as_str().unwrap());
        if source == target {
            assert_eq!(converted, rate);
        }
        compare(
            convert_yield(converted, target, source).unwrap(),
            c["rate"].as_str().unwrap(),
        );
    }
    let date = Date::from_ymd(2026, 1, 1).unwrap();
    for c in pack["pole_cases"].as_array().unwrap() {
        let compounding = comp(c["compounding"].as_str().unwrap());
        let rate = d(c["rate"].as_str().unwrap());
        let bond = ZeroCouponBond::builder()
            .cusip_unchecked("ZERO-POLE")
            .issue_date(date)
            .maturity(date.add_days(1))
            .day_count(DayCountConvention::Act360)
            .compounding(compounding)
            .build()
            .unwrap();
        if c["admissible"].as_bool().unwrap() {
            let price = bond.price_from_yield(rate, date).unwrap();
            compare(price, c["expected_price"].as_str().unwrap());
            compare(
                convert_yield(rate, compounding, Compounding::Continuous).unwrap(),
                c["expected_continuous"].as_str().unwrap(),
            );
            assert_eq!(convert_yield(rate, compounding, compounding).unwrap(), rate);
            let inverse = bond.yield_from_price(price, date).unwrap();
            assert!(inverse > -Decimal::from(compounding.periods_per_year().unwrap()));
            compare(
                bond.price_from_yield(inverse, date).unwrap(),
                c["expected_price"].as_str().unwrap(),
            );
        } else {
            assert!(matches!(
                bond.price_from_yield(rate, date),
                Err(BondError::InvalidSpec { .. })
            ));
            assert!(convert_yield(rate, compounding, Compounding::Continuous).is_err());
            assert!(convert_yield(rate, compounding, compounding).is_err());
        }
    }
    assert!(Compounding::Continuous.periods_per_year().is_err());
    assert_eq!(Compounding::Continuous.periods_per_year_opt(), None);
    let original = instrument(&pack["price_cases"][0]);
    for invalid in [
        Date::from_ymd(2019, 12, 31).unwrap(),
        date.add_days(1),
        date.add_days(2),
    ] {
        // The first original contract runs from date through date+1.
        assert!(original.price_from_yield(d("0.05"), invalid).is_err());
        assert!(original.yield_from_price(d("97"), invalid).is_err());
        assert!(original.continuous_rate(d("97"), invalid).is_err());
    }
    for price in [Decimal::ZERO, d("-1")] {
        assert!(original.yield_from_price(price, date).is_err());
        assert!(original.continuous_rate(price, date).is_err());
    }
    for field in ["face_value", "redemption_value"] {
        let mut terms = serde_json::to_value(&original).unwrap();
        terms[field] = json!("0");
        let bad: ZeroCouponBond = serde_json::from_value(terms).unwrap();
        assert!(bad.price_from_yield(d("0.05"), date).is_err());
        assert!(bad.yield_from_price(d("97"), date).is_err());
    }
    for dc in [
        "ActActIcma",
        "Act365Leap",
        "ActActAfb",
        "Thirty360EIsda",
        "Thirty360German",
    ] {
        let mut terms = serde_json::to_value(&original).unwrap();
        terms["day_count"] = json!(dc);
        let bad: ZeroCouponBond = serde_json::from_value(terms).unwrap();
        assert!(bad.price_from_yield(d("0.05"), date).is_err());
    }
    for (issue, maturity) in [
        ("2026-01-02", "2026-01-02"),
        ("2026-01-03", "2026-01-02"),
        ("2020-01-01", "2300-01-01"),
    ] {
        let mut terms = serde_json::to_value(&original).unwrap();
        terms["issue_date"] = json!(issue);
        terms["maturity"] = json!(maturity);
        let bad: ZeroCouponBond = serde_json::from_value(terms).unwrap();
        assert!(bad.price_from_yield(d("0.05"), date).is_err());
    }
    for years in [0.0, -1.0, f64::NAN, f64::INFINITY, 151.0] {
        assert!(
            zero_yield::price_from_yield(d("100"), d("0.05"), years, Compounding::Annual).is_err()
        );
        assert!(
            zero_yield::yield_from_price(d("100"), d("97"), years, Compounding::Annual).is_err()
        );
    }
    assert!(
        zero_yield::price_from_yield(Decimal::MAX, d("-0.5"), 1.0, Compounding::Annual).is_err()
    );
    assert!(
        zero_yield::price_from_yield(d("100"), Decimal::MAX, 1.0, Compounding::Continuous).is_err()
    );
    assert!(
        zero_yield::yield_from_price(d("100"), d("1e-28"), 0.0001, Compounding::Annual).is_err()
    );
    assert_eq!(
        convert_yield(Decimal::MAX, Compounding::Annual, Compounding::Annual).unwrap(),
        Decimal::MAX
    );
    json!({"status":"passed","independent_price_cases":300,"independent_inverse_cases":375,"independent_conversion_cases":125,"exact_pole_cases":12,"checked_methods":5,"certified":false})
}
