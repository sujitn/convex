//! Independent supplied-factor aggregation and unavailable-data controls.
use convex_analytics::risk::{Duration, KeyRateDuration, KeyRateDurations};
use convex_bonds::types::BondIdentifiers;
use convex_portfolio::analytics::{
    aggregate_key_rate_exposure, MAX_KEY_RATE_FACTORS, MAX_KEY_RATE_HOLDINGS,
};
use convex_portfolio::types::{AnalyticsConfig, Holding, HoldingAnalytics, WeightingMethod};
use convex_portfolio::{aggregate_key_rate_profile, partial_dv01s};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

fn decimal(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}
fn input(value: &Value) -> Holding {
    let rows = value["risk"].as_array().map(|rows| {
        KeyRateDurations::new(
            rows.iter()
                .map(|row| KeyRateDuration {
                    tenor: row[0].as_str().unwrap().parse().unwrap(),
                    duration: Duration::new(decimal(row[1].as_str().unwrap())),
                })
                .collect(),
        )
    });
    let mut analytics = HoldingAnalytics::new();
    analytics.key_rate_durations = rows;
    Holding::builder()
        .id(value["id"].as_str().unwrap())
        .identifiers(BondIdentifiers::new().with_ticker("SUPPLIED-FIXTURE"))
        .par_amount(decimal(value["par"].as_str().unwrap()))
        .market_price(decimal(value["price"].as_str().unwrap()))
        .accrued_interest(decimal(value["accrued"].as_str().unwrap_or("0")))
        .fx_rate(decimal(value["fx"].as_str().unwrap()))
        .analytics(analytics)
        .build()
        .unwrap()
}
fn simple(id: &str, price: &str, rows: Value) -> Holding {
    input(&json!({"id":id,"par":"100","price":price,"fx":"1","risk":rows}))
}
fn close(actual: f64, expected: &str) {
    let expected: f64 = expected.parse().unwrap();
    assert!(
        actual.is_finite() && (actual - expected).abs() <= 1e-12f64.max(expected.abs() * 1e-14),
        "{actual} != {expected}"
    );
}

#[test]
fn all_independent_rational_factor_cases_match_known_currency_exposure() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/holding-key-rates.json")).unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 48);
    let mut actual_rows = Vec::new();
    let mut admission_rejections = 0;
    for case in cases {
        let holdings: Vec<_> = case["holdings"]
            .as_array()
            .unwrap()
            .iter()
            .map(input)
            .collect();
        let tenors: Vec<f64> = case["requested_tenors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().parse().unwrap())
            .collect();
        let weighting = match case["weighting"].as_str().unwrap() {
            "MarketValue" => WeightingMethod::MarketValue,
            "ParValue" => WeightingMethod::ParValue,
            "EqualWeight" => WeightingMethod::EqualWeight,
            _ => panic!("undeclared weight"),
        };
        let config = AnalyticsConfig {
            weighting,
            ..AnalyticsConfig::default()
        };
        let result = aggregate_key_rate_exposure(&holdings, &config, Some(&tenors));
        if case["admission_error"].is_string() {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("positive dirty valuation basis"));
            admission_rejections += 1;
            continue;
        }
        let actual = result.unwrap();
        let expected = case["factors"].as_array().unwrap();
        assert_eq!(actual.factors.len(), expected.len());
        for (point, expected) in actual.factors.iter().zip(expected) {
            assert_eq!(
                point.tenor.to_bits(),
                expected["tenor"]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap()
                    .to_bits()
            );
            match expected["duration"].as_str() {
                Some(value) => close(point.duration.unwrap().as_f64(), value),
                None => assert!(point.duration.is_none()),
            }
            match expected["known_partial_dv01"].as_str() {
                Some(value) => close(point.known_partial_dv01.unwrap().as_f64(), value),
                None => assert!(point.known_partial_dv01.is_none()),
            }
            close(
                point.covered_market_value.as_f64(),
                expected["covered_market_value"].as_str().unwrap(),
            );
            assert_eq!(
                point.covered_holdings,
                expected["covered_holdings"].as_u64().unwrap() as usize
            );
            assert_eq!(
                point.total_holdings,
                expected["total_holdings"].as_u64().unwrap() as usize
            );
            assert_eq!(point.is_complete(), expected["complete"].as_bool().unwrap());
            actual_rows.push(json!({"case_id":case["id"],"tenor":point.tenor,"duration":point.duration.map(|v|v.as_f64()),"known_partial_dv01":point.known_partial_dv01.map(|v|v.as_f64()),"covered_market_value":point.covered_market_value.as_f64(),"covered_holdings":point.covered_holdings,"total_holdings":point.total_holdings,"complete":point.is_complete()}));
        }
    }
    assert_eq!(actual_rows.len(), 69);
    assert_eq!(admission_rejections, 3);
    println!("HOLDING_KEY_RATE_ADMISSION_REJECTIONS={admission_rejections}");
    println!("HOLDING_KEY_RATE_REFERENCE={}", json!(actual_rows));
}

#[test]
fn original_nearby_missing_weighting_and_zero_value_failures_are_repaired() {
    let config = AnalyticsConfig::default();
    let inputs = [simple("A", "100", json!([["2", "1"], ["2.0005", "7"]]))];
    let profile = aggregate_key_rate_profile(&inputs, &config, Some(&[2.0, 2.0005])).unwrap();
    assert_eq!(profile.at_tenor(2.0), Some(1.0));
    assert_eq!(profile.at_tenor(2.0005), Some(7.0));
    let missing = [
        simple("A", "100", json!([["5", "5"]])),
        simple("B", "100", Value::Null),
    ];
    close(
        partial_dv01s(&missing, &config, Some(&[5.0])).unwrap()[0].1,
        "0.05",
    );
    let unequal = [
        simple("A", "100", json!([["5", "1"]])),
        simple("B", "200", json!([["5", "9"]])),
    ];
    for weighting in [
        WeightingMethod::MarketValue,
        WeightingMethod::ParValue,
        WeightingMethod::EqualWeight,
    ] {
        let config = AnalyticsConfig {
            weighting,
            ..config.clone()
        };
        close(
            partial_dv01s(&unequal, &config, Some(&[5.0])).unwrap()[0].1,
            "0.19",
        );
    }
    let zero = [simple("A", "0", json!([["5", "5"]]))];
    assert!(aggregate_key_rate_profile(&zero, &config, Some(&[5.0])).is_none());
    assert!(aggregate_key_rate_exposure(&zero, &config, Some(&[5.0])).is_err());
}

#[test]
fn absent_factors_remain_distinct_from_explicit_zero_and_zero_sum() {
    let inputs = [simple(
        "A",
        "100",
        json!([["2", "0"], ["5", "3"], ["10", "-3"]]),
    )];
    let result = aggregate_key_rate_exposure(
        &inputs,
        &AnalyticsConfig::default(),
        Some(&[2.0, 5.0, 10.0, 30.0]),
    )
    .unwrap();
    assert_eq!(result.factors[0].duration.unwrap().as_f64(), 0.0);
    assert_eq!(result.factors[0].known_partial_dv01, Some(Decimal::ZERO));
    assert!(result.factors[3].duration.is_none());
    assert!(result.factors[3].known_partial_dv01.is_none());
    assert!(!result.factors[3].is_complete());
    let profile = result.profile.unwrap();
    assert_eq!(profile.total_duration, 0.0);
    assert!(profile.at_tenor(30.0).is_none());
}

#[test]
fn invalid_full_input_sets_fail_without_silently_dropping_rows() {
    let config = AnalyticsConfig::default();
    let valid = simple("A", "100", json!([["5", "1"]]));
    for tenors in [
        vec![],
        vec![0.0],
        vec![-1.0],
        vec![f64::NAN],
        vec![f64::INFINITY],
        vec![5.0, 5.0],
        vec![5.0; MAX_KEY_RATE_FACTORS + 1],
    ] {
        assert!(
            aggregate_key_rate_exposure(std::slice::from_ref(&valid), &config, Some(&tenors))
                .is_err()
        );
    }
    assert!(
        aggregate_key_rate_exposure(&[valid.clone(), valid.clone()], &config, Some(&[5.0]))
            .is_err()
    );
    for field in [
        "empty-id",
        "negative-price",
        "zero-fx",
        "negative-par",
        "bad-extra-factor",
        "duplicate-extra-factor",
    ] {
        let mut invalid = valid.clone();
        match field {
            "empty-id" => invalid.id.clear(),
            "negative-price" => invalid.market_price = -Decimal::ONE,
            "zero-fx" => invalid.fx_rate = Decimal::ZERO,
            "negative-par" => invalid.par_amount = -Decimal::ONE,
            "bad-extra-factor" => invalid
                .analytics
                .key_rate_durations
                .as_mut()
                .unwrap()
                .durations
                .push(KeyRateDuration {
                    tenor: f64::NAN,
                    duration: Duration::new(Decimal::ONE),
                }),
            _ => invalid
                .analytics
                .key_rate_durations
                .as_mut()
                .unwrap()
                .durations
                .extend([
                    KeyRateDuration {
                        tenor: 7.0,
                        duration: Duration::new(Decimal::ONE),
                    },
                    KeyRateDuration {
                        tenor: 7.0,
                        duration: Duration::new(Decimal::ONE),
                    },
                ]),
        }
        assert!(
            aggregate_key_rate_exposure(&[invalid], &config, Some(&[5.0])).is_err(),
            "{field}"
        );
    }
    let mut profile =
        aggregate_key_rate_profile(std::slice::from_ref(&valid), &config, Some(&[5.0])).unwrap();
    profile.durations.push(profile.durations[0].clone());
    assert!(profile.at_tenor(5.0).is_none());
    let too_many = vec![valid; MAX_KEY_RATE_HOLDINGS + 1];
    assert!(aggregate_key_rate_exposure(&too_many, &config, Some(&[5.0])).is_err());
}

#[test]
fn decimal_range_and_nonzero_underflow_fail_without_panics() {
    let config = AnalyticsConfig::default();
    let mut overflow = simple("A", "100", json!([["5", "1"]]));
    overflow.par_amount = Decimal::MAX;
    overflow.market_price = Decimal::MAX;
    assert!(aggregate_key_rate_exposure(&[overflow], &config, Some(&[5.0])).is_err());
    let mut tiny = simple("A", "100", json!([["5", "1"]]));
    tiny.par_amount = decimal("0.0000000000000000000000000001");
    tiny.market_price = decimal("0.1");
    assert!(aggregate_key_rate_exposure(&[tiny], &config, Some(&[5.0])).is_err());
}

#[test]
fn configured_order_serialization_and_parallel_flags_preserve_exact_inputs() {
    let inputs = [simple("A", "100", json!([["2", "1"], ["2.0005", "7"]]))];
    let config = AnalyticsConfig::default().with_key_rate_tenors(vec![2.0005, 2.0]);
    let expected = aggregate_key_rate_exposure(&inputs, &config, None).unwrap();
    assert_eq!(
        expected.factors.iter().map(|p| p.tenor).collect::<Vec<_>>(),
        vec![2.0005, 2.0]
    );
    let parallel = AnalyticsConfig {
        parallel: true,
        parallel_threshold: 0,
        ..config.clone()
    };
    assert_eq!(
        serde_json::to_value(&expected).unwrap(),
        serde_json::to_value(aggregate_key_rate_exposure(&inputs, &parallel, None).unwrap())
            .unwrap()
    );
    let actual: convex_portfolio::analytics::KeyRateExposure =
        serde_json::from_value(serde_json::to_value(&expected).unwrap()).unwrap();
    assert_eq!(
        actual.factors[0].tenor.to_bits(),
        expected.factors[0].tenor.to_bits()
    );
    let explicit = aggregate_key_rate_exposure(&inputs, &config, Some(&[2.0])).unwrap();
    assert_eq!(explicit.factors.len(), 1);
    assert_eq!(explicit.factors[0].tenor, 2.0);
}
