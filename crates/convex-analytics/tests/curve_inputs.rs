//! CVX-056 lossless economic descriptions and pure composition fixtures.
use convex_analytics::curve_inputs::*;
use convex_core::{
    calendars::WeekendCalendar,
    cashflows::CashflowError,
    contracts::{IndexId, UtcTimestamp},
    daycounts::DayCountConvention as Dc,
    fixings::{FixingRequirement, FixingSeries},
    types::{Currency, Date},
};
use convex_curves::{contracts::*, ExtrapolationMethod as Ext, InterpolationMethod as Int};
use rust_decimal_macros::dec;

fn date(value: &str) -> Date {
    Date::parse(value).unwrap()
}
fn fixture() -> Vec<CurveHelperInput> {
    let v: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/curve-helper-inputs.json")).unwrap();
    serde_json::from_value(v["helpers"].clone()).unwrap()
}
fn pin() -> ReferencePin {
    ReferencePin {
        id: "WEEKEND".into(),
        version: "supplied-v1".into(),
    }
}
fn resources<'a>(calendar: &'a WeekendCalendar, fixings: &'a FixingSeries) -> HelperResources<'a> {
    HelperResources {
        calendars: vec![CalendarBinding {
            pin: pin(),
            calendar,
        }],
        fixings,
    }
}
fn recipe(helpers: &[CurveHelperInput]) -> HelperCurveRecipe {
    HelperCurveRecipe {
        recipe: CurveRecipe {
            key: CurveKey {
                id: "USD.contract".into(),
                version: "supplied-v1".into(),
            },
            role: CurveRole::Discount {
                collateral_currency: Currency::USD,
            },
            currency: Currency::USD,
            reference_date: date("2024-01-01"),
            time_basis: Dc::Act365Fixed,
            representation: CurveRepresentation::DiscountFactors,
            interpolation: Int::LogLinear,
            extrapolation: Ext::None,
            dependencies: vec![],
            calibration: CalibrationMode::Piecewise,
            source: CurveSource::Helpers(helpers.iter().map(|h| h.id.clone()).collect()),
        },
        helpers: helpers.to_vec(),
    }
}
fn money_helper() -> CurveHelperInput {
    CurveHelperInput {
        id: "deposit".into(),
        currency: Currency::USD,
        terms: CurveHelperTerms::MoneyMarket(MoneyMarketTerms::Deposit {
            start_date: date("2024-01-01"),
            maturity_date: date("2024-04-01"),
            day_count: Dc::Act360,
        }),
        quote: MarketQuote {
            value: 5.0,
            unit: QuoteUnit::PercentRate,
        },
        pillar: HelperPillar::Maturity,
        tolerance: 1e-10,
        weight: 1.0,
        residual_scale: 100.0,
    }
}

#[test]
fn all_economic_helper_terms_roundtrip_without_loss() {
    let mut helpers = fixture();
    if let CurveHelperTerms::RateFuture(future) = &mut helpers[0].terms {
        future.fixing_context.known_at =
            UtcTimestamp::parse("2024-01-01T12:00:00.000000007Z").unwrap();
    }
    assert_eq!(helpers.len(), 5);
    for helper in helpers {
        let encoded = serde_json::to_string(&helper).unwrap();
        let decoded: CurveHelperInput = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, helper);
    }
    // Protect the original binary f64 node value that exposed parser rounding.
    let quote = MarketQuote {
        value: (-0.10_f64).exp(),
        unit: QuoteUnit::DiscountFactor,
    };
    let encoded = serde_json::to_string(&quote).unwrap();
    let decoded: MarketQuote = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.value.to_bits(), quote.value.to_bits());
}

#[test]
fn contractual_decimal_profiles_face_and_ticks_preserve_every_digit_and_scale() {
    let mut helpers = fixture();
    let rate: rust_decimal::Decimal = "0.0523456789012345678901234567".parse().unwrap();
    let cash: rust_decimal::Decimal = "99999999999999999.00000000001".parse().unwrap();
    let tick: rust_decimal::Decimal = "0.0025000000000000000000000001".parse().unwrap();
    for helper in &mut helpers {
        let profiles = match &mut helper.terms {
            CurveHelperTerms::RateFuture(future) => {
                future.tick_size = tick;
                future.tick_value = tick.checked_mul(future.price_multiplier).unwrap();
                continue;
            }
            CurveHelperTerms::FixedCouponBond(bond) => {
                bond.quoted_face = cash;
                bond.redemption_fraction = "1.0000000000000000000000000000".parse().unwrap();
                bond.coupons[0].value = rate;
                &mut bond.notionals
            }
            CurveHelperTerms::Cds(cds) => &mut cds.notionals,
            CurveHelperTerms::Swap {
                fixed, floating, ..
            } => {
                fixed.coupons[0].value = rate;
                floating.spreads[0].value = rate;
                &mut fixed.notionals
            }
            CurveHelperTerms::BasisSwap { pay, receive, .. } => {
                pay.spreads[0].value = rate;
                receive.gearings[0].value = "1.0000000000000000000000000000".parse().unwrap();
                &mut pay.notionals
            }
            CurveHelperTerms::MoneyMarket(_) => unreachable!(),
        };
        profiles[0].value = cash;
    }
    for helper in helpers {
        let encoded = serde_json::to_string(&helper).unwrap();
        let decoded: CurveHelperInput = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, helper);
        // Compare encodings too: Decimal equality alone ignores trailing scale.
        assert_eq!(serde_json::to_string(&decoded).unwrap(), encoded);
        match &helper.terms {
            CurveHelperTerms::FixedCouponBond(_) => {
                assert!(encoded.contains(&format!("\"quoted_face\":\"{cash}\"")))
            }
            CurveHelperTerms::RateFuture(_) => {
                assert!(encoded.contains(&format!("\"tick_size\":\"{tick}\"")))
            }
            _ => assert!(encoded.contains(&format!("\"value\":\"{cash}\""))),
        }
    }
}

#[test]
fn exact_contractual_decimals_reject_numeric_and_inexact_string_ingress() {
    let helper = fixture().remove(1);
    let json = serde_json::to_value(helper).unwrap();
    for invalid in [
        serde_json::json!(100.123),
        serde_json::json!("0.12345678901234567890123456789"),
        serde_json::json!("100000000000000000000000000000"),
        serde_json::json!("NaN"),
    ] {
        let mut changed = json.clone();
        changed["terms"]["FixedCouponBond"]["quoted_face"] = invalid.clone();
        assert!(serde_json::from_value::<CurveHelperInput>(changed).is_err());
        let mut changed = json.clone();
        changed["terms"]["FixedCouponBond"]["coupons"][0]["value"] = invalid;
        assert!(serde_json::from_value::<CurveHelperInput>(changed).is_err());
    }
}

#[test]
fn full_future_bond_credit_swap_and_basis_descriptions_use_actual_core_primitives() {
    let calendar = WeekendCalendar;
    let fixings = FixingSeries::default();
    let resources = resources(&calendar, &fixings);
    let helpers = fixture();
    let described: Vec<_> = helpers
        .iter()
        .map(|h| describe_helper(h, date("2024-01-01"), &resources).unwrap())
        .collect();
    assert!(described
        .iter()
        .all(|d| d.calibration_status == CurveCapabilityStatus::DescribedUnsupported));
    assert_eq!(described[0].pillar, date("2024-07-02"));
    assert_eq!(described[1].pillar, date("2025-01-17"));
    assert_eq!(described[2].pillar, date("2029-12-24"));
    assert_eq!(described[3].pillar, date("2024-10-03"));
    let CurveHelperTerms::RateFuture(future) = &helpers[0].terms else {
        panic!("future fixture")
    };
    assert_eq!(future.tick_size, dec!(0.0025));
    assert_eq!(future.tick_value, dec!(2.5));
    assert_eq!(future.fixing_requirements.len(), 1);
    let CurveHelperTerms::FixedCouponBond(bond) = &helpers[1].terms else {
        panic!("bond fixture")
    };
    assert_eq!(bond.coupons[1].value, dec!(0.06));
    assert_eq!(bond.notionals[1].value, dec!(60));
    assert_eq!(bond.redemption_fraction, dec!(1.025));
    assert!(bond.coupon_schedule.dates.ex_coupon.is_some());
    let CurveHelperTerms::Cds(cds) = &helpers[2].terms else {
        panic!("credit fixture")
    };
    assert_eq!(cds.running_coupon.unit, QuoteUnit::BasisPoints);
    assert_eq!(cds.quote_convention, CdsHelperQuote::CleanUpfront);
    assert!(cds.settles_accrual_on_default);
    assert_ne!(cds.protection_start, cds.step_in_date);
}

#[test]
fn complex_helpers_never_become_tenor_rate_proxies_or_partial_builds() {
    let calendar = WeekendCalendar;
    let fixings = FixingSeries::default();
    let resources = resources(&calendar, &fixings);
    for helper in fixture() {
        let request = recipe(&[helper]);
        let result = build_helper_curve_set(&[request], &resources, &CurveSet::new());
        assert!(
            matches!(result,Err(CurveContractError::Unsupported{capability,..}) if capability=="product-backed helper calibration")
        );
    }
    let mut helpers = fixture();
    helpers.push(money_helper());
    assert!(build_helper_curve_set(&[recipe(&helpers)], &resources, &CurveSet::new()).is_err());
}

#[test]
fn actual_simple_helper_dispatch_is_shared_by_dto_and_public_native_routes() {
    let calendar = WeekendCalendar;
    let fixings = FixingSeries::default();
    let resources = resources(&calendar, &fixings);
    let request = recipe(&[money_helper()]);
    let encoded = serde_json::to_string(&request).unwrap();
    let decoded: convex_analytics::dto::HelperCurveRecipe = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, request);
    let result = build_helper_curve_set(&[decoded], &resources, &CurveSet::new()).unwrap();
    let c = result.curves.get(&request.recipe.key).unwrap();
    let query: convex_analytics::dto::CurveQuery = CurveQuery::DiscountFactor {
        point: CurvePoint::Date(date("2024-04-01")),
    };
    let value = c.query(&query).unwrap();
    assert!((value.value - 1.0 / (1.0 + 0.05 * 91.0 / 360.0)).abs() < 1e-12);
    assert_eq!(c.diagnostics().quotes[0].model.unit, QuoteUnit::PercentRate);
}

#[test]
fn recipe_id_dimensions_currency_weights_pillars_and_expiry_are_checked() {
    let calendar = WeekendCalendar;
    let fixings = FixingSeries::default();
    let resources = resources(&calendar, &fixings);
    let helper = money_helper();
    let mut request = recipe(std::slice::from_ref(&helper));
    request.recipe.source = CurveSource::Helpers(vec!["different".into()]);
    assert!(build_helper_curve_set(&[request], &resources, &CurveSet::new()).is_err());
    let mut request = recipe(std::slice::from_ref(&helper));
    request.helpers[0].currency = Currency::EUR;
    assert!(build_helper_curve_set(&[request], &resources, &CurveSet::new()).is_err());
    for bad in [0.0, -1.0, f64::NAN] {
        let mut h = helper.clone();
        h.weight = bad;
        assert!(describe_helper(&h, date("2024-01-01"), &resources).is_err());
    }
    let mut bond = fixture()[1].clone();
    bond.pillar = HelperPillar::Explicit(date("2025-02-01"));
    assert!(describe_helper(&bond, date("2024-01-01"), &resources).is_err());
    assert!(describe_helper(&fixture()[1], date("2026-01-01"), &resources).is_err());
}

#[test]
fn calendars_require_exact_versions_and_duplicate_bindings_fail() {
    let calendar = WeekendCalendar;
    let fixings = FixingSeries::default();
    let mut resources = resources(&calendar, &fixings);
    let mut helper = fixture()[1].clone();
    if let CurveHelperTerms::FixedCouponBond(terms) = &mut helper.terms {
        terms.coupon_schedule.calendar.version = "unavailable-v2".into();
    }
    assert!(describe_helper(&helper, date("2024-01-01"), &resources).is_err());
    resources.calendars.push(CalendarBinding {
        pin: pin(),
        calendar: &calendar,
    });
    assert!(describe_helper(&money_helper(), date("2024-01-01"), &resources).is_err());
}

#[test]
fn stepped_bond_overlaps_gaps_bad_quote_and_principal_terms_fail() {
    let calendar = WeekendCalendar;
    let fixings = FixingSeries::default();
    let resources = resources(&calendar, &fixings);
    let original = fixture()[1].clone();
    let mut overlap = original.clone();
    if let CurveHelperTerms::FixedCouponBond(b) = &mut overlap.terms {
        b.coupons[1].start = date("2024-08-14");
    }
    assert!(matches!(
        describe_helper(&overlap, date("2024-01-01"), &resources),
        Err(CurveContractError::Primitives(_))
    ));
    let mut gap = original.clone();
    if let CurveHelperTerms::FixedCouponBond(b) = &mut gap.terms {
        b.notionals[1].start = date("2024-10-16");
    }
    assert!(describe_helper(&gap, date("2024-01-01"), &resources).is_err());
    let mut bad = original.clone();
    bad.quote.unit = QuoteUnit::PercentRate;
    assert!(describe_helper(&bad, date("2024-01-01"), &resources).is_err());
    let mut bad = original;
    if let CurveHelperTerms::FixedCouponBond(b) = &mut bad.terms {
        b.quoted_face = dec!(0);
    }
    assert!(describe_helper(&bad, date("2024-01-01"), &resources).is_err());
}

#[test]
fn future_ticks_units_and_observation_requirements_are_exact() {
    let calendar = WeekendCalendar;
    let fixings = FixingSeries::default();
    let resources = resources(&calendar, &fixings);
    let original = fixture()[0].clone();
    let mut bad = original.clone();
    if let CurveHelperTerms::RateFuture(f) = &mut bad.terms {
        f.tick_value = dec!(2.6);
    }
    assert!(describe_helper(&bad, date("2024-01-01"), &resources).is_err());
    let mut bad = original.clone();
    if let CurveHelperTerms::RateFuture(f) = &mut bad.terms {
        f.fixing_requirements[0].index_id = IndexId::new("OTHER").unwrap();
    }
    assert!(describe_helper(&bad, date("2024-01-01"), &resources).is_err());
    let mut bad = original;
    bad.quote.unit = QuoteUnit::DecimalRate;
    assert!(describe_helper(&bad, date("2024-01-01"), &resources).is_err());
}

#[test]
fn future_missing_published_history_remains_a_complete_typed_failure() {
    let calendar = WeekendCalendar;
    let fixings = FixingSeries::default();
    let resources = resources(&calendar, &fixings);
    let mut future = fixture()[0].clone();
    if let CurveHelperTerms::RateFuture(f) = &mut future.terms {
        f.fixing_context.valuation_date = date("2024-04-02");
        f.fixing_context.known_at = UtcTimestamp::parse("2024-04-02T12:00:00Z").unwrap();
    }
    let result = describe_helper(&future, date("2024-04-02"), &resources);
    assert!(
        matches!(result,Err(CurveContractError::Primitives(CashflowError::MissingFixings(requirements))) if requirements.len()==1&&requirements[0].observation_date==date("2024-03-28"))
    );
}

#[test]
fn overnight_future_preserves_compounding_window_and_generated_publications() {
    let calendar = WeekendCalendar;
    let fixings = FixingSeries::default();
    let resources = resources(&calendar, &fixings);
    let mut helper = fixture()[0].clone();
    if let CurveHelperTerms::RateFuture(f) = &mut helper.terms {
        f.accrual_start = date("2024-04-01");
        f.accrual_end = date("2024-04-08");
        f.last_trading_date = date("2024-04-05");
        f.final_settlement_date = date("2024-04-09");
        f.accrual = FutureAccrual::OvernightCompounded {
            index_id: IndexId::new("USD.SOFR").unwrap(),
            convention: convex_core::cashflows::OvernightConvention {
                lookback_business_days: 0,
                observation_shift: false,
                lockout_business_days: 0,
            },
        };
        f.fixing_requirements = (1..=5)
            .map(|day| FixingRequirement {
                index_id: IndexId::new("USD.SOFR").unwrap(),
                observation_date: Date::from_ymd(2024, 4, day).unwrap(),
                expected_publication: UtcTimestamp::parse(&format!("2024-04-0{day}T12:00:00Z"))
                    .unwrap(),
            })
            .collect();
    }
    assert!(describe_helper(&helper, date("2024-01-01"), &resources).is_ok());
    let roundtrip: CurveHelperInput =
        serde_json::from_str(&serde_json::to_string(&helper).unwrap()).unwrap();
    assert_eq!(roundtrip, helper);
    if let CurveHelperTerms::RateFuture(f) = &mut helper.terms {
        f.fixing_requirements.pop();
    }
    assert!(describe_helper(&helper, date("2024-01-01"), &resources).is_err());
}

#[test]
fn credit_recovery_upfront_running_coupon_and_protection_are_checked() {
    let calendar = WeekendCalendar;
    let fixings = FixingSeries::default();
    let resources = resources(&calendar, &fixings);
    let original = fixture()[2].clone();
    let mut bad = original.clone();
    if let CurveHelperTerms::Cds(c) = &mut bad.terms {
        c.recovery = f64::NAN;
    }
    assert!(describe_helper(&bad, date("2024-01-01"), &resources).is_err());
    let mut bad = original.clone();
    bad.quote.unit = QuoteUnit::BasisPoints;
    assert!(describe_helper(&bad, date("2024-01-01"), &resources).is_err());
    let mut bad = original.clone();
    if let CurveHelperTerms::Cds(c) = &mut bad.terms {
        c.running_coupon.value = -100.0;
    }
    assert!(describe_helper(&bad, date("2024-01-01"), &resources).is_err());
    let mut bad = original;
    if let CurveHelperTerms::Cds(c) = &mut bad.terms {
        c.protection_end = date("2024-01-01");
    }
    assert!(describe_helper(&bad, date("2024-01-01"), &resources).is_err());
}

#[test]
fn floating_spread_gearing_reset_and_projection_terms_cannot_be_dropped() {
    let calendar = WeekendCalendar;
    let fixings = FixingSeries::default();
    let resources = resources(&calendar, &fixings);
    let original = fixture()[3].clone();
    let mut bad = original.clone();
    if let CurveHelperTerms::Swap { floating, .. } = &mut bad.terms {
        floating.gearings[0].end = date("2024-09-30");
    }
    assert!(describe_helper(&bad, date("2024-01-01"), &resources).is_err());
    let mut bad = original.clone();
    if let CurveHelperTerms::Swap { floating, .. } = &mut bad.terms {
        floating.fixing_requirements.pop();
    }
    assert!(describe_helper(&bad, date("2024-01-01"), &resources).is_err());
    let mut bad = original;
    if let CurveHelperTerms::Swap { floating, .. } = &mut bad.terms {
        floating.projection_curve.version.clear();
    }
    assert!(describe_helper(&bad, date("2024-01-01"), &resources).is_err());
}

#[test]
fn unrecognized_economic_terms_are_rejected_in_nested_dtos() {
    let helper = fixture()[1].clone();
    let mut value = serde_json::to_value(&helper).unwrap();
    value["terms"]["FixedCouponBond"]["coupon_schedule"]["dates"]["missing_model"] =
        serde_json::json!(1);
    assert!(serde_json::from_value::<CurveHelperInput>(value).is_err());
    let mut value = serde_json::to_value(&fixture()[3]).unwrap();
    value["terms"]["Swap"]["cap"] = serde_json::json!(0.1);
    assert!(serde_json::from_value::<CurveHelperInput>(value).is_err());
}
