//! Independent scalar risk contracts, declared before candidate execution.
use convex_analytics::risk::var::MAX_HISTORICAL_OBSERVATIONS;
use convex_analytics::risk::*;
use convex_math::stats::standard_normal_inverse;
use rust_decimal::Decimal;
use serde_json::{json, Value};

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/risk-contracts.json")).unwrap()
}
fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}
fn close(actual: f64, expected: f64, absolute: f64, relative: f64) {
    assert!(
        actual.is_finite() && (actual - expected).abs() <= absolute.max(expected.abs() * relative),
        "actual={actual:?}, expected={expected:?}"
    );
}
fn duration(value: i32) -> Duration {
    Duration::new(value.into())
}
fn dv01(value: i32) -> DV01 {
    DV01::new(value.into())
}

#[test]
fn conversions_never_replace_invalid_or_unrepresentable_input_with_zero() {
    for value in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        -f64::MAX,
        1e-100,
        -1e-100,
    ] {
        assert!(Duration::try_from(value).is_err());
        assert!(Convexity::try_from(value).is_err());
        assert!(DV01::try_from(value).is_err());
    }
    for value in [0.0, -0.0, -5.0, 1.5, 5.0] {
        assert_eq!(Duration::try_from(value).unwrap().as_f64(), value);
        assert_eq!(Convexity::try_from(value).unwrap().as_f64(), value);
        assert_eq!(DV01::try_from(value).unwrap().as_f64(), value);
    }
    for value in [
        Decimal::MIN,
        Decimal::MAX,
        Decimal::ZERO,
        Decimal::new(1, 28),
    ] {
        assert_eq!(Duration::new(value).as_f64(), value.as_f64());
        assert_eq!(Convexity::new(value).as_f64(), value.as_f64());
        assert_eq!(DV01::new(value).as_f64(), value.as_f64());
    }
}

#[test]
fn hedge_ratios_match_independent_signed_rational_cases() {
    let pack = reference();
    let mut actual = Vec::new();
    for (id, row) in pack["hedges"].as_array().unwrap().iter().enumerate() {
        let units = duration_hedge_ratio(
            number(&row["duration"]),
            number(&row["value"]),
            number(&row["hedge_duration"]),
            number(&row["hedge_price"]),
        )
        .unwrap();
        close(
            units,
            number(&row["units"]),
            number(&pack["bounds"]["hedge_absolute"]),
            number(&pack["bounds"]["hedge_relative"]),
        );
        actual.push(json!({"id": id, "units": units}));
    }
    assert_eq!(dv01_hedge_ratio(dv01(1000), dv01(50)).unwrap(), -20.0);
    println!(
        "{}",
        json!({"kind": "checked_hedge_contracts", "cases": actual})
    );
}

#[test]
fn hedge_admission_cannot_hide_nonfinite_or_unresolved_arithmetic() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for slot in 0..4 {
            let mut inputs = [5.0, 10_000_000.0, 7.5, 100.0];
            inputs[slot] = bad;
            assert!(duration_hedge_ratio(inputs[0], inputs[1], inputs[2], inputs[3]).is_err());
        }
    }
    for denominator in [0.0, -0.0, 1e-12, -1e-12] {
        assert!(duration_hedge_ratio(5.0, 10_000_000.0, denominator, 100.0).is_err());
        assert!(duration_hedge_ratio(5.0, 10_000_000.0, 7.5, denominator).is_err());
    }
    assert!(duration_hedge_ratio(f64::MAX, f64::MAX, 7.5, 100.0).is_err());
    assert!(duration_hedge_ratio(f64::MIN_POSITIVE, f64::MIN_POSITIVE, 7.5, 100.0).is_err());
    assert!(dv01_hedge_ratio(dv01(1), dv01(0)).is_err());
    assert_eq!(duration_hedge_ratio(0.0, 100.0, 7.5, 100.0).unwrap(), 0.0);
}

#[test]
fn normal_and_parametric_loss_quantiles_match_independent_reference() {
    let pack = reference();
    let mut normal = Vec::new();
    let mut parametric = Vec::new();
    for (id, row) in pack["normal"]
        .as_array()
        .unwrap()
        .iter()
        .chain(pack["extreme_normal"].as_array().unwrap().iter())
        .enumerate()
    {
        let inverse = standard_normal_inverse(number(&row["confidence"])).unwrap();
        close(
            inverse,
            number(&row["inverse"]),
            number(&pack["bounds"]["normal_inverse_absolute"]),
            0.0,
        );
        normal.push(json!({"id": id, "inverse": inverse}));
    }
    for (id, row) in pack["parametric"].as_array().unwrap().iter().enumerate() {
        let (exposure, vol, confidence, days) = (
            number(&row["exposure"]),
            number(&row["volatility"]),
            number(&row["confidence"]),
            row["horizon"].as_u64().unwrap() as u32,
        );
        let values = [
            parametric_var(exposure, vol, confidence, days).unwrap().var,
            parametric_var_from_dv01(exposure, vol, confidence, days)
                .unwrap()
                .var,
        ];
        for value in values {
            close(
                value.as_f64(),
                number(&row["loss_quantile"]),
                number(&pack["bounds"]["var_absolute"]),
                number(&pack["bounds"]["var_relative"]),
            );
        }
        parametric.push(json!({"id": id, "values": values}));
    }
    println!(
        "{}",
        json!({"kind": "checked_parametric_contracts", "normal": normal, "parametric": parametric})
    );
}

#[test]
fn historical_quantiles_preserve_actual_signed_losses_and_exact_ranks() {
    let pack = reference();
    let mut actual = Vec::new();
    for (id, row) in pack["historical"].as_array().unwrap().iter().enumerate() {
        let returns: Vec<_> = row["returns"]
            .as_array()
            .unwrap()
            .iter()
            .map(number)
            .collect();
        let result = historical_var(
            &returns,
            number(&row["exposure"]),
            number(&row["confidence"]),
            row["horizon"].as_u64().unwrap() as u32,
        )
        .unwrap();
        close(
            result.var.as_f64(),
            number(&row["loss_quantile"]),
            number(&pack["bounds"]["var_absolute"]),
            number(&pack["bounds"]["var_relative"]),
        );
        actual.push(json!({"id": id, "loss": result.var}));
    }
    println!(
        "{}",
        json!({"kind": "checked_historical_contracts", "cases": actual})
    );
}

#[test]
fn var_admission_rejects_invalid_inputs_and_enforces_bounded_history() {
    for confidence in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -0.1,
        0.0,
        1.0,
        2.0,
    ] {
        assert!(standard_normal_inverse(confidence).is_err());
        assert!(parametric_var(1e6, 0.01, confidence, 1).is_err());
        assert!(parametric_var_from_dv01(1000.0, 5.0, confidence, 1).is_err());
        assert!(historical_var(&[-0.02, 0.01], 1e6, confidence, 1).is_err());
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(parametric_var(bad, 0.01, 0.95, 1).is_err());
        assert!(parametric_var(0.0, bad, 0.95, 1).is_err());
        assert!(parametric_var_from_dv01(bad, 0.01, 0.95, 1).is_err());
        assert!(historical_var(&[bad], 0.0, 0.95, 1).is_err());
        assert!(historical_var(&[0.0], bad, 0.95, 1).is_err());
    }
    assert!(parametric_var(1e6, -0.01, 0.95, 1).is_err());
    assert!(parametric_var(1e6, 0.01, 0.95, 0).is_err());
    assert!(historical_var(&[], 1e6, 0.95, 1).is_err());
    assert!(historical_var(&[0.0], 1e6, 0.95, 0).is_err());
    assert!(historical_var(&vec![0.0; MAX_HISTORICAL_OBSERVATIONS + 1], 1e6, 0.95, 1).is_err());
    assert!(parametric_var(f64::MAX, f64::MAX, 0.95, 1).is_err());
    assert!(parametric_var(1e-100, 0.01, 0.95, 1).is_err());
    assert_eq!(
        historical_var(&[0.0], 1e6, f64::from_bits(1), 1)
            .unwrap()
            .var,
        Decimal::ZERO
    );
}

#[test]
fn portfolio_preserves_signed_totals_and_distinguishes_undefined_duration() {
    let regular = aggregate_portfolio_risk(&[
        Position::new("A", 1e6, duration(5), dv01(500)),
        Position::new("B", 2e6, duration(3), dv01(600)),
    ])
    .unwrap();
    assert_eq!(regular.market_value, Decimal::from(3_000_000));
    assert_eq!(regular.total_dv01, dv01(1100));
    close(
        regular.weighted_duration.unwrap().as_f64(),
        11.0 / 3.0,
        1e-12,
        0.0,
    );
    let flat = aggregate_portfolio_risk(&[
        Position::new("A", 100.0, duration(5), dv01(5)),
        Position::new("B", -100.0, duration(3), dv01(-3)),
    ])
    .unwrap();
    assert_eq!(flat.market_value, Decimal::ZERO);
    assert_eq!(flat.total_dv01, dv01(2));
    assert!(flat.weighted_duration.is_none());
    let empty = aggregate_portfolio_risk(&[]).unwrap();
    assert!(empty.weighted_duration.is_none());
    assert_eq!(empty.market_value, Decimal::ZERO);
    assert_eq!(empty.total_dv01, dv01(0));
    let short =
        aggregate_portfolio_risk(&[Position::new("S", -100.0, duration(7), dv01(-3))]).unwrap();
    assert_eq!(short.weighted_duration, Some(duration(7)));
    let at_guard =
        aggregate_portfolio_risk(&[Position::new("G", 1e-10, duration(7), dv01(0))]).unwrap();
    assert!(at_guard.weighted_duration.is_none());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, 1e-100] {
        assert!(
            aggregate_portfolio_risk(&[Position::new("A", value, duration(5), dv01(1))]).is_err()
        );
    }
    let position = Position::new("A", 100.0, duration(5), dv01(1));
    assert!(aggregate_portfolio_risk(&[position.clone(), position]).is_err());
    assert!(aggregate_portfolio_risk(&[Position::new("", 100.0, duration(5), dv01(1))]).is_err());
}

#[test]
fn nearby_factors_are_exact_and_invalid_collections_have_no_match() {
    let rows = vec![
        KeyRateDuration {
            tenor: 2.0,
            duration: duration(1),
        },
        KeyRateDuration {
            tenor: 2.0005,
            duration: duration(7),
        },
    ];
    let rates = KeyRateDurations::new(rows.clone());
    assert_eq!(rates.at_tenor(2.0005).unwrap().duration, duration(7));
    assert_eq!(rates.at_tenor(2.0).unwrap().duration, duration(1));
    assert!(rates.at_tenor(2.0005000001).is_none());
    for tenor in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
        assert!(rates.at_tenor(tenor).is_none());
    }
    let ambiguous = KeyRateDurations::new(vec![rows[0].clone(), rows[0].clone()]);
    assert!(ambiguous.at_tenor(2.0).is_none());
    assert!(KeyRateDurations::new(vec![rows[0].clone(); 4097])
        .at_tenor(2.0)
        .is_none());
}
