//! Independent Fraction books and checked amount/availability controls.
use convex_bonds::types::BondIdentifiers;
use convex_core::{Currency, Date};
use convex_portfolio::*;
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde_json::Value;
use std::str::FromStr;
fn d(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}
fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/holding-aggregates.json")).unwrap()
}
fn holding(row: &Value) -> Holding {
    let id = row["id"].as_str().unwrap();
    let mut a = HoldingAnalytics::new();
    let value = row["value"].as_str().map(|v| v.parse::<f64>().unwrap());
    a.ytm = value;
    a.ytw = value;
    a.ytc = value;
    a.current_yield = value;
    a.modified_duration = value;
    a.effective_duration = value;
    a.macaulay_duration = value;
    a.spread_duration = value;
    a.convexity = value;
    a.effective_convexity = value;
    a.z_spread = value;
    a.oas = value;
    a.g_spread = value;
    a.i_spread = value;
    a.asw = value;
    a.years_to_maturity = value;
    a.coupon_rate = value;
    a.dv01 = row["dv01"].as_str().map(|v| v.parse().unwrap());
    a.cs01 = row["cs01"].as_str().map(|v| v.parse().unwrap());
    Holding::builder()
        .id(id)
        .identifiers(BondIdentifiers::new().with_ticker(id))
        .par_amount(d(row["par"].as_str().unwrap()))
        .market_price(d(row["clean"].as_str().unwrap()))
        .accrued_interest(d(row["accrued"].as_str().unwrap()))
        .fx_rate(d(row["fx"].as_str().unwrap()))
        .analytics(a)
        .build()
        .unwrap()
}
fn expected(value: &Value) -> Option<Decimal> {
    value.as_str().map(d)
}
#[test]
fn independent_fraction_books_preserve_full_coverage_and_signed_currency() {
    type Weighted = fn(&[Holding], &AnalyticsConfig) -> PortfolioResult<Option<f64>>;
    let weighted: [Weighted; 18] = [
        weighted_modified_duration,
        weighted_effective_duration,
        weighted_macaulay_duration,
        weighted_spread_duration,
        weighted_best_duration,
        weighted_convexity,
        weighted_effective_convexity,
        weighted_ytm,
        weighted_ytw,
        weighted_ytc,
        weighted_current_yield,
        weighted_best_yield,
        weighted_z_spread,
        weighted_oas,
        weighted_g_spread,
        weighted_i_spread,
        weighted_asw,
        weighted_best_spread,
    ];
    let reference = reference();
    let cases = reference["weighted_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 45);
    for case in cases {
        let rows = case["holdings"]
            .as_array()
            .unwrap()
            .iter()
            .map(holding)
            .collect::<Vec<_>>();
        let method = match case["weighting"].as_str().unwrap() {
            "market_value" => WeightingMethod::MarketValue,
            "par_value" => WeightingMethod::ParValue,
            "equal_weight" => WeightingMethod::EqualWeight,
            _ => panic!("reference weighting"),
        };
        let config = AnalyticsConfig::default().with_weighting(method);
        let wanted = expected(&case["expected_weighted_Decimal"]).map(|v| v.to_f64().unwrap());
        for operation in weighted {
            let actual = operation(&rows, &config).unwrap();
            match (actual, wanted) {
                (Some(a), Some(b)) => assert!(
                    a.to_bits().abs_diff(b.to_bits()) <= 4,
                    "{}: {a} != {b}",
                    case["id"]
                ),
                (None, None) => {}
                _ => panic!("{}: availability differs", case["id"]),
            }
        }
        let shares = Some(d(case["shares"].as_str().unwrap()));
        assert_eq!(
            total_dv01(&rows, &config).unwrap(),
            expected(&case["expected_currency"]["dv01"]),
            "{}",
            case["id"]
        );
        assert_eq!(
            total_cs01(&rows, &config).unwrap(),
            expected(&case["expected_currency"]["cs01"])
        );
        assert_eq!(
            dv01_per_share(&rows, shares, &config).unwrap(),
            expected(&case["expected_per_share"]["dv01"])
        );
        assert_eq!(
            cs01_per_share(&rows, shares, &config).unwrap(),
            expected(&case["expected_per_share"]["cs01"])
        );
        let risk = calculate_risk_metrics(&rows, shares, &config).unwrap();
        let spread = calculate_spread_metrics(&rows, shares, &config).unwrap();
        assert_eq!(
            risk.total_dv01,
            expected(&case["expected_currency"]["dv01"])
        );
        assert_eq!(
            spread.total_cs01,
            expected(&case["expected_currency"]["cs01"])
        );
        for (json, key, wanted) in [
            (
                serde_json::to_value(&risk).unwrap(),
                "total_dv01",
                risk.total_dv01,
            ),
            (
                serde_json::to_value(&spread).unwrap(),
                "total_cs01",
                spread.total_cs01,
            ),
        ] {
            match wanted {
                Some(v) => assert_eq!(d(json[key].as_str().unwrap()), v),
                None => assert!(json[key].is_null()),
            }
        }
    }
}
#[test]
fn independent_exact_amounts_round_only_at_the_final_output() {
    type Amount = fn(&Holding) -> PortfolioResult<Decimal>;
    let operations: [(&str, Amount); 7] = [
        ("market_value_local", Holding::market_value_local),
        ("market_value", Holding::market_value),
        ("accrued_amount_local", Holding::accrued_amount_local),
        ("accrued_amount", Holding::accrued_amount),
        ("total_value_local", Holding::total_value_local),
        ("total_value", Holding::total_value),
        ("dirty_price", Holding::dirty_price),
    ];
    let reference = reference();
    let cases = reference["amount_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 8);
    for case in cases {
        let row = holding(&case["holding"]);
        for (name, operation) in operations {
            let wanted = &case["expected"][name];
            let actual = operation(&row);
            match wanted["status"].as_str().unwrap() {
                "available" => assert_eq!(
                    actual.unwrap(),
                    d(wanted["value"].as_str().unwrap()),
                    "{} {name}",
                    case["id"]
                ),
                "invalid" => assert!(matches!(actual, Err(PortfolioError::InvalidHolding { .. }))),
                "arithmetic" => assert!(matches!(
                    actual,
                    Err(PortfolioError::InvalidPortfolio { .. })
                )),
                _ => panic!("reference status"),
            }
        }
    }
}
#[test]
fn malformed_metrics_and_duplicate_identities_never_become_zero() {
    let mut row = holding(&reference()["weighted_cases"][0]["holdings"][0]);
    for scalar in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        f64::MIN_POSITIVE,
    ] {
        row.analytics.modified_duration = Some(scalar);
        row.analytics.dv01 = Some(scalar);
        assert!(weighted_modified_duration(&[row.clone()], &AnalyticsConfig::default()).is_err());
        assert!(row.total_dv01().is_err());
        assert!(total_dv01(&[row.clone()], &AnalyticsConfig::default()).is_err());
    }
    row.analytics.modified_duration = Some(1.0);
    row.analytics.dv01 = Some(1.0);
    assert!(
        weighted_modified_duration(&[row.clone(), row.clone()], &AnalyticsConfig::default())
            .is_err()
    );
    row.fx_rate = Decimal::ZERO;
    assert!(row.market_value().is_err());
    assert!(total_dv01(&[row], &AnalyticsConfig::default()).is_err());
}
#[test]
fn nav_weights_and_currency_identity_retain_signed_and_undefined_states() {
    let mut a = holding(&reference()["weighted_cases"][0]["holdings"][0]);
    a.market_price = Decimal::ONE_HUNDRED;
    a.par_amount = d("10000000000000000000000000000");
    a.accrued_interest = Decimal::ZERO;
    a.fx_rate = Decimal::ONE;
    let mut b = a.clone();
    b.id = "small".into();
    b.par_amount = Decimal::ONE;
    let mut c = a.clone();
    c.id = "short".into();
    c.par_amount = -c.par_amount;
    let p = Portfolio {
        id: "signed".into(),
        name: "exact cancellation".into(),
        base_currency: Currency::USD,
        as_of_date: Date::from_ymd(2025, 1, 15).unwrap(),
        holdings: vec![a.clone(), b, c.clone()],
        cash: vec![],
        shares_outstanding: Some(Decimal::ONE),
        liabilities: None,
    };
    assert_eq!(p.nav().unwrap(), Decimal::ONE);
    assert_eq!(p.nav_per_share().unwrap(), Some(Decimal::ONE));
    let mut zero = p;
    zero.holdings = vec![a, c];
    assert_eq!(zero.nav().unwrap(), Decimal::ZERO);
    assert!(zero
        .calculate_weights(WeightingMethod::MarketValue)
        .unwrap()
        .is_none());
    assert!(calculate_nav_breakdown(&zero)
        .unwrap()
        .securities_pct()
        .unwrap()
        .is_none());
    let parity = CashPosition::with_fx_rate(Decimal::ONE, Currency::EUR, Decimal::ONE);
    assert!(!parity.is_base_currency(Currency::USD));
    assert!(parity.is_base_currency(Currency::EUR));
}

#[test]
fn retired_config_cannot_silently_enable_incomplete_averages() {
    let mut value = serde_json::to_value(AnalyticsConfig::default()).unwrap();
    assert!(value.get("include_incomplete").is_none());
    value["include_incomplete"] = true.into();
    assert!(serde_json::from_value::<AnalyticsConfig>(value).is_err());
}
#[test]
fn large_exact_stress_manifest_and_original_error_identity_are_preserved() {
    use convex_analytics::risk::{Duration, KeyRateDuration, KeyRateDurations};
    use convex_portfolio::stress::{StressOrder, StressPolicy, StressRateBasis, TenorShift};
    let mut row = holding(&reference()["weighted_cases"][0]["holdings"][0]);
    row.par_amount = Decimal::ONE_HUNDRED;
    row.market_price = Decimal::ONE_HUNDRED;
    row.accrued_interest = Decimal::ZERO;
    row.fx_rate = Decimal::ONE;
    let tenors = (1..=4096).map(f64::from).collect::<Vec<_>>();
    row.analytics.key_rate_durations = Some(KeyRateDurations::new(
        tenors
            .iter()
            .map(|&tenor| KeyRateDuration {
                tenor,
                duration: Duration::new(Decimal::ONE),
            })
            .collect(),
    ));
    let mut book = Portfolio {
        id: "large-exact-factors".into(),
        name: "supplied factor manifest".into(),
        base_currency: Currency::USD,
        as_of_date: Date::from_ymd(2025, 1, 15).unwrap(),
        holdings: vec![row],
        cash: vec![],
        shares_outstanding: None,
        liabilities: None,
    };
    let policy = StressPolicy {
        rate_basis: StressRateBasis::KeyRateCurve,
        rate_order: StressOrder::FirstOrder,
        key_rate_tenors: tenors.clone(),
        fx: vec![],
    };
    let scenario =
        StressScenario::new("4096exact").with_rate_scenario(RateScenario::KeyRateShifts(
            tenors
                .into_iter()
                .map(|t| TenorShift::new(t, 1.0))
                .collect(),
        ));
    let result = run_stress_scenario(&book, &scenario, &policy).unwrap();
    assert_eq!(result.pnl, d("-40.96"));
    assert_eq!(result.pnl_pct, Some(d("-40.96")));
    book.holdings[0].analytics.modified_duration = Some(f64::INFINITY);
    let policy = StressPolicy {
        rate_basis: StressRateBasis::ModifiedYield,
        rate_order: StressOrder::FirstOrder,
        key_rate_tenors: vec![],
        fx: vec![],
    };
    let scenario = StressScenario::new("invalid supplied scalar")
        .with_rate_scenario(RateScenario::ParallelShift(1.0));
    let error = run_stress_scenario(&book, &scenario, &policy).unwrap_err();
    assert_eq!(error.holding_id.as_deref(), Some("a"));
}

#[test]
fn exact_allocations_keep_explicit_measures_full_coverage_and_zero_percentages() {
    for case in reference()["weighted_cases"].as_array().unwrap() {
        let rows = case["holdings"]
            .as_array()
            .unwrap()
            .iter()
            .map(holding)
            .collect::<Vec<_>>();
        let method = match case["weighting"].as_str().unwrap() {
            "market_value" => WeightingMethod::MarketValue,
            "par_value" => WeightingMethod::ParValue,
            "equal_weight" => WeightingMethod::EqualWeight,
            _ => panic!("reference weighting"),
        };
        let config = AnalyticsConfig::default().with_weighting(method);
        let duration = duration_contributions(&rows, &config, DurationMeasure::Modified);
        if let Some(wanted) = expected(&case["expected_weighted_Decimal"]) {
            let actual = duration.unwrap();
            assert_eq!(actual.by_holding.len(), rows.len());
            assert!(
                actual
                    .portfolio_duration
                    .to_bits()
                    .abs_diff(wanted.to_f64().unwrap().to_bits())
                    <= 4
            );
            if wanted.is_zero() {
                assert!(actual
                    .by_holding
                    .iter()
                    .all(|r| r.contribution_pct.is_none()));
            }
        } else {
            assert!(duration.is_err());
        }
        let risk = dv01_contributions(&rows, &config);
        if rows.is_empty() || expected(&case["expected_currency"]["dv01"]).is_none() {
            assert!(risk.is_err());
        } else {
            let actual = risk.unwrap();
            assert_eq!(
                Some(actual.total_dv01),
                expected(&case["expected_currency"]["dv01"])
            );
            assert_eq!(actual.by_holding.len(), rows.len());
            if actual.total_dv01.is_zero() {
                assert!(actual
                    .by_holding
                    .iter()
                    .all(|r| r.contribution_pct.is_none()));
            }
        }
    }
    let mut a = holding(&reference()["weighted_cases"][0]["holdings"][0]);
    a.analytics.modified_duration = None;
    assert!(duration_contributions(
        &[a.clone()],
        &AnalyticsConfig::default(),
        DurationMeasure::Modified
    )
    .is_err());
    assert!(duration_contributions(
        &[a.clone()],
        &AnalyticsConfig::default(),
        DurationMeasure::Effective
    )
    .is_ok());
    a.analytics.z_spread = None;
    assert!(spread_contributions(
        &[a.clone()],
        &AnalyticsConfig::default(),
        SpreadMeasure::ZSpread
    )
    .is_err());
    assert!(spread_contributions(&[a], &AnalyticsConfig::default(), SpreadMeasure::Oas).is_ok());
}

#[test]
fn signed_bucket_cohorts_never_disappear_at_zero_net_value() {
    use convex_portfolio::{bucketing, Classification, Sector, SectorInfo};
    let row = |id: &str, par: &str, sector| {
        HoldingBuilder::new()
            .id(id)
            .identifiers(convex_bonds::types::BondIdentifiers::new())
            .par_amount(par.parse::<Decimal>().unwrap())
            .market_price(Decimal::ONE_HUNDRED)
            .classification(Classification::new().with_sector(SectorInfo::from_composite(sector)))
            .build()
            .unwrap()
    };
    let config = AnalyticsConfig::default();
    let cancelled = [
        row("long", "1", Sector::Government),
        row("short", "-1", Sector::Corporate),
    ];
    assert!(bucketing::bucket_by_sector(&cancelled, &config).is_err());
    assert!(bucketing::bucket_by_rating(&cancelled, &config).is_err());
    assert!(bucketing::bucket_by_maturity(&cancelled, &config).is_err());
    assert!(bucketing::bucket_by_currency(&cancelled, &config).is_err());
    assert!(bucketing::bucket_by_sector(&[], &config)
        .unwrap()
        .by_sector
        .is_empty());
    let residual = [
        row("long", "1", Sector::Government),
        row(
            "short",
            "-0.9999999999999999999999999999",
            Sector::Corporate,
        ),
    ];
    let buckets = bucketing::bucket_by_sector(&residual, &config).unwrap();
    assert_eq!(
        buckets.total_market_value,
        "0.0000000000000000000000000001".parse::<Decimal>().unwrap()
    );
    assert_eq!(buckets.by_sector.len(), 2);
    assert_eq!(
        buckets.by_sector[&Sector::Government].weight_pct.to_bits(),
        1e30_f64.to_bits()
    );
}
