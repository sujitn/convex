//! Independent complete-cash quotation and same-convention risk controls.
use convex_analytics::{
    functions,
    yas::{YASCalculator, YASResult},
};
use convex_bonds::{types::CalendarId, FixedRateBond};
use convex_core::{
    calendars::BusinessDayConvention, daycounts::DayCountConvention, types::Frequency, Date,
};
use convex_curves::{curves::ZeroCurveBuilder, ZeroCurve};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use std::str::FromStr;

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/yas-quote-owner.json")).unwrap()
}
fn date(text: &str) -> Date {
    Date::parse(text).unwrap()
}
fn decimal(row: &Value, key: &str) -> Decimal {
    Decimal::from_str(row[key].as_str().unwrap()).unwrap()
}
fn frequency(count: u32) -> Frequency {
    match count {
        1 => Frequency::Annual,
        2 => Frequency::SemiAnnual,
        4 => Frequency::Quarterly,
        12 => Frequency::Monthly,
        _ => panic!("invalid test quote frequency"),
    }
}
fn bond(row: &Value) -> FixedRateBond {
    let dc = match row["day_count"].as_str().unwrap() {
        "Thirty360US" => DayCountConvention::Thirty360US,
        "Act365Fixed" => DayCountConvention::Act365Fixed,
        "ActActIcma" => DayCountConvention::ActActIcma,
        _ => panic!("invalid test day count"),
    };
    FixedRateBond::builder()
        .cusip_unchecked("YAS-COMPLETE-CASH")
        .issue_date(date("2025-01-15"))
        .maturity(date("2035-01-15"))
        .face_value(decimal(row, "face"))
        .redemption_value(decimal(row, "face"))
        .coupon_rate(dec!(0.05))
        .frequency(Frequency::SemiAnnual)
        .day_count(dc)
        .calendar(CalendarId::weekend_only())
        .business_day_convention(BusinessDayConvention::Unadjusted)
        .end_of_month(false)
        .build()
        .unwrap()
}
fn curve() -> ZeroCurve {
    ZeroCurveBuilder::new()
        .reference_date(date("2025-01-15"))
        .add_rate(date("2025-04-15"), dec!(0.04))
        .add_rate(date("2035-01-15"), dec!(0.04))
        .build()
        .unwrap()
}
fn close(actual: Decimal, expected: Decimal, bound: Decimal, label: &str) {
    assert!(
        (actual - expected).abs() <= bound,
        "{label}: {actual} vs {expected}, bound {bound}"
    );
}
fn metrics(result: &YASResult) -> Value {
    json!({"annual_yield": (result.ytm / dec!(100)).to_string(),
        "macaulay_duration": result.risk.macaulay_duration.years().to_string(),
        "modified_duration": result.modified_duration().to_string(),
        "convexity": result.convexity().to_string(),
        "dv01_per_100": result.dv01().to_string(), "dv01": result.risk.dv01.value().to_string()})
}

#[test]
fn complete_cash_independent_yield_and_risk_match_all_quote_frequencies() {
    let pack = reference();
    let curve = curve();
    let settle = date("2025-04-15");
    let mut actual = vec![];
    for row in pack["cases"].as_array().unwrap() {
        let bond = bond(row);
        let count = row["frequency"].as_u64().unwrap() as u32;
        let result = YASCalculator::new(&curve)
            .with_frequency(count)
            .analyze(&bond, settle.into(), decimal(row, "clean"))
            .unwrap();
        let observed = metrics(&result);
        for key in [
            "annual_yield",
            "macaulay_duration",
            "modified_duration",
            "convexity",
            "dv01_per_100",
            "dv01",
        ] {
            let tolerance = if key == "annual_yield" {
                "annual_yield"
            } else if key.starts_with("dv01") {
                "dv01"
            } else {
                "risk"
            };
            close(
                decimal(&observed, key),
                decimal(row, key),
                decimal(&pack["bounds"], tolerance),
                key,
            );
        }
        let repriced = functions::dirty_price_from_yield(
            &bond,
            settle,
            (result.ytm / dec!(100)).to_f64().unwrap(),
            frequency(count),
        )
        .unwrap();
        close(
            Decimal::from_f64_retain(repriced).unwrap(),
            decimal(row, "dirty"),
            decimal(&pack["bounds"], "price"),
            "full cash reprice",
        );
        close(
            result.invoice.accrued_interest,
            decimal(row, "accrued"),
            decimal(&pack["bounds"], "price"),
            "actual accrued",
        );
        actual.push(json!({"id":row["id"],"metrics":observed,"dirty_reprice":repriced}));
    }
    assert_eq!(actual.len(), 72);
    println!(
        "{}",
        json!({"kind":"complete_yas_quote_results","cases":actual})
    );
}

#[test]
fn configured_quote_frequency_changes_the_yield_and_risk_consistently() {
    let pack = reference();
    let row = &pack["cases"][0];
    let bond = bond(row);
    let curve = curve();
    let settle = date("2025-04-15");
    let mut outputs = vec![];
    for count in [1, 2, 4, 12] {
        let result = YASCalculator::new(&curve)
            .with_frequency(count)
            .analyze(&bond, settle.into(), decimal(row, "clean"))
            .unwrap();
        let canonical =
            functions::yield_to_maturity(&bond, settle, decimal(row, "clean"), frequency(count))
                .unwrap();
        close(
            result.ytm / dec!(100),
            Decimal::from_f64_retain(canonical.yield_value).unwrap(),
            dec!(0.00000000000001),
            "canonical quote owner",
        );
        outputs.push((result.ytm, result.modified_duration()));
    }
    assert!(outputs
        .windows(2)
        .all(|pair| pair[0].0 != pair[1].0 && pair[0].1 != pair[1].1));
}

#[test]
fn invalid_quote_counts_fail_before_any_financial_result() {
    let pack = reference();
    let bond = bond(&pack["cases"][0]);
    let curve = curve();
    for count in [0, 3, 6, 365, u32::MAX] {
        let error = YASCalculator::new(&curve)
            .with_frequency(count)
            .analyze(&bond, date("2025-04-15").into(), dec!(100))
            .unwrap_err();
        assert!(error.to_string().contains("quote frequency"));
    }
}

#[test]
fn checked_cash_accrual_and_dirty_price_failures_propagate() {
    let pack = reference();
    let bond = bond(&pack["cases"][0]);
    let curve = curve();
    let calculator = YASCalculator::new(&curve);
    for settle in ["2024-12-31", "2035-01-15", "2036-01-15"] {
        assert!(calculator
            .analyze(&bond, date(settle).into(), dec!(100))
            .is_err());
    }
    assert!(calculator
        .analyze(&bond, date("2025-04-15").into(), Decimal::MAX)
        .is_err());
    assert!(calculator
        .analyze(&bond, date("2025-04-15").into(), dec!(-100))
        .is_err());
    assert_eq!(bond.try_cash_flows(date("2025-04-15")).unwrap().len(), 20);
}
