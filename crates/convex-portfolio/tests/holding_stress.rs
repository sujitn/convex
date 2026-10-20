//! Independent supplied-economics and explicit unavailable-result controls.
use convex_analytics::risk::{Duration, KeyRateDuration, KeyRateDurations};
use convex_bonds::types::BondIdentifiers;
use convex_core::{Currency, Date};
use convex_portfolio::stress::*;
use convex_portfolio::types::{
    CashPosition, Classification, CreditRating, Holding, HoldingAnalytics, RatingInfo, Sector,
    SectorInfo,
};
use convex_portfolio::Portfolio;
use rust_decimal::Decimal;
use serde_json::Value;

fn decimal(value: &Value) -> Decimal {
    Decimal::from_str_exact(value.as_str().unwrap()).unwrap()
}
fn number(value: &Value) -> Option<f64> {
    value.as_str().map(|v| v.parse().unwrap())
}
fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/holding-stress.json")).unwrap()
}
fn inputs(value: &Value) -> (Portfolio, StressScenario, StressPolicy) {
    let date = Date::from_ymd(2025, 1, 15).unwrap();
    let holdings = value["holdings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let mut analytics = HoldingAnalytics::new();
            analytics.modified_duration = number(&row["duration"]);
            analytics.convexity = number(&row["convexity"]);
            analytics.effective_duration = number(&row["effective_duration"]);
            analytics.effective_convexity = number(&row["effective_convexity"]);
            analytics.spread_duration = number(&row["spread_duration"]);
            if let Some(rows) = row["key_rates"].as_array() {
                analytics.key_rate_durations = Some(KeyRateDurations::new(
                    rows.iter()
                        .map(|r| KeyRateDuration {
                            tenor: number(&r[0]).unwrap(),
                            duration: Duration::new(decimal(&r[1])),
                        })
                        .collect(),
                ));
            }
            let mut classification = Classification::new();
            if let Some(rating) = row["rating"].as_str() {
                classification.rating =
                    RatingInfo::from_composite(CreditRating::parse(rating).unwrap());
            }
            if let Some(sector) = row["sector"].as_str() {
                classification.sector = SectorInfo::from_composite(match sector {
                    "Corporate" => Sector::Corporate,
                    "Financial" => Sector::Financial,
                    _ => panic!("unknown independently declared reference sector"),
                });
            }
            Holding {
                id: row["id"].as_str().unwrap().into(),
                identifiers: BondIdentifiers::new(),
                par_amount: decimal(&row["par"]),
                market_price: decimal(&row["clean"]),
                accrued_interest: decimal(&row["accrued"]),
                fx_rate: decimal(&row["fx"]),
                currency: Currency::from_code(row["currency"].as_str().unwrap()).unwrap(),
                analytics,
                classification,
            }
        })
        .collect();
    let portfolio = Portfolio {
        id: "independent-stress-book".into(),
        name: "supplied exact economics".into(),
        base_currency: Currency::USD,
        as_of_date: date,
        holdings,
        cash: value["cash"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                CashPosition::with_fx_rate(
                    decimal(&c["amount"]),
                    Currency::from_code(c["currency"].as_str().unwrap()).unwrap(),
                    decimal(&c["fx"]),
                )
            })
            .collect(),
        shares_outstanding: None,
        liabilities: Some(decimal(&value["liabilities"])),
    };
    let mut scenario = StressScenario::new(value["id"].as_str().unwrap());
    if let Some(rate) = number(&value["rate_bps"]) {
        scenario.rate_scenario = Some(RateScenario::parallel(rate));
    }
    if let Some(rows) = value["rate_shifts"].as_array() {
        scenario.rate_scenario = Some(RateScenario::key_rates(
            &rows
                .iter()
                .map(|r| (number(&r[0]).unwrap(), number(&r[1]).unwrap()))
                .collect::<Vec<_>>(),
        ));
    }
    if let Some(spread) = number(&value["spread_bps"]) {
        scenario.spread_scenario = Some(SpreadScenario::Uniform(spread));
    }
    for (key, rating) in [("spread_by_rating", true), ("spread_by_sector", false)] {
        if let Some(map) = value[key].as_object() {
            let shifts = map
                .iter()
                .map(|(k, v)| (k.clone(), number(v).unwrap()))
                .collect();
            scenario.spread_scenario = Some(if rating {
                SpreadScenario::ByRating(shifts)
            } else {
                SpreadScenario::BySector(shifts)
            });
        }
    }
    let policy = StressPolicy {
        rate_basis: match value["basis"].as_str().unwrap() {
            "modified_yield" => StressRateBasis::ModifiedYield,
            "effective_curve" => StressRateBasis::EffectiveCurve,
            "key_rate_curve" => StressRateBasis::KeyRateCurve,
            _ => panic!("invalid reference basis"),
        },
        rate_order: if value["order"] == "first_order" {
            StressOrder::FirstOrder
        } else {
            StressOrder::SecondOrder
        },
        key_rate_tenors: value["tenors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| number(t).unwrap())
            .collect(),
        fx: value["fx_pins"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pin| StressFxPin {
                from_currency: Currency::from_code(pin["from_currency"].as_str().unwrap()).unwrap(),
                to_currency: Currency::from_code(pin["to_currency"].as_str().unwrap()).unwrap(),
                as_of_date: if pin["as_of_date"] == "2025-01-15" {
                    date
                } else {
                    Date::from_ymd(2025, 1, 16).unwrap()
                },
                rate: decimal(&pin["rate"]),
            })
            .collect(),
    };
    (portfolio, scenario, policy)
}

#[test]
fn independent_fraction_books_preserve_actual_amounts_and_typed_failures() {
    let reference = reference();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 64);
    let mut observations = Vec::new();
    let mut available = 0;
    for case in cases {
        let (book, scenario, policy) = inputs(case);
        let result = run_stress_scenario(&book, &scenario, &policy);
        match case["expected_status"].as_str().unwrap() {
            "available" => {
                let result = result.unwrap_or_else(|error| panic!("{}: {error}", case["id"]));
                let e = &case["expected"];
                for (field, actual) in [
                    ("initial_value", result.initial_value),
                    ("stressed_value", result.stressed_value),
                    ("pnl", result.pnl),
                    ("rate_pnl", result.rate_impact.unwrap_or(Decimal::ZERO)),
                    ("spread_pnl", result.spread_impact.unwrap_or(Decimal::ZERO)),
                ] {
                    assert_eq!(actual, decimal(&e[field]), "{} {field}", case["id"]);
                }
                assert_eq!(
                    result.pnl_pct,
                    e["pnl_pct"]
                        .as_str()
                        .map(|s| Decimal::from_str_exact(s).unwrap()),
                    "{} percentage",
                    case["id"]
                );
                assert_eq!(
                    result.rate_impact.is_some(),
                    scenario.rate_scenario.is_some()
                );
                assert_eq!(
                    result.spread_impact.is_some(),
                    scenario.spread_scenario.is_some()
                );
                assert_eq!(result.currency, Currency::USD);
                observations.push(
                    serde_json::json!({"id":case["id"],"status":"available","result":result}),
                );
                available += 1;
            }
            expected => {
                let error = result.unwrap_err();
                assert_eq!(
                    serde_json::to_value(error.code).unwrap(),
                    expected,
                    "{} {error}",
                    case["id"]
                );
                observations
                    .push(serde_json::json!({"id":case["id"],"status":expected,"error":error}));
            }
        }
    }
    assert_eq!(available, 36);
    println!(
        "EXACT_HOLDING_STRESS={}",
        serde_json::to_string(&observations).unwrap()
    );
}

#[test]
fn batches_preserve_unavailable_and_zero_nav_in_summary_counts() {
    let references = reference();
    let base = &references["cases"][0];
    let (mut book, mut up, policy) = inputs(base);
    book.holdings[0].analytics.spread_duration = None;
    up.name = "up".into();
    let down = StressScenario::new("down").with_rate_scenario(RateScenario::parallel(-100.0));
    let missing =
        StressScenario::new("missing").with_spread_scenario(SpreadScenario::Uniform(50.0));
    let results =
        run_stress_scenarios(&book, &[up.clone(), down.clone(), missing.clone()], &policy).unwrap();
    let summary = summarize_results(&results).unwrap();
    assert_eq!(
        (
            summary.scenario_count,
            summary.available_count,
            summary.unavailable_count,
            summary.percentage_count
        ),
        (3, 2, 1, 2)
    );
    assert_eq!(summary.worst.unwrap().pnl, Decimal::from(-47500));
    assert_eq!(summary.best.unwrap().pnl, Decimal::from(52500));
    assert_eq!(summary.avg_pnl, Some(Decimal::from(2500)));
    assert_eq!(summary.avg_pnl_pct, Some(Decimal::new(25, 2)));
    book.liabilities = Some(Decimal::from(1000000));
    let results = run_stress_scenarios(&book, &[up, down, missing], &policy).unwrap();
    let summary = summarize_results(&results).unwrap();
    assert_eq!(summary.percentage_count, 0);
    assert_eq!(summary.avg_pnl_pct, None);
    assert_eq!(summary.avg_pnl, Some(Decimal::from(2500)));
    let results = run_stress_scenarios(
        &book,
        &[StressScenario::new("missing-only").with_spread_scenario(SpreadScenario::Uniform(50.0))],
        &policy,
    )
    .unwrap();
    let summary = summarize_results(&results).unwrap();
    assert_eq!((summary.available_count, summary.unavailable_count), (0, 1));
    assert!(summary.worst.is_none() && summary.best.is_none() && summary.avg_pnl.is_none());

    // Equal identifiers, NAV, dates and counts do not identify the same actual book.
    let (original, scenario, policy) = inputs(base);
    let original_result = run_stress_scenario(&original, &scenario, &policy).unwrap();
    let mut changed = original.clone();
    changed.holdings[0].analytics.modified_duration = Some(6.0);
    let changed_result = run_stress_scenario(&changed, &scenario, &policy).unwrap();
    assert_eq!(original_result.initial_value, changed_result.initial_value);
    assert!(summarize_results(&[
        StressOutcome::Available {
            result: original_result.clone()
        },
        StressOutcome::Available {
            result: changed_result
        },
    ])
    .is_err());
    // Retain actual foreign FX policy even at unit parity and identical monetary NAV.
    let mut foreign = original.clone();
    foreign.holdings[0].currency = Currency::EUR;
    let mut foreign_policy = policy.clone();
    foreign_policy.fx.push(StressFxPin {
        from_currency: Currency::EUR,
        to_currency: Currency::USD,
        as_of_date: foreign.as_of_date,
        rate: Decimal::ONE,
    });
    let foreign_result = run_stress_scenario(&foreign, &scenario, &foreign_policy).unwrap();
    assert_eq!(original_result.initial_value, foreign_result.initial_value);
    assert!(summarize_results(&[
        StressOutcome::Available {
            result: original_result
        },
        StressOutcome::Available {
            result: foreign_result
        },
    ])
    .is_err());
    // Exact factors are policy, even when the supplied monetary effects coincide.
    let mut factor_book = original.clone();
    factor_book.holdings[0].analytics.key_rate_durations = Some(KeyRateDurations::new(vec![
        KeyRateDuration {
            tenor: 2.0,
            duration: Duration::new(Decimal::ONE),
        },
        KeyRateDuration {
            tenor: 5.0,
            duration: Duration::new(Decimal::ONE),
        },
    ]));
    let mut factor_policy = policy.clone();
    factor_policy.rate_basis = StressRateBasis::KeyRateCurve;
    factor_policy.rate_order = StressOrder::FirstOrder;
    factor_policy.key_rate_tenors = vec![2.0];
    let two = run_stress_scenario(&factor_book, &scenario, &factor_policy).unwrap();
    factor_policy.key_rate_tenors = vec![5.0];
    let five = run_stress_scenario(&factor_book, &scenario, &factor_policy).unwrap();
    assert_eq!(two.pnl, five.pnl);
    assert!(summarize_results(&[
        StressOutcome::Available { result: two },
        StressOutcome::Available { result: five },
    ])
    .is_err());
    // Unavailable outcomes also retain their original complete native context.
    let absent = StressScenario::new("absent").with_spread_scenario(SpreadScenario::Uniform(50.0));
    let mut a = original.clone();
    a.holdings[0].analytics.spread_duration = None;
    let first = run_stress_scenarios(&a, std::slice::from_ref(&absent), &policy).unwrap();
    a.holdings[0].analytics.modified_duration = Some(7.0);
    let second = run_stress_scenarios(&a, &[absent], &policy).unwrap();
    assert!(summarize_results(&[first[0].clone(), second[0].clone()]).is_err());
}

#[test]
fn declared_work_and_identity_bounds_reject_without_partial_success() {
    let references = reference();
    let (mut book, scenario, mut policy) = inputs(&references["cases"][0]);
    assert!(run_stress_scenarios(&book, &[], &policy).is_err());
    assert!(run_stress_scenarios(
        &book,
        &vec![scenario.clone(); MAX_STRESS_SCENARIOS + 1],
        &policy
    )
    .is_err());
    assert!(run_stress_scenarios(&book, &[scenario.clone(), scenario.clone()], &policy).is_err());
    policy.key_rate_tenors = (1..=4097).map(f64::from).collect();
    assert!(run_stress_scenario(&book, &scenario, &policy).is_err());
    policy.key_rate_tenors.clear();
    book.cash = vec![CashPosition::new(Decimal::ZERO, Currency::USD); MAX_STRESS_ROWS];
    assert!(run_stress_scenario(&book, &scenario, &policy).is_err());
    assert!(summarize_results(&[]).unwrap().avg_pnl.is_none());
}

#[test]
fn shapes_require_exact_factors_and_explicit_bounded_interpolation() {
    let references: Value =
        serde_json::from_str(include_str!("fixtures/holding-stress-shapes.json")).unwrap();
    let discrete = RateScenario::key_rates(&[(2.0, 50.0), (10.0, 100.0)]);
    assert_eq!(discrete.shift_at_tenor(2.0).unwrap(), 50.0);
    assert_eq!(discrete.shift_at_tenor(10.0).unwrap(), 100.0);
    assert!(discrete.shift_at_tenor(6.0).is_err());
    let expected = references["explicit_linear_no_extrapolation"]["at_6"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!((discrete.interpolated_shift_at_tenor(6.0).unwrap() - expected).abs() <= 1e-12);
    assert!(discrete.interpolated_shift_at_tenor(1.0).is_err());
    assert!(discrete.interpolated_shift_at_tenor(11.0).is_err());
    let actual = RateScenario::steepening_2s10s(-50.0, 50.0);
    for (tenor, expected) in [(2.0, -50.0), (5.0, 0.0), (10.0, 50.0)] {
        assert_eq!(actual.shift_at_tenor(tenor).unwrap(), expected);
    }
    let shape = RateScenario::Steepening {
        short_shift: -50.0,
        long_shift: 50.0,
        pivot_tenor: 5.0,
    };
    for (tenor, expected) in [(2.0, -30.0), (5.0, 0.0), (10.0, 10.0), (30.0, 50.0)] {
        assert!((shape.shift_at_tenor(tenor).unwrap() - expected).abs() <= 1e-12);
    }
    assert!(shape.shift_at_tenor(31.0).is_err());
    for bad in [
        RateScenario::parallel(f64::NAN),
        RateScenario::parallel(f64::INFINITY),
        RateScenario::key_rates(&[]),
        RateScenario::key_rates(&[(2.0, 50.0), (2.0, 100.0)]),
        RateScenario::Steepening {
            short_shift: -50.0,
            long_shift: 50.0,
            pivot_tenor: 0.0,
        },
        RateScenario::Flattening {
            short_shift: 50.0,
            long_shift: -50.0,
            pivot_tenor: 30.0,
        },
        RateScenario::Butterfly {
            wing_shift: 25.0,
            belly_shift: -25.0,
            belly_start: 5.0,
            belly_end: 3.0,
        },
    ] {
        assert!(bad.validate().is_err());
    }
    assert!(discrete.shift_at_tenor(f64::NAN).is_err());
    assert!(discrete.shift_at_tenor(-1.0).is_err());
    assert!(SpreadScenario::by_rating(&[("AAA", 25.0), ("AAA", 50.0)]).is_err());
    assert!(SpreadScenario::by_sector(&[("Corporate", f64::NAN)]).is_err());
    assert!(SpreadScenario::by_sector(&[]).is_err());
}

#[test]
fn native_exposure_json_preserves_exact_duration_and_money_under_all_features() {
    use convex_portfolio::types::AnalyticsConfig;
    use convex_portfolio::{aggregate_key_rate_exposure, KeyRateExposure};
    let references = reference();
    let (mut book, _, _) = inputs(&references["cases"][0]);
    let maximum = Decimal::MAX;
    book.holdings[0].par_amount =
        Decimal::from_str_exact("0.0000000000000000000000000001").unwrap();
    book.holdings[0].analytics.key_rate_durations =
        Some(KeyRateDurations::new(vec![KeyRateDuration {
            tenor: 5.0,
            duration: Duration::new(maximum),
        }]));
    let result =
        aggregate_key_rate_exposure(&book.holdings, &AnalyticsConfig::default(), Some(&[5.0]))
            .unwrap();
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["factors"][0]["duration"], maximum.to_string());
    assert_eq!(
        json["profile"]["durations"][0]["duration"],
        maximum.to_string()
    );
    assert!(json["factors"][0]["known_partial_dv01"].is_string());
    assert!(json["factors"][0]["covered_market_value"].is_string());
    let decoded: KeyRateExposure = serde_json::from_value(json).unwrap();
    assert_eq!(decoded.factors[0].duration.unwrap().years(), maximum);
    assert_eq!(
        decoded.profile.unwrap().durations[0].duration.years(),
        maximum
    );
    let malformed = serde_json::json!({"rate_basis":"modified_yield","rate_order":"second_order","key_rate_tenors":[],"fx":[{"from_currency":"EUR","to_currency":"USD","as_of_date":"2025-01-15","rate":1.25}]});
    assert!(serde_json::from_value::<StressPolicy>(malformed).is_err());
}
