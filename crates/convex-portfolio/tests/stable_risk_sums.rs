//! Exact supplied-risk controls with independently declared Fraction references.
use convex_analytics::risk::{Duration, KeyRateDuration, KeyRateDurations};
use convex_bonds::types::BondIdentifiers;
use convex_portfolio::aggregate_key_rate_exposure;
use convex_portfolio::types::{AnalyticsConfig, Holding, HoldingAnalytics, WeightingMethod};
use rust_decimal::Decimal;
use serde_json::{json, Value};

fn decimal(value: &Value) -> Decimal {
    Decimal::from_str_exact(value.as_str().unwrap()).unwrap()
}

fn holdings(rows: &Value) -> Vec<Holding> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let mut analytics = HoldingAnalytics::new();
            analytics.key_rate_durations = Some(KeyRateDurations::new(
                row["factors"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(tenor, duration)| KeyRateDuration {
                        tenor: tenor.parse().unwrap(),
                        duration: Duration::new(decimal(duration)),
                    })
                    .collect(),
            ));
            let mut holding = Holding::builder()
                .id(row["id"].as_str().unwrap())
                .identifiers(BondIdentifiers::new())
                .par_amount(decimal(&row["par"]))
                .market_price(decimal(&row["clean"]))
                .analytics(analytics)
                .build()
                .unwrap();
            holding.accrued_interest = decimal(&row["accrued"]);
            holding.fx_rate = decimal(&row["fx"]);
            holding
        })
        .collect()
}

#[test]
fn exact_fraction_books_preserve_signed_residuals_and_declared_decimal_rounding() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/stable-risk-sums.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 105);
    let mut outputs = Vec::new();
    let (mut successes, mut rejections) = (0, 0);
    for case in cases {
        let holdings = holdings(&case["holdings"]);
        let config = AnalyticsConfig {
            weighting: match case["weighting"].as_str().unwrap() {
                "MarketValue" => WeightingMethod::MarketValue,
                "ParValue" => WeightingMethod::ParValue,
                "EqualWeight" => WeightingMethod::EqualWeight,
                other => panic!("undeclared weighting {other}"),
            },
            ..AnalyticsConfig::default()
        };
        let tenors: Vec<f64> = case["tenors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().parse().unwrap())
            .collect();
        let result = aggregate_key_rate_exposure(&holdings, &config, Some(&tenors));
        if case["expected"] == "reject" {
            assert!(result.is_err(), "{}: {result:?}", case["id"]);
            outputs.push(json!({"id":case["id"],"rejected":true}));
            rejections += 1;
            continue;
        }
        let result = result.unwrap_or_else(|error| panic!("{}: {error}", case["id"]));
        let expected = &case["result"];
        assert_eq!(
            result.complete_holdings,
            expected["complete_holdings"].as_u64().unwrap() as usize
        );
        assert_eq!(
            result.total_holdings,
            expected["total_holdings"].as_u64().unwrap() as usize
        );
        let expected_points = expected["factors"].as_array().unwrap();
        assert_eq!(result.factors.len(), expected_points.len());
        for (point, wanted) in result.factors.iter().zip(expected_points) {
            assert_eq!(
                point.tenor.to_bits(),
                wanted["tenor"]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap()
                    .to_bits()
            );
            assert_eq!(
                point.duration.map(|d| d.years()),
                wanted["duration"]
                    .as_str()
                    .map(|_| decimal(&wanted["duration"]))
            );
            assert_eq!(
                point.known_partial_dv01,
                wanted["known_partial_dv01"]
                    .as_str()
                    .map(|_| decimal(&wanted["known_partial_dv01"]))
            );
            assert_eq!(
                point.covered_market_value,
                decimal(&wanted["covered_market_value"])
            );
            assert_eq!(
                point.covered_holdings,
                wanted["covered_holdings"].as_u64().unwrap() as usize
            );
            assert_eq!(
                point.total_holdings,
                wanted["total_holdings"].as_u64().unwrap() as usize
            );
        }
        if let Some(total) = expected["total_duration_decimal"].as_str() {
            let exact = Decimal::from_str_exact(total).unwrap();
            assert_eq!(
                result.profile.as_ref().unwrap().total_duration.to_bits(),
                exact.as_f64().to_bits()
            );
        } else {
            assert!(result.profile.is_none());
        }
        outputs.push(json!({"id":case["id"],"rejected":false,"actual":result}));
        successes += 1;
    }
    assert_eq!(successes, 72);
    assert_eq!(rejections, 33);
    println!("EXACT_STABLE_RISK_SUMS={}", json!(outputs));
}

#[test]
fn cancellation_repair_keeps_existing_work_bounds_and_missing_factor_identity() {
    use convex_portfolio::analytics::{MAX_KEY_RATE_FACTORS, MAX_KEY_RATE_HOLDINGS};
    let input = json!([{ "id":"one", "par":"1", "clean":"100", "accrued":"0", "fx":"1", "factors":{"5":"1"}}]);
    let one = holdings(&input);
    assert!(aggregate_key_rate_exposure(&one, &AnalyticsConfig::default(), Some(&[])).is_err());
    let too_many_tenors: Vec<f64> = (1..=MAX_KEY_RATE_FACTORS + 1).map(|n| n as f64).collect();
    assert!(
        aggregate_key_rate_exposure(&one, &AnalyticsConfig::default(), Some(&too_many_tenors))
            .is_err()
    );
    let too_many_holdings = vec![one[0].clone(); MAX_KEY_RATE_HOLDINGS + 1];
    assert!(aggregate_key_rate_exposure(
        &too_many_holdings,
        &AnalyticsConfig::default(),
        Some(&[5.0])
    )
    .is_err());
    let exposure = aggregate_key_rate_exposure(
        &one,
        &AnalyticsConfig::default(),
        Some(&[5.0, f64::from_bits(5.0f64.to_bits() + 1)]),
    )
    .unwrap();
    assert_eq!(exposure.factors[0].covered_holdings, 1);
    assert_eq!(exposure.factors[1].covered_holdings, 0);
    assert!(exposure.factors[1].duration.is_none());
    assert!(exposure.factors[1].known_partial_dv01.is_none());
}
