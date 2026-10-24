//! Independent rational/Decimal90 risk, original units and explicit failures.
use convex_analytics::risk::*;
use convex_bonds::{pricing::risk_math, traits::BondAnalytics, types::CalendarId, FixedRateBond};
use convex_core::{
    calendars::BusinessDayConvention,
    daycounts::DayCountConvention,
    types::{Date, Frequency},
};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/risk-stencils.json")).unwrap()
}
fn number(row: &Value, key: &str) -> f64 {
    row[key].as_str().unwrap().parse().unwrap()
}
fn near(actual: f64, expected: f64, bound: f64) {
    assert!(
        actual.is_finite() && (actual - expected).abs() <= bound,
        "actual={actual:?} expected={expected:?} bound={bound:?}"
    );
}
fn duration(row: &Value) -> Duration {
    Duration::new(Decimal::from_str(row["duration"].as_str().unwrap()).unwrap())
}
fn dated_bond(row: &Value) -> FixedRateBond {
    let face = Decimal::from_str(row["face"].as_str().unwrap()).unwrap();
    let day_count = match row["day_count"].as_str().unwrap() {
        "Thirty360US" => DayCountConvention::Thirty360US,
        "Act365Fixed" => DayCountConvention::Act365Fixed,
        "ActActIcma" => DayCountConvention::ActActIcma,
        _ => panic!("declared dated convention"),
    };
    FixedRateBond::builder()
        .cusip_unchecked("RISK-METHODS")
        .coupon_rate("0.05".parse().unwrap())
        .face_value(face)
        .redemption_value(face)
        .issue_date(Date::parse("2025-01-15").unwrap())
        .maturity(Date::parse("2035-01-15").unwrap())
        .frequency(Frequency::SemiAnnual)
        .day_count(day_count)
        .calendar(CalendarId::weekend_only())
        .business_day_convention(BusinessDayConvention::Unadjusted)
        .end_of_month(false)
        .build()
        .unwrap()
}

#[test]
fn independent_scaled_signed_stencils_use_one_native_owner() {
    let pack = reference();
    let mut actual = Vec::new();
    for row in pack["stencils"].as_array().unwrap() {
        let (up, down, base, bump) = (
            number(row, "price_up"),
            number(row, "price_down"),
            number(row, "price_base"),
            number(row, "bump"),
        );
        let effective = EffectiveDurationCalculator::new(bump * 10_000.0).unwrap();
        let key = KeyRateDurationCalculator::with_tenors(vec![5.0])
            .unwrap()
            .with_bump_bps(bump * 10_000.0)
            .unwrap()
            .calculate(base, &[(5.0, up, down)])
            .unwrap();
        let values = [
            risk_math::duration_stencil(up, down, base, bump).unwrap(),
            effective_duration(up, down, base, bump).unwrap().as_f64(),
            spread_duration(up, down, base, bump).unwrap().as_f64(),
            key_rate_duration_at_tenor(up, down, base, bump, 5.0)
                .unwrap()
                .duration
                .as_f64(),
            effective.from_prices(base, up, down).unwrap().as_f64(),
            key.durations[0].duration.as_f64(),
            risk_math::convexity_stencil(up, down, base, bump).unwrap(),
            effective_convexity(up, down, base, bump).unwrap().as_f64(),
            effective
                .convexity_from_prices(base, up, down)
                .unwrap()
                .as_f64(),
        ];
        for value in &values[..6] {
            near(
                *value,
                number(row, "duration"),
                number(&pack["bounds"], "stencil_duration"),
            );
        }
        for value in &values[6..] {
            near(
                *value,
                number(row, "convexity"),
                number(&pack["bounds"], "stencil_convexity"),
            );
        }
        actual.push(json!({"id":row["id"],"values":values}));
    }
    assert_eq!(actual.len(), 96);
    println!(
        "{}",
        json!({"kind":"checked_risk_stencil_results","cases":actual})
    );
}

#[test]
fn independent_dv01_and_defined_inverse_keep_signed_zero_exposures() {
    let pack = reference();
    let mut actual = Vec::new();
    for row in pack["dv01"].as_array().unwrap() {
        let (price, face) = (number(row, "price"), number(row, "face"));
        let d = duration(row);
        let values = [
            risk_math::dv01(d.as_f64(), price, face).unwrap(),
            dv01_from_duration(d, price, face).unwrap().as_f64(),
        ];
        for value in values {
            near(value, number(row, "dv01"), number(&pack["bounds"], "dv01"));
        }
        let target = DV01::new(Decimal::from_str(row["inverse_target"].as_str().unwrap()).unwrap());
        let native = risk_math::notional_from_dv01(target.as_f64(), d.as_f64(), price);
        let analytics = notional_from_dv01(target, d, price);
        let inverse = if row["inverse_defined"].as_bool().unwrap() {
            let values = [native.unwrap(), analytics.unwrap()];
            for value in values {
                near(
                    value,
                    number(row, "inverse_face"),
                    number(&pack["bounds"], "inverse_notional"),
                );
            }
            Some(values)
        } else {
            assert!(native.is_err() && analytics.is_err());
            None
        };
        actual.push(json!({"id":row["id"],"dv01":values,"inverse":inverse}));
    }
    assert_eq!(actual.len(), 27);
    println!("{}", json!({"kind":"checked_dv01_results","cases":actual}));
}

#[test]
fn independent_price_effects_keep_actual_first_and_second_order_units() {
    let pack = reference();
    let mut actual = Vec::new();
    for row in pack["price_effects"].as_array().unwrap() {
        let d = duration(row);
        let c = Convexity::new(Decimal::from_str(row["convexity"].as_str().unwrap()).unwrap());
        let (price, change) = (number(row, "price"), number(row, "yield_change"));
        let metrics = BondRiskMetrics {
            macaulay_duration: d,
            modified_duration: d,
            convexity: c,
            dv01: DV01::new(Decimal::ZERO),
            dv01_per_100: DV01::new(Decimal::ZERO),
        };
        let values = [
            risk_math::duration_price_change(d.as_f64(), price, change).unwrap(),
            price_change_from_duration(d, price, change).unwrap(),
            risk_math::convexity_adjustment(c.as_f64(), change).unwrap(),
            metrics.convexity_adjustment(change).unwrap(),
            risk_math::price_change(d.as_f64(), c.as_f64(), price, change).unwrap(),
            price_change_with_convexity(d.as_f64(), c.as_f64(), price, change).unwrap(),
            metrics.estimate_price_change(change, price).unwrap(),
        ];
        for value in &values[..2] {
            near(
                *value,
                number(row, "first_order"),
                number(&pack["bounds"], "price_change"),
            );
        }
        for value in &values[2..4] {
            near(
                *value,
                number(row, "convexity_adjustment"),
                number(&pack["bounds"], "convexity_adjustment"),
            );
        }
        for value in &values[4..] {
            near(
                *value,
                number(row, "total_price_change"),
                number(&pack["bounds"], "price_change"),
            );
        }
        actual.push(json!({"id":row["id"],"values":values}));
    }
    assert_eq!(actual.len(), 81);
    println!(
        "{}",
        json!({"kind":"checked_price_effect_results","cases":actual})
    );
}

#[test]
fn key_rate_keeps_its_original_smaller_nonzero_bump_domain() {
    let pack = reference();
    let part = &pack["key_rate_small_bumps"];
    let mut actual = Vec::new();
    for row in part["cases"].as_array().unwrap() {
        let (up, down, base, bump, tenor) = (
            number(row, "price_up"),
            number(row, "price_down"),
            number(row, "price_base"),
            number(row, "bump"),
            number(row, "tenor"),
        );
        let key = KeyRateDurationCalculator::with_tenors(vec![tenor])
            .unwrap()
            .with_bump_bps(bump * 10_000.0)
            .unwrap()
            .calculate(base, &[(tenor, up, down)])
            .unwrap();
        let values = [
            risk_math::duration_stencil(up, down, base, bump).unwrap(),
            key_rate_duration_at_tenor(up, down, base, bump, tenor)
                .unwrap()
                .duration
                .as_f64(),
            key.durations[0].duration.as_f64(),
        ];
        let expected = number(row, "duration");
        let bound = number(part, "absolute_duration_bound")
            .max(expected.abs() * number(part, "relative_duration_bound"));
        for value in values {
            near(value, expected, bound);
        }
        assert!(effective_duration(up, down, base, bump).is_err());
        assert!(spread_duration(up, down, base, bump).is_err());
        actual.push(json!({"id":row["id"],"values":values}));
    }
    assert_eq!(actual.len(), 4);
    println!(
        "{}",
        json!({"kind":"checked_small_key_rate_results","cases":actual})
    );
}

#[test]
fn original_dated_bond_references_own_native_risk_and_per100_notional_scaling() {
    let pack: Value = serde_json::from_str(include_str!("fixtures/yas-quote-owner.json")).unwrap();
    let settle = Date::parse("2025-04-15").unwrap();
    let mut actual = Vec::new();
    for row in pack["cases"].as_array().unwrap() {
        let bond = dated_bond(row);
        let freq = match row["frequency"].as_u64().unwrap() {
            1 => Frequency::Annual,
            2 => Frequency::SemiAnnual,
            4 => Frequency::Quarterly,
            12 => Frequency::Monthly,
            _ => panic!("declared quote frequency"),
        };
        let (y, price) = (number(row, "annual_yield"), number(row, "dirty"));
        let values = [
            bond.macaulay_duration(settle, y, freq).unwrap(),
            bond.modified_duration(settle, y, freq).unwrap(),
            bond.convexity(settle, y, freq).unwrap(),
            bond.dv01(settle, y, price, freq).unwrap(),
            bond.dv01_notional(settle, y, price, 1_000_000.0, freq)
                .unwrap(),
            bond.dv01_notional(settle, y, price, -2_000_000.0, freq)
                .unwrap(),
        ];
        for (value, key) in
            values[..3]
                .iter()
                .zip(["macaulay_duration", "modified_duration", "convexity"])
        {
            near(*value, number(row, key), number(&pack["bounds"], "risk"));
        }
        let dv = number(row, "dv01_per_100");
        let tol = number(&pack["bounds"], "dv01");
        near(values[3], dv, tol);
        near(values[4], dv * 10_000.0, tol * 10_000.0);
        near(values[5], dv * (-20_000.0), tol * 20_000.0);
        actual.push(json!({"id":row["id"],"values":values}));
    }
    assert_eq!(actual.len(), 72);
    println!(
        "{}",
        json!({"kind":"checked_native_bond_risk_results","cases":actual})
    );
}

#[test]
fn nonfinite_inputs_never_become_successful_zero_or_nonfinite_risk() {
    let d = Duration::new(5.into());
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for index in 0..4 {
            let mut args = [99.0, 101.0, 100.0, 0.001];
            args[index] = bad;
            assert!(effective_duration(args[0], args[1], args[2], args[3]).is_err());
            assert!(spread_duration(args[0], args[1], args[2], args[3]).is_err());
            assert!(effective_convexity(args[0], args[1], args[2], args[3]).is_err());
            assert!(key_rate_duration_at_tenor(args[0], args[1], args[2], args[3], 5.0).is_err());
        }
        assert!(key_rate_duration_at_tenor(99.0, 101.0, 100.0, 0.001, bad).is_err());
        assert!(dv01_from_prices(bad, 100.0).is_err());
        assert!(dv01_from_prices(100.0, bad).is_err());
        assert!(dv01_from_duration(d, bad, 0.0).is_err());
        assert!(dv01_from_duration(d, 100.0, bad).is_err());
        assert!(price_change_from_duration(d, 100.0, bad).is_err());
        for index in 0..4 {
            let mut args = [5.0, 50.0, 100.0, 0.01];
            args[index] = bad;
            assert!(price_change_with_convexity(args[0], args[1], args[2], args[3]).is_err());
        }
        assert!(EffectiveDurationCalculator::new(bad).is_err());
        assert!(KeyRateDurationCalculator::standard()
            .with_bump_bps(bad)
            .is_err());
    }
}

#[test]
fn configured_key_rate_tenors_are_complete_unique_bounded_and_consumed() {
    for tenors in [
        vec![],
        vec![0.0],
        vec![-1.0],
        vec![f64::NAN],
        vec![f64::INFINITY],
        vec![2.0, 2.0],
        (1..=4097).map(f64::from).collect(),
    ] {
        assert!(KeyRateDurationCalculator::with_tenors(tenors).is_err());
    }
    let calc = KeyRateDurationCalculator::with_tenors(vec![2.0, 5.0]).unwrap();
    for rows in [
        vec![],
        vec![(10.0, 99.0, 101.0)],
        vec![(2.0, 99.0, 101.0), (2.0, 99.0, 101.0)],
        vec![(2.0, 99.0, 101.0), (10.0, 99.0, 101.0)],
    ] {
        assert!(calc.calculate(100.0, &rows).is_err());
    }
    let result = calc
        .calculate(100.0, &[(5.0, 99.0, 101.0), (2.0, 99.5, 100.5)])
        .unwrap();
    assert_eq!(
        result
            .durations
            .iter()
            .map(|row| row.tenor)
            .collect::<Vec<_>>(),
        vec![2.0, 5.0]
    );
    near(result.total_duration().unwrap().as_f64(), 150.0, 1e-10);
    let overflow = KeyRateDurations::new(vec![
        KeyRateDuration {
            tenor: 2.0,
            duration: Duration::new(Decimal::MAX),
        },
        KeyRateDuration {
            tenor: 5.0,
            duration: Duration::new(Decimal::MAX),
        },
    ]);
    assert!(overflow.total_duration().is_err());
    assert_eq!(
        KeyRateDurations::new(vec![])
            .total_duration()
            .unwrap()
            .years(),
        Decimal::ZERO
    );
}

#[test]
fn genuine_zero_effects_survive_and_undefined_or_underflowing_operations_fail() {
    assert!(effective_duration(99.0, 101.0, 100.0, 0.0).is_err());
    assert!(effective_convexity(99.0, 101.0, 100.0, 0.0).is_err());
    assert!(key_rate_duration_at_tenor(99.0, 101.0, 100.0, 0.0, 5.0).is_err());
    assert!(effective_duration(99.0, 101.0, 1e-11, 0.001).is_err());
    assert!(dv01_from_duration(
        Duration::new(1.into()),
        f64::MIN_POSITIVE,
        f64::MIN_POSITIVE
    )
    .is_err());
    assert_eq!(
        dv01_from_duration(Duration::new(1.into()), 100.0, 0.0)
            .unwrap()
            .value(),
        Decimal::ZERO
    );
    assert_eq!(
        price_change_with_convexity(0.0, 0.0, 0.0, f64::MAX).unwrap(),
        0.0
    );
    assert!(
        notional_from_dv01(DV01::new(1000.into()), Duration::new(Decimal::ZERO), 100.0).is_err()
    );
    assert!(EffectiveDurationCalculator::new(0.0).is_err());
    assert!(EffectiveDurationCalculator::new(1e-10).is_err());
    assert!(KeyRateDurationCalculator::standard()
        .with_bump_bps(0.0)
        .is_err());
}

#[test]
fn original_native_bond_nonfinite_and_unresolved_bump_failures_are_explicit() {
    let pack = reference();
    let row = &pack["native_notional"]["case"];
    let bond = dated_bond(row);
    let settle = Date::parse("2025-04-15").unwrap();
    let y = number(row, "annual_yield");
    let price = number(row, "dirty");
    near(
        bond.dv01_notional(settle, y, price, 1_000_000.0, Frequency::SemiAnnual)
            .unwrap(),
        number(&pack["native_notional"], "expected_notional_dv01"),
        1e-6,
    );
    assert!(bond
        .estimate_price_change(settle, y, price, f64::NAN, Frequency::SemiAnnual)
        .is_err());
    assert!(bond
        .dv01(settle, y, f64::INFINITY, Frequency::SemiAnnual)
        .is_err());
    for bump in [0.0, f64::NAN, f64::INFINITY, 1e-30] {
        assert!(bond
            .effective_duration(settle, y, Frequency::SemiAnnual, bump)
            .is_err());
        assert!(bond
            .effective_convexity(settle, y, Frequency::SemiAnnual, bump)
            .is_err());
    }
    assert!(bond
        .modified_duration(settle, -11.9999, Frequency::Monthly)
        .is_err());
    assert!(bond
        .convexity(settle, -11.9999, Frequency::Monthly)
        .is_err());
}
