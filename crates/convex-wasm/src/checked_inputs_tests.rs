//! Independently declared input and per100 units; no candidate-generated reference.
use crate::{
    bond::{create_bond, get_yield_rules},
    convert::*,
};
use convex_bonds::traits::{Bond, FixedCouponBond};
use rust_decimal::Decimal;
use std::str::FromStr;

fn fixed() -> crate::dto::BondParams {
    serde_json::from_value(
        serde_json::json!({"coupon_rate":5.0,"maturity_date":"2030-06-15",
        "issue_date":"2020-06-15","settlement_date":"2026-06-15","face_value":100.0,
        "frequency":2,"day_count":"30/360","currency":"USD"}),
    )
    .unwrap()
}

#[test]
fn exact_numeric_and_percentage_inputs_have_no_rounding_or_zero_substitution() {
    for (input, expected) in [
        (5.12345678912345, "5.12345678912345"),
        (0.000000000123456789, "0.000000000123456789"),
        (1000.625, "1000.625"),
    ] {
        assert_eq!(
            f64_to_decimal(input).unwrap(),
            Decimal::from_str(expected).unwrap()
        );
    }
    assert_eq!(
        percentage_to_decimal(5.12345678912345).unwrap(),
        Decimal::from_str("0.0512345678912345").unwrap()
    );
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, 1e-100] {
        assert!(f64_to_decimal(value).is_err());
    }
    assert!(percentage_to_decimal(1e-27).is_err());
}

#[test]
fn unknown_enum_values_fail_and_old_aliases_remain() {
    assert!(parse_currency("XXUSD").is_err());
    assert!(parse_market("MOON").is_err());
    assert!(parse_frequency(3).is_err());
    assert!(parse_day_count("ACT/MOON").is_err());
    assert!(parse_instrument_type("UNKNOWN").is_err());
    assert!(parse_yield_convention("UNKNOWN").is_err());
    assert!(parse_compounding("UNKNOWN").is_err());
    for value in ["US", "USA", "United States"] {
        assert_eq!(
            parse_market(value).unwrap(),
            convex_bonds::conventions::Market::US
        );
    }
    for value in ["30/360", "30/360 US"] {
        assert_eq!(
            parse_day_count(value).unwrap(),
            convex_core::daycounts::DayCountConvention::Thirty360US
        );
    }
    assert!(parse_date("2026-6-15").is_err());
    assert!(parse_date("2026-06-15").is_ok());
}

#[test]
fn construction_retains_exact_coupon_and_cash_face_and_rejects_other_payoffs() {
    let mut params = fixed();
    params.coupon_rate = 5.12345678912345;
    params.face_value = Some(1000.625);
    let bond = create_bond(&params).unwrap();
    assert_eq!(
        bond.coupon_rate(),
        Decimal::from_str("0.0512345678912345").unwrap()
    );
    assert_eq!(bond.face_value(), Decimal::from_str("1000.625").unwrap());
    for kind in [
        "CorporateFRN",
        "GovernmentFRN",
        "TreasuryBill",
        "InflationLinked",
        "Convertible",
        "MBS",
        "ABS",
        "CommercialPaper",
        "Strips",
    ] {
        params.instrument_type = Some(kind.into());
        assert!(create_bond(&params).is_err());
    }
}

#[test]
fn explicit_quote_and_settlement_overrides_survive_market_selection() {
    let mut params = fixed();
    params.market = Some("US".into());
    params.instrument_type = Some("GovernmentBond".into());
    params.compounding = Some("Annual".into());
    params.yield_convention = Some("True".into());
    params.settlement_days = Some(3);
    params.use_business_days = Some(false);
    let rules = get_yield_rules(&params).unwrap();
    assert_eq!(
        rules.compounding,
        convex_bonds::types::CompoundingMethod::Periodic { frequency: 1 }
    );
    assert_eq!(
        rules.convention,
        convex_bonds::types::YieldConvention::TrueYield
    );
    assert_eq!(rules.settlement_rules.days, 3);
    assert!(!rules.settlement_rules.use_business_days);
}

#[test]
fn original_face_is_the_absolute_redemption_cash_in_this_fixed_convenience() {
    let mut params = fixed();
    params.face_value = Some(1000.625);
    let bond = create_bond(&params).unwrap();
    let cash = bond
        .try_cash_flows(parse_date("2026-04-20").unwrap())
        .unwrap();
    assert_eq!(
        cash.last().unwrap().amount,
        Decimal::from_str("1025.640625").unwrap()
    );
}
