//! Independent scalar answers and actual checked-object migration controls.
use convex::bill_yield as bill;
use convex_analytics::{error::AnalyticsError, yields::ShortDateCalculator as AnalyticsCalculator};
use convex_bonds::{
    instruments::ZeroCouponBond, pricing::ShortDateCalculator as BondCalculator, BondError,
};
use convex_core::types::Date;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

fn d(text: &str) -> Decimal {
    Decimal::from_str(text)
        .or_else(|_| Decimal::from_scientific(text))
        .unwrap()
}
fn compare(actual: Decimal, expected: &str, scale: Decimal) {
    let expected = d(expected) * scale;
    assert!(
        (actual - expected).abs() <= (d("1e-22") + d("1e-24") * expected.abs()) * scale,
        "{actual} differs from {expected}"
    );
}
fn bond(face: Decimal, days: i64) -> ZeroCouponBond {
    let issue = Date::from_ymd(2026, 1, 1).unwrap();
    ZeroCouponBond::builder()
        .cusip_unchecked("BILL-CONTROL")
        .issue_date(issue)
        .maturity(issue.add_days(days))
        .face_value(face)
        .redemption_value(face)
        .us_treasury_bill()
        .build()
        .unwrap()
}

pub fn run() -> Value {
    let reference: Value =
        serde_json::from_str(include_str!("bill-methods-reference.json")).unwrap();
    let boundaries: Value =
        serde_json::from_str(include_str!("bill-methods-boundary-reference.json")).unwrap();
    let a = AnalyticsCalculator::default();
    let b = BondCalculator::default();
    let issue = Date::from_ymd(2026, 1, 1).unwrap();
    let hundred = Decimal::from(100);
    for c in reference["cash_cases"].as_array().unwrap() {
        let price = d(c["price"].as_str().unwrap());
        let face = d(c["face"].as_str().unwrap());
        let days = c["days"].as_i64().unwrap();
        let instrument = bond(face, days);
        let before = serde_json::to_value(&instrument).unwrap();
        let bank = c["bank_discount_decimal"].as_str().unwrap();
        let mmy = c["money_market_decimal"].as_str().unwrap();
        compare(
            bill::bank_discount_yield(price, face, days).unwrap(),
            bank,
            Decimal::ONE,
        );
        compare(
            instrument.discount_yield(price, issue).unwrap(),
            bank,
            Decimal::ONE,
        );
        compare(
            bill::money_market_yield(price, face, days).unwrap(),
            mmy,
            Decimal::ONE,
        );
        compare(
            a.money_market_yield(price, face, days).unwrap(),
            mmy,
            Decimal::ONE,
        );
        compare(
            b.money_market_yield(price, face, days).unwrap(),
            mmy,
            Decimal::ONE,
        );
        compare(
            convex_analytics::yields::discount_yield(price, face, days as u32).unwrap(),
            bank,
            hundred,
        );
        compare(
            convex_analytics::yields::cd_equivalent_yield(price, face, days as u32).unwrap(),
            mmy,
            hundred,
        );
        if let Some(expected) = c["simple_fixed365_decimal"].as_str() {
            compare(
                bill::simple_bill_yield(price, face, days).unwrap(),
                expected,
                Decimal::ONE,
            );
            compare(
                instrument.bond_equivalent_yield(price, issue).unwrap(),
                expected,
                Decimal::ONE,
            );
            let rate = bill::money_market_yield(price, face, days).unwrap();
            compare(
                a.bond_equivalent_yield(rate, days).unwrap(),
                expected,
                Decimal::ONE,
            );
            compare(
                b.bond_equivalent_yield(rate, days).unwrap(),
                expected,
                Decimal::ONE,
            );
            compare(
                convex_analytics::yields::bond_equivalent_yield(price, face, days as u32).unwrap(),
                expected,
                hundred,
            );
        } else {
            assert!(matches!(
                instrument.bond_equivalent_yield(price, issue),
                Err(BondError::InvalidSpec { .. })
            ));
            assert!(matches!(
                a.bond_equivalent_yield(d(mmy), days),
                Err(AnalyticsError::InvalidInput(_))
            ));
            assert!(matches!(
                b.bond_equivalent_yield(d(mmy), days),
                Err(BondError::InvalidSpec { .. })
            ));
            assert!(bill::simple_bill_yield(price, face, days).is_err());
        }
        assert_eq!(serde_json::to_value(&instrument).unwrap(), before);
        let rebuilt: ZeroCouponBond = serde_json::from_value(before.clone()).unwrap();
        assert_eq!(serde_json::to_value(rebuilt).unwrap(), before);
    }
    for c in reference["rate_cases"].as_array().unwrap() {
        let rate = d(c["money_market_decimal"].as_str().unwrap());
        let days = c["days"].as_i64().unwrap();
        if let Some(expected) = c["simple_fixed365_decimal"].as_str() {
            compare(
                bill::simple_bill_yield_from_money_market(rate, days).unwrap(),
                expected,
                Decimal::ONE,
            );
            compare(
                a.bond_equivalent_yield(rate, days).unwrap(),
                expected,
                Decimal::ONE,
            );
            compare(
                b.bond_equivalent_yield(rate, days).unwrap(),
                expected,
                Decimal::ONE,
            );
        } else {
            assert!(bill::simple_bill_yield_from_money_market(rate, days).is_err());
            assert!(a.bond_equivalent_yield(rate, days).is_err());
            assert!(b.bond_equivalent_yield(rate, days).is_err());
        }
    }
    for c in boundaries["cases"].as_array().unwrap() {
        let rate = d(c["money_market_decimal"].as_str().unwrap());
        let days = c["days"].as_i64().unwrap();
        if c["admissible"].as_bool().unwrap() {
            let expected = c["simple_fixed365_decimal"].as_str().unwrap();
            compare(
                bill::simple_bill_yield_from_money_market(rate, days).unwrap(),
                expected,
                Decimal::ONE,
            );
            compare(
                a.bond_equivalent_yield(rate, days).unwrap(),
                expected,
                Decimal::ONE,
            );
            compare(
                b.bond_equivalent_yield(rate, days).unwrap(),
                expected,
                Decimal::ONE,
            );
        } else {
            assert!(bill::simple_bill_yield_from_money_market(rate, days).is_err());
            assert!(a.bond_equivalent_yield(rate, days).is_err());
            assert!(b.bond_equivalent_yield(rate, days).is_err());
        }
    }
    for c in reference["invalid_cash"].as_array().unwrap() {
        let price = d(c["price"].as_str().unwrap());
        let face = d(c["face"].as_str().unwrap());
        let days = c["days"].as_i64().unwrap();
        assert!(bill::bank_discount_yield(price, face, days).is_err());
        assert!(bill::money_market_yield(price, face, days).is_err());
        assert!(bill::simple_bill_yield(price, face, days).is_err());
        assert!(a.money_market_yield(price, face, days).is_err());
        assert!(b.money_market_yield(price, face, days).is_err());
    }
    let instrument = bond(hundred, 90);
    for settlement in [issue.add_days(-1), issue.add_days(90), issue.add_days(91)] {
        assert!(instrument.discount_yield(d("97"), settlement).is_err());
        assert!(instrument
            .bond_equivalent_yield(d("97"), settlement)
            .is_err());
    }
    for price in [Decimal::ZERO, d("-1")] {
        assert!(instrument.discount_yield(price, issue).is_err());
        assert!(instrument.bond_equivalent_yield(price, issue).is_err());
    }
    let no_interval = bond(hundred, 0);
    assert!(no_interval.discount_yield(d("97"), issue).is_err());
    let bad_face = bond(Decimal::ZERO, 90);
    assert!(bad_face.discount_yield(d("97"), issue).is_err());
    let mut terms = serde_json::to_value(&instrument).unwrap();
    terms["redemption_value"] = json!("99");
    let non_face: ZeroCouponBond = serde_json::from_value(terms).unwrap();
    assert!(non_face.discount_yield(d("97"), issue).is_err());
    assert!(non_face.bond_equivalent_yield(d("97"), issue).is_err());
    assert!(matches!(
        bill::simple_bill_yield(d("1e-28"), hundred, 1),
        Err(BondError::PricingFailed { .. })
    ));
    assert!(matches!(
        a.bond_equivalent_yield(Decimal::MAX, 1),
        Err(AnalyticsError::CalculationFailed(_))
    ));
    assert!(matches!(
        b.bond_equivalent_yield(Decimal::MAX, 1),
        Err(BondError::PricingFailed { .. })
    ));
    assert!(a.bond_equivalent_yield(Decimal::ZERO, i64::MAX).is_err());
    assert!(b.money_market_yield(d("97"), hundred, i64::MIN).is_err());
    json!({"status":"passed","independent_cash_cases":40,"independent_rate_cases":42,"exact_pole_cases":9,"checked_object_methods":6,"certified":false})
}
