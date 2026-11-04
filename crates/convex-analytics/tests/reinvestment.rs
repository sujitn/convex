//! Actual unchecked projection/short-date failures, before candidate execution.
use convex_analytics::yields::{
    constant_rate_return_with_horizon as money_market_yield_with_horizon, ShortDateCalculator,
};
use convex_bonds::pricing::reinvestment::{self, ProjectionCash, ProjectionTerms};
use convex_bonds::traits::BondCashFlow;
use convex_core::contracts::ExactDecimal;
use convex_core::types::Date;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use std::str::FromStr;

fn date(s: &str) -> Date {
    Date::parse(s).unwrap()
}
fn cash() -> Vec<BondCashFlow> {
    vec![BondCashFlow::coupon_and_principal(
        date("2026-01-01"),
        dec!(5),
        dec!(100),
    )]
}
#[test]
fn zero_reporting_basis_returns_an_error_without_panicking() {
    let actual = std::panic::catch_unwind(|| {
        money_market_yield_with_horizon(
            &cash(),
            dec!(100),
            dec!(0.05),
            date("2025-01-01"),
            date("2026-01-01"),
            2,
            0,
            180,
        )
    });
    assert!(actual.is_ok(), "zero reporting basis panicked");
    assert!(actual.unwrap().is_err());
}
#[test]
fn zero_frequency_cannot_silently_remove_the_supplied_model_rate() {
    assert!(money_market_yield_with_horizon(
        &cash(),
        dec!(100),
        dec!(0.05),
        date("2025-01-01"),
        date("2026-01-01"),
        0,
        365,
        180
    )
    .is_err());
}
#[test]
fn invalid_power_domain_cannot_become_unit_growth() {
    assert!(money_market_yield_with_horizon(
        &cash(),
        dec!(100),
        dec!(-3),
        date("2025-01-01"),
        date("2026-01-01"),
        2,
        365,
        30
    )
    .is_err());
}
#[test]
fn rolled_cash_overflow_is_a_result_instead_of_a_panic() {
    let actual = std::panic::catch_unwind(|| {
        money_market_yield_with_horizon(
            &cash(),
            Decimal::MAX,
            dec!(0.5),
            date("2025-01-01"),
            date("2026-01-01"),
            2,
            365,
            180,
        )
    });
    assert!(actual.is_ok(), "rolled cash overflow panicked");
    assert!(actual.unwrap().is_err());
}
#[test]
fn missing_terminal_cash_cannot_close_the_remaining_claim_at_zero() {
    let cash = [BondCashFlow::coupon(date("2025-07-15"), dec!(2.5))];
    assert!(money_market_yield_with_horizon(
        &cash,
        dec!(100),
        dec!(0.05),
        date("2025-01-01"),
        date("2026-01-01"),
        2,
        365,
        365
    )
    .is_err());
}
#[test]
fn unordered_cash_cannot_roll_the_account_backwards() {
    let cash = [
        BondCashFlow::coupon(date("2025-07-15"), dec!(2.5)),
        BondCashFlow::coupon(date("2025-03-15"), dec!(2.5)),
        BondCashFlow::principal(date("2026-01-01"), dec!(100)),
    ];
    assert!(money_market_yield_with_horizon(
        &cash,
        dec!(100),
        dec!(0.05),
        date("2025-01-01"),
        date("2026-01-01"),
        2,
        365,
        300
    )
    .is_err());
}
#[test]
fn expired_short_date_queries_are_explicit_errors() {
    let actual = ShortDateCalculator::years_to_maturity(date("2026-01-01"), date("2025-01-01"));
    assert!(
        format!("{actual:?}").starts_with("Err("),
        "expired short-date query accepted: {actual:?}"
    );
}

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/reinvestment.json")).unwrap()
}
fn decimal(value: &Value) -> Decimal {
    Decimal::from_str(value.as_str().unwrap()).unwrap()
}
#[test]
fn native_projection_matches_predeclared_decimal110_cash_and_rate_references() {
    let root = reference();
    assert_eq!(root["cases"].as_array().unwrap().len(), 74);
    for row in root["cases"].as_array().unwrap() {
        let terms:ProjectionTerms=serde_json::from_value(json!({
            "settlement":row["settlement"],"maturity":row["maturity"],"initial_cash_value":row["initial_cash_value"],
            "valuation_rate":row["valuation_rate"],"reinvestment_rate":row["reinvestment_rate"],"frequency":row["frequency"],
            "reporting_days_per_year":row["reporting_days_per_year"],"max_horizon_days":row["max_horizon_days"]})).unwrap();
        let before = serde_json::to_value(terms).unwrap();
        let rebuilt: ProjectionTerms = serde_json::from_value(before.clone()).unwrap();
        assert_eq!(serde_json::to_value(rebuilt).unwrap(), before);
        let cash: Vec<_> = row["cash"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| ProjectionCash {
                date: date(c[0].as_str().unwrap()),
                amount: ExactDecimal::new(decimal(&c[1])),
            })
            .collect();
        let actual =
            reinvestment::project(&cash, &terms).unwrap_or_else(|e| panic!("{}: {e}", row["id"]));
        assert_eq!(actual.horizon, date(row["horizon"].as_str().unwrap()));
        for (key, value) in [
            ("residual_balance", actual.residual_balance),
            ("reinvested_cash", actual.reinvested_cash),
            ("horizon_value", actual.horizon_value),
            ("annualized_return", actual.annualized_return),
        ] {
            let bound = decimal(
                &row[if key == "annualized_return" {
                    "rate_absolute_bound"
                } else {
                    "cash_absolute_bound"
                }],
            );
            assert!(
                (value.decimal() - decimal(&row["comparison"][key])).abs() <= bound,
                "{} {key}: {value:?}",
                row["id"]
            );
        }
    }
}
#[test]
fn native_growth_matches_declared_factors_with_zero_and_signed_rates() {
    let root = reference();
    assert_eq!(root["growth_cases"].as_array().unwrap().len(), 12);
    for row in root["growth_cases"].as_array().unwrap() {
        let actual = reinvestment::periodic_growth(
            decimal(&row["annual_rate"]),
            row["frequency"].as_u64().unwrap() as u32,
            date(row["start"].as_str().unwrap()),
            date(row["end"].as_str().unwrap()),
        )
        .unwrap();
        assert!(
            (actual - decimal(&row["comparison"])).abs() <= decimal(&row["absolute_bound"]),
            "{}: {actual}",
            row["id"]
        );
    }
}

#[test]
fn native_projection_enforces_cash_capacity_before_financial_arithmetic() {
    let reference = reference();
    let row = &reference["cases"][0];
    let terms: ProjectionTerms = serde_json::from_value(json!({
        "settlement":row["settlement"],"maturity":row["maturity"],
        "initial_cash_value":row["initial_cash_value"],"valuation_rate":row["valuation_rate"],
        "reinvestment_rate":row["reinvestment_rate"],"frequency":row["frequency"],
        "reporting_days_per_year":row["reporting_days_per_year"],"max_horizon_days":row["max_horizon_days"]
    })).unwrap();
    let cash = vec![
        ProjectionCash {
            date: date(row["cash"][0][0].as_str().unwrap()),
            amount: ExactDecimal::new(decimal(&row["cash"][0][1])),
        };
        4097
    ];
    assert!(reinvestment::project(&cash, &terms).is_err());
}
#[test]
fn short_date_native_and_analytics_use_actual_flags_and_validate_even_when_disabled() {
    use convex_bonds::{pricing::ShortDateCalculator as Native, types::YieldCalculationRules};
    let mut rules = YieldCalculationRules::us_treasury();
    rules.use_short_date_method = false;
    let native = Native::from_rules(&rules).unwrap();
    let analytics = ShortDateCalculator::from_rules(&rules).unwrap();
    assert!(!native.enabled());
    assert!(!analytics.enabled());
    assert!(!native.use_money_market_below(0.01).unwrap());
    assert!(!analytics.use_money_market_below(0.01).unwrap());
    assert!(!native.is_short_dated(0.5).unwrap());
    assert!(!analytics.is_short_dated(0.5).unwrap());
    for years in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
        assert!(native.use_money_market_below(years).is_err());
        assert!(analytics.use_money_market_below(years).is_err());
        assert!(native.is_short_dated(years).is_err());
        assert!(analytics.is_short_dated(years).is_err());
    }
    for (money, short) in [
        (f64::NAN, 1.0),
        (0.1, f64::INFINITY),
        (-1.0, 1.0),
        (2.0, 1.0),
    ] {
        assert!(Native::new(money, short).is_err());
        assert!(ShortDateCalculator::new(money, short).is_err());
    }
}
