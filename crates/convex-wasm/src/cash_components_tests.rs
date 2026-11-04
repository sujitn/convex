//! Literal native cash components; type identity never depends on amount thresholds.
use crate::analyze::fixed_cash_flow_entry;
use convex_bonds::traits::BondCashFlow;
use convex_core::types::Date;
use rust_decimal::Decimal;
use std::str::FromStr;

fn d(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}
fn date() -> Date {
    Date::from_ymd(2028, 6, 15).unwrap()
}
fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() <= 1e-12);
}

#[test]
fn actual_coupon_and_principal_types_preserve_literal_absolute_cash() {
    let face = d("1000.625");
    let c = fixed_cash_flow_entry(&BondCashFlow::coupon(date(), d("25.015625")), face).unwrap();
    assert_eq!(c.cf_type, "coupon");
    close(c.coupon_amount, 25.015625);
    close(c.principal_amount, 0.0);
    close(c.fee_amount, 0.0);
    close(c.amount, 25.015625);
    let p = fixed_cash_flow_entry(&BondCashFlow::principal(date(), face), face).unwrap();
    assert_eq!(p.cf_type, "principal");
    close(p.coupon_amount, 0.0);
    close(p.principal_amount, 1000.625);
    close(p.amount, 1000.625);
}

#[test]
fn combined_terminal_cash_uses_actual_redemption_instead_of_one_hundred() {
    let face = d("1000.625");
    let flow = BondCashFlow::coupon_and_principal(date(), d("25.015625"), face);
    let entry = fixed_cash_flow_entry(&flow, face).unwrap();
    assert_eq!(entry.cf_type, "coupon_and_principal");
    close(entry.coupon_amount, 25.015625);
    close(entry.principal_amount, 1000.625);
    close(entry.amount, 1025.640625);
    close(
        entry.coupon_amount + entry.principal_amount + entry.fee_amount,
        entry.amount,
    );
}

#[test]
fn zero_and_negative_coupon_cannot_relabel_combined_cash_as_principal() {
    let face = d("1000.625");
    for (coupon, total) in [("0", 1000.625), ("-2.5", 998.125)] {
        let flow = BondCashFlow::coupon_and_principal(date(), d(coupon), face);
        let entry = fixed_cash_flow_entry(&flow, face).unwrap();
        assert_eq!(entry.cf_type, "coupon_and_principal");
        close(entry.coupon_amount, d(coupon).to_string().parse().unwrap());
        close(entry.principal_amount, 1000.625);
        close(entry.amount, total);
    }
}

#[test]
fn a_fee_is_never_relabelled_as_coupon() {
    let mut fee = BondCashFlow::coupon(date(), d("-1.25"));
    fee.flow_type = convex_bonds::traits::CashFlowType::Fee;
    let entry = fixed_cash_flow_entry(&fee, d("1000.625")).unwrap();
    assert_eq!(entry.cf_type, "fee");
    close(entry.coupon_amount, 0.0);
    close(entry.principal_amount, 0.0);
    close(entry.fee_amount, -1.25);
    close(entry.amount, -1.25);
}

#[test]
fn invalid_redemption_fail_without_zero_substitution() {
    let flow = BondCashFlow::coupon_and_principal(date(), Decimal::ZERO, d("1000.625"));
    for face in [Decimal::ZERO, d("-1")] {
        assert!(fixed_cash_flow_entry(&flow, face).is_err());
    }
}
