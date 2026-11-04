//! CVX-056 analytic contract fixtures. Synthetic supplied values, no market pack.
use convex_core::{
    daycounts::DayCountConvention as Dc,
    types::{Compounding, Currency, Date},
};
use convex_curves::{
    contracts::*, DiscreteCurve, ExtrapolationMethod as Ext, InterpolationMethod as Int, ValueType,
};
use proptest::{
    prelude::*,
    test_runner::{Config, RngSeed},
};

fn date(value: &str) -> Date {
    Date::parse(value).unwrap()
}
fn key(id: &str) -> CurveKey {
    CurveKey {
        id: id.into(),
        version: "supplied-v1".into(),
    }
}
fn quote(value: f64, unit: QuoteUnit) -> MarketQuote {
    MarketQuote { value, unit }
}
fn node(t: f64, value: f64, unit: QuoteUnit) -> CurveNode {
    CurveNode {
        pillar: CurvePoint::Tenor(t),
        quote: quote(value, unit),
    }
}
fn recipe() -> CurveRecipe {
    CurveRecipe {
        key: key("USD.test"),
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
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(vec![
            node(0.0, 1.0, QuoteUnit::DiscountFactor),
            node(1.0, (-0.04_f64).exp(), QuoteUnit::DiscountFactor),
            node(2.0, (-0.10_f64).exp(), QuoteUnit::DiscountFactor),
        ]),
    }
}
fn build(recipe: CurveRecipe) -> std::sync::Arc<BuiltCurve> {
    let id = recipe.key.clone();
    build_curve_set(&[recipe], &CurveSet::new())
        .unwrap()
        .curves
        .get(&id)
        .unwrap()
        .clone()
}
fn query(curve: &BuiltCurve, query: CurveQuery) -> f64 {
    curve.query(&query).unwrap().value
}
fn near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-11 + 1e-11 * expected.abs(),
        "{actual} != {expected}"
    );
}
fn helper(id: &str, start: &str, end: &str, value: f64, unit: QuoteUnit) -> MoneyMarketHelper {
    MoneyMarketHelper {
        id: id.into(),
        terms: MoneyMarketTerms::Deposit {
            start_date: date(start),
            maturity_date: date(end),
            day_count: Dc::Act360,
        },
        quote: quote(value, unit),
        tolerance: 1e-9,
    }
}

#[test]
fn explicit_units_normalize_rates_and_refuse_prices() {
    for (value, unit) in [
        (0.05, QuoteUnit::DecimalRate),
        (5.0, QuoteUnit::PercentRate),
        (500.0, QuoteUnit::BasisPoints),
    ] {
        near(quote(value, unit).decimal_rate().unwrap(), 0.05);
        assert_eq!(
            quote(value, unit).model_rate_quote(0.05).unwrap().unit,
            unit
        );
    }
    for unit in [
        QuoteUnit::PricePer100,
        QuoteUnit::FuturePrice100MinusRate,
        QuoteUnit::UpfrontFraction,
        QuoteUnit::DiscountFactor,
    ] {
        assert!(quote(1.0, unit).decimal_rate().is_err());
    }
    assert!(quote(f64::NAN, QuoteUnit::DecimalRate).validate().is_err());
    assert!(quote(10.1, QuoteUnit::DecimalRate).validate().is_err());
    assert!(quote(1.01, QuoteUnit::SurvivalProbability)
        .validate()
        .is_err());
    assert!(quote(-0.01, QuoteUnit::DecimalRate).validate().is_ok());
    assert!(quote(1.1, QuoteUnit::DiscountFactor).validate().is_ok());
    assert!(quote(101.0, QuoteUnit::FuturePrice100MinusRate)
        .validate()
        .is_ok());
}

#[test]
fn direct_nodes_sort_without_mutating_the_original_recipe() {
    let mut r = recipe();
    if let CurveSource::Nodes(nodes) = &mut r.source {
        nodes.reverse();
    }
    let original = r.clone();
    let c = build(r);
    assert_eq!(c.recipe(), &original);
    assert_eq!(c.diagnostics().pillars, vec![0.0, 1.0, 2.0]);
    assert!(c.diagnostics().quotes.is_empty());
    near(
        query(
            &c,
            CurveQuery::DiscountFactor {
                point: CurvePoint::Tenor(1.0),
            },
        ),
        (-0.04_f64).exp(),
    );
}

#[test]
fn negative_rates_and_discount_factors_above_one_are_supported() {
    let mut r = recipe();
    r.source = CurveSource::Nodes(vec![
        node(0.0, 1.0, QuoteUnit::DiscountFactor),
        node(2.0, 0.02_f64.exp(), QuoteUnit::DiscountFactor),
    ]);
    let c = build(r);
    assert!(
        query(
            &c,
            CurveQuery::DiscountFactor {
                point: CurvePoint::Tenor(1.0)
            }
        ) > 1.0
    );
    near(
        query(
            &c,
            CurveQuery::ZeroRate {
                point: CurvePoint::Tenor(1.0),
                compounding: Compounding::Continuous,
                day_count: Dc::Act365Fixed,
            },
        ),
        -0.01,
    );
}

#[test]
fn all_compounding_queries_use_the_actual_forward_period() {
    let c = build(recipe());
    let df = (-0.06_f64).exp();
    for compounding in [
        Compounding::Simple,
        Compounding::Continuous,
        Compounding::Annual,
        Compounding::SemiAnnual,
        Compounding::Quarterly,
        Compounding::Monthly,
        Compounding::Daily,
    ] {
        let expected = match compounding {
            Compounding::Continuous => 0.06,
            Compounding::Simple => 1.0 / df - 1.0,
            _ => {
                let n = f64::from(compounding.periods_per_year());
                n * (df.powf(-1.0 / n) - 1.0)
            }
        };
        near(
            query(
                &c,
                CurveQuery::ForwardRate {
                    start: CurvePoint::Tenor(1.0),
                    end: CurvePoint::Tenor(2.0),
                    compounding,
                    day_count: Dc::Act365Fixed,
                },
            ),
            expected,
        );
    }
    near(
        query(
            &c,
            CurveQuery::DiscountRatio {
                start: CurvePoint::Tenor(1.0),
                end: CurvePoint::Tenor(2.0),
            },
        ),
        df,
    );
}

#[test]
fn nonunit_forward_period_uses_periodic_compounding_exponent() {
    let c = build(recipe());
    let tau = 0.5;
    let df = (-0.06_f64 * tau).exp();
    near(
        query(
            &c,
            CurveQuery::ForwardRate {
                start: CurvePoint::Tenor(1.0),
                end: CurvePoint::Tenor(1.5),
                compounding: Compounding::SemiAnnual,
                day_count: Dc::Act365Fixed,
            },
        ),
        2.0 * (df.powf(-1.0 / (2.0 * tau)) - 1.0),
    );
}

#[test]
fn date_queries_use_true_year_fractions_and_separate_output_basis() {
    let mut r = recipe();
    r.time_basis = Dc::ActActIsda;
    let c = build(r);
    let end = date("2025-01-01");
    near(c.date_to_tenor(end).unwrap(), 1.0);
    let df = query(
        &c,
        CurveQuery::DiscountFactor {
            point: CurvePoint::Date(end),
        },
    );
    near(
        query(
            &c,
            CurveQuery::ZeroRate {
                point: CurvePoint::Date(end),
                compounding: Compounding::Simple,
                day_count: Dc::Act360,
            },
        ),
        (1.0 / df - 1.0) / (366.0 / 360.0),
    );
    assert!(matches!(
        c.query(&CurveQuery::ZeroRate {
            point: CurvePoint::Tenor(1.0),
            compounding: Compounding::Simple,
            day_count: Dc::Act360
        }),
        Err(CurveContractError::Unsupported { .. })
    ));
    let start = date("2024-01-31");
    let end = date("2024-02-29");
    let ratio = query(
        &c,
        CurveQuery::DiscountRatio {
            start: CurvePoint::Date(start),
            end: CurvePoint::Date(end),
        },
    );
    near(
        query(
            &c,
            CurveQuery::ForwardRate {
                start: CurvePoint::Date(start),
                end: CurvePoint::Date(end),
                compounding: Compounding::Simple,
                day_count: Dc::Thirty360E,
            },
        ),
        (1.0 / ratio - 1.0) / (29.0 / 360.0),
    );
}

#[test]
fn inverse_date_conversion_is_explicit_and_exact_across_leap_years() {
    for dc in [Dc::Act360, Dc::Act365Fixed, Dc::ActActIsda] {
        let mut r = recipe();
        r.time_basis = dc;
        let c = build(r);
        for end in ["2024-01-01", "2024-02-29", "2025-01-01", "2026-07-17"] {
            let end = date(end);
            let t = c.date_to_tenor(end).unwrap();
            for rounding in [
                TenorRounding::AtOrBefore,
                TenorRounding::AtOrAfter,
                TenorRounding::Nearest,
            ] {
                assert_eq!(c.tenor_to_date(t, rounding).unwrap(), end);
            }
        }
        let t1 = c.date_to_tenor(date("2024-02-28")).unwrap();
        let t2 = c.date_to_tenor(date("2024-02-29")).unwrap();
        let between = t1 + 0.25 * (t2 - t1);
        assert_eq!(
            c.tenor_to_date(between, TenorRounding::AtOrBefore).unwrap(),
            date("2024-02-28")
        );
        assert_eq!(
            c.tenor_to_date(between, TenorRounding::AtOrAfter).unwrap(),
            date("2024-02-29")
        );
        assert_eq!(
            c.tenor_to_date(between, TenorRounding::Nearest).unwrap(),
            date("2024-02-28")
        );
    }
}

#[test]
fn invalid_query_domains_and_origin_rates_never_return_zero() {
    let c = build(recipe());
    for t in [f64::NAN, f64::INFINITY, -1.0, 150.01] {
        assert!(c
            .query(&CurveQuery::DiscountFactor {
                point: CurvePoint::Tenor(t)
            })
            .is_err());
    }
    for (a, b) in [(1.0, 1.0), (2.0, 1.0)] {
        assert!(c
            .query(&CurveQuery::ForwardRate {
                start: CurvePoint::Tenor(a),
                end: CurvePoint::Tenor(b),
                compounding: Compounding::Simple,
                day_count: Dc::Act365Fixed
            })
            .is_err());
    }
    assert!(c
        .query(&CurveQuery::ZeroRate {
            point: CurvePoint::Tenor(0.0),
            compounding: Compounding::Continuous,
            day_count: Dc::Act365Fixed
        })
        .is_err());
    assert_eq!(
        query(
            &c,
            CurveQuery::DiscountFactor {
                point: CurvePoint::Tenor(0.0)
            }
        ),
        1.0
    );
    assert!(c.date_to_tenor(date("2023-12-31")).is_err());
    assert!(c
        .query(&CurveQuery::SurvivalProbability {
            point: CurvePoint::Tenor(1.0)
        })
        .is_err());
    assert!(c
        .query(&CurveQuery::ZeroRate {
            point: CurvePoint::Date(date("2025-01-01")),
            compounding: Compounding::Simple,
            day_count: Dc::ActActIcma
        })
        .is_err());
}

#[test]
fn tail_policies_are_actual_raw_value_semantics_and_disclosed() {
    let c = build(recipe());
    assert!(c
        .query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Tenor(3.0)
        })
        .is_err());
    let mut r = recipe();
    r.extrapolation = Ext::Flat;
    let c = build(r);
    let result = c
        .query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Tenor(3.0),
        })
        .unwrap();
    assert!(result.extrapolated);
    near(result.value, (-0.10_f64).exp());
    let mut r = recipe();
    r.interpolation = Int::Linear;
    r.extrapolation = Ext::Linear;
    let c = build(r);
    near(
        query(
            &c,
            CurveQuery::Raw {
                point: CurvePoint::Tenor(3.0),
            },
        ),
        2.0 * (-0.10_f64).exp() - (-0.04_f64).exp(),
    );
    assert!(c
        .query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Tenor(150.0)
        })
        .is_err());
}

#[test]
fn unsupported_interpolation_and_extrapolation_never_fall_back() {
    for interpolation in [
        Int::PiecewiseConstant,
        Int::FlatForward,
        Int::MonotoneConvex,
        Int::NelsonSiegel,
        Int::Svensson,
    ] {
        let mut r = recipe();
        r.interpolation = interpolation;
        assert!(matches!(
            build_curve_set(&[r], &CurveSet::new()),
            Err(CurveContractError::Unsupported { .. })
        ));
    }
    for extrapolation in [
        Ext::FlatForward,
        Ext::UfrConvergence {
            ufr: 0.03,
            alpha: 0.1,
        },
    ] {
        let mut r = recipe();
        r.extrapolation = extrapolation;
        assert!(matches!(
            build_curve_set(&[r], &CurveSet::new()),
            Err(CurveContractError::Unsupported { .. })
        ));
    }
    let mut r = recipe();
    r.representation = CurveRepresentation::ZeroRates {
        compounding: Compounding::Continuous,
    };
    assert!(build_curve_set(&[r], &CurveSet::new()).is_err());
}

#[test]
fn duplicate_near_duplicate_bad_unit_origin_and_nonfinite_nodes_fail() {
    let mut variations = Vec::new();
    for extra in [
        node(1.0, 0.9, QuoteUnit::DiscountFactor),
        node(1.0 + 1e-12, 0.9, QuoteUnit::DiscountFactor),
        node(f64::NAN, 0.9, QuoteUnit::DiscountFactor),
        node(4.0, f64::INFINITY, QuoteUnit::DiscountFactor),
        node(4.0, 0.03, QuoteUnit::DecimalRate),
    ] {
        let mut r = recipe();
        if let CurveSource::Nodes(nodes) = &mut r.source {
            nodes.push(extra);
        }
        variations.push(r);
    }
    let mut r = recipe();
    if let CurveSource::Nodes(nodes) = &mut r.source {
        nodes.remove(0);
    }
    variations.push(r);
    let mut r = recipe();
    if let CurveSource::Nodes(nodes) = &mut r.source {
        nodes[0].quote.value = 0.999;
    }
    variations.push(r);
    for r in variations {
        assert!(build_curve_set(&[r], &CurveSet::new()).is_err());
    }
}

#[test]
fn zero_nodes_check_periodic_bases_and_negative_rates() {
    let mut r = recipe();
    r.interpolation = Int::Linear;
    r.representation = CurveRepresentation::ZeroRates {
        compounding: Compounding::SemiAnnual,
    };
    r.source = CurveSource::Nodes(vec![
        node(0.0, -0.01, QuoteUnit::DecimalRate),
        node(2.0, -0.01, QuoteUnit::DecimalRate),
    ]);
    let c = build(r.clone());
    near(
        query(
            &c,
            CurveQuery::DiscountFactor {
                point: CurvePoint::Tenor(2.0),
            },
        ),
        (1.0_f64 - 0.01 / 2.0).powf(-4.0),
    );
    if let CurveSource::Nodes(nodes) = &mut r.source {
        nodes[0].quote.value = -2.0;
    }
    assert!(build_curve_set(&[r], &CurveSet::new()).is_err());
    let mut r = recipe();
    r.interpolation = Int::Linear;
    r.representation = CurveRepresentation::ZeroRates {
        compounding: Compounding::Simple,
    };
    r.source = CurveSource::Nodes(vec![
        node(0.0, 0.01, QuoteUnit::DecimalRate),
        node(2.0, -0.5, QuoteUnit::DecimalRate),
    ]);
    assert!(build_curve_set(&[r], &CurveSet::new()).is_err());
}

#[test]
fn survival_default_and_left_right_hazards_keep_credit_semantics() {
    let mut r = recipe();
    r.role = CurveRole::Credit {
        entity_id: "issuer".into(),
        seniority: "senior".into(),
        recovery: 0.4,
    };
    r.representation = CurveRepresentation::SurvivalProbabilities;
    r.source = CurveSource::Nodes(vec![
        node(0.0, 1.0, QuoteUnit::SurvivalProbability),
        node(1.0, (-0.02_f64).exp(), QuoteUnit::SurvivalProbability),
        node(2.0, (-0.06_f64).exp(), QuoteUnit::SurvivalProbability),
    ]);
    let c = build(r);
    near(
        query(
            &c,
            CurveQuery::HazardRate {
                point: CurvePoint::Tenor(1.0),
                side: HazardSide::Left,
            },
        ),
        0.02,
    );
    near(
        query(
            &c,
            CurveQuery::HazardRate {
                point: CurvePoint::Tenor(1.0),
                side: HazardSide::Right,
            },
        ),
        0.04,
    );
    near(
        query(
            &c,
            CurveQuery::DefaultProbability {
                start: CurvePoint::Tenor(1.0),
                end: CurvePoint::Tenor(2.0),
                conditional: false,
            },
        ),
        (-0.02_f64).exp() - (-0.06_f64).exp(),
    );
    near(
        query(
            &c,
            CurveQuery::DefaultProbability {
                start: CurvePoint::Tenor(1.0),
                end: CurvePoint::Tenor(2.0),
                conditional: true,
            },
        ),
        1.0 - (-0.04_f64).exp(),
    );
    assert!(c
        .query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Tenor(1.0)
        })
        .is_err());
    assert!(c
        .query(&CurveQuery::HazardRate {
            point: CurvePoint::Tenor(0.0),
            side: HazardSide::Left
        })
        .is_err());
    assert!(c
        .query(&CurveQuery::HazardRate {
            point: CurvePoint::Tenor(2.0),
            side: HazardSide::Right
        })
        .is_err());
}

#[test]
fn survival_domain_recovery_and_extrapolation_reject_invalid_curves() {
    let mut r = recipe();
    r.role = CurveRole::Credit {
        entity_id: "issuer".into(),
        seniority: "senior".into(),
        recovery: 0.4,
    };
    r.representation = CurveRepresentation::SurvivalProbabilities;
    r.source = CurveSource::Nodes(vec![
        node(0.0, 1.0, QuoteUnit::SurvivalProbability),
        node(1.0, 0.98, QuoteUnit::SurvivalProbability),
        node(2.0, 0.99, QuoteUnit::SurvivalProbability),
    ]);
    assert!(build_curve_set(&[r.clone()], &CurveSet::new()).is_err());
    r.source = CurveSource::Nodes(vec![
        node(0.0, 1.0, QuoteUnit::SurvivalProbability),
        node(2.0, 0.96, QuoteUnit::SurvivalProbability),
    ]);
    r.extrapolation = Ext::Flat;
    let c = build(r.clone());
    assert_eq!(
        query(
            &c,
            CurveQuery::HazardRate {
                point: CurvePoint::Tenor(3.0),
                side: HazardSide::Right
            }
        ),
        0.0
    );
    r.extrapolation = Ext::Linear;
    assert!(build_curve_set(&[r.clone()], &CurveSet::new()).is_err());
    r.extrapolation = Ext::None;
    if let CurveRole::Credit { recovery, .. } = &mut r.role {
        *recovery = 1.1;
    }
    assert!(build_curve_set(&[r], &CurveSet::new()).is_err());
}

#[test]
fn dated_deposit_fra_chain_reprices_every_original_native_quote() {
    let mut r = recipe();
    r.calibration = CalibrationMode::Piecewise;
    let dep = helper(
        "deposit",
        "2024-01-01",
        "2024-04-01",
        5.0,
        QuoteUnit::PercentRate,
    );
    let fra = MoneyMarketHelper {
        id: "fra".into(),
        terms: MoneyMarketTerms::Fra {
            index_id: "TERM-3M".into(),
            fixing_date: date("2024-03-28"),
            start_date: date("2024-04-01"),
            end_date: date("2024-07-01"),
            settlement_date: date("2024-04-01"),
            settlement: FraSettlement::AtStart,
            day_count: Dc::Act360,
        },
        quote: quote(550.0, QuoteUnit::BasisPoints),
        tolerance: 1e-9,
    };
    r.source = CurveSource::MoneyMarket(vec![fra.clone(), dep.clone()]);
    let c = build(r.clone());
    let expected1 = 1.0 / (1.0 + 0.05 * 91.0 / 360.0);
    let expected2 = expected1 / (1.0 + 0.055 * 91.0 / 360.0);
    near(
        query(
            &c,
            CurveQuery::DiscountFactor {
                point: CurvePoint::Date(date("2024-04-01")),
            },
        ),
        expected1,
    );
    near(
        query(
            &c,
            CurveQuery::DiscountFactor {
                point: CurvePoint::Date(date("2024-07-01")),
            },
        ),
        expected2,
    );
    assert_eq!(c.diagnostics().quotes.len(), 2);
    assert_eq!(c.diagnostics().quotes[0].input, dep.quote);
    assert_eq!(c.diagnostics().quotes[1].input, fra.quote);
    for row in &c.diagnostics().quotes {
        assert_eq!(row.model.unit, row.input.unit);
        assert!(row.residual.abs() <= row.tolerance);
    }
    r.source = CurveSource::MoneyMarket(vec![dep, fra]);
    let ordered = build(r);
    assert_eq!(ordered.diagnostics(), c.diagnostics());
}

#[test]
fn helper_duplicates_unresolved_starts_expiry_history_and_bad_factors_fail() {
    let mut r = recipe();
    r.calibration = CalibrationMode::Piecewise;
    for helpers in [
        vec![helper(
            "a",
            "2024-01-02",
            "2024-04-01",
            0.05,
            QuoteUnit::DecimalRate,
        )],
        vec![
            helper(
                "a",
                "2024-01-01",
                "2024-04-01",
                0.05,
                QuoteUnit::DecimalRate,
            ),
            helper(
                "b",
                "2024-01-01",
                "2024-04-01",
                0.06,
                QuoteUnit::DecimalRate,
            ),
        ],
        vec![helper(
            "a",
            "2023-12-31",
            "2024-04-01",
            0.05,
            QuoteUnit::DecimalRate,
        )],
        vec![helper(
            "a",
            "2024-01-01",
            "2025-01-01",
            -1.0,
            QuoteUnit::DecimalRate,
        )],
    ] {
        r.source = CurveSource::MoneyMarket(helpers);
        assert!(build_curve_set(&[r.clone()], &CurveSet::new()).is_err());
    }
    let fra = MoneyMarketHelper {
        id: "fra".into(),
        terms: MoneyMarketTerms::Fra {
            index_id: "term".into(),
            fixing_date: date("2024-01-01"),
            start_date: date("2024-01-03"),
            end_date: date("2024-04-03"),
            settlement_date: date("2024-01-03"),
            settlement: FraSettlement::AtStart,
            day_count: Dc::Act360,
        },
        quote: quote(0.05, QuoteUnit::DecimalRate),
        tolerance: 1e-10,
    };
    assert!(matches!(
        fra.validate(date("2024-01-01")),
        Err(CurveContractError::Unsupported { .. })
    ));
}

#[test]
fn native_tolerance_failure_is_explicit_and_never_widened() {
    let mut r = recipe();
    r.calibration = CalibrationMode::Piecewise;
    let mut h = helper(
        "strict",
        "2024-01-01",
        "2024-04-01",
        5.0,
        QuoteUnit::PercentRate,
    );
    h.tolerance = 1e-30;
    r.source = CurveSource::MoneyMarket(vec![h]);
    assert!(
        matches!(build_curve_set(&[r],&CurveSet::new()),Err(CurveContractError::RepricingFailure{tolerance,..}) if tolerance==1e-30)
    );
}

fn spread_recipe(base: &CurveRecipe, id: &str) -> CurveRecipe {
    let mut r = base.clone();
    r.key = key(id);
    r.representation = CurveRepresentation::ZeroRates {
        compounding: Compounding::Continuous,
    };
    r.dependencies = vec![base.key.clone()];
    r.source = CurveSource::ContinuousZeroSpread {
        base: base.key.clone(),
        spread: quote(100.0, QuoteUnit::BasisPoints),
    };
    r
}

#[test]
fn dependency_order_and_spread_versions_resolve_actual_values_atomically() {
    let base = recipe();
    let spread = spread_recipe(&base, "USD.spread");
    let result = build_curve_set(&[spread.clone(), base.clone()], &CurveSet::new()).unwrap();
    assert_eq!(
        result.build_order,
        vec![base.key.clone(), spread.key.clone()]
    );
    let c = result.curves.get(&spread.key).unwrap();
    near(
        query(
            c,
            CurveQuery::DiscountFactor {
                point: CurvePoint::Tenor(1.0),
            },
        ),
        (-0.05_f64).exp(),
    );
    assert_eq!(c.diagnostics().dependencies, vec![base.key.clone()]);
    let mut wrong = base.key.clone();
    wrong.version = "v2".into();
    assert!(matches!(
        result.curves.get(&wrong),
        Err(CurveContractError::MissingDependency(_))
    ));
    let mut bad = spread.clone();
    bad.key = key("USD.bad");
    bad.currency = Currency::EUR;
    assert!(build_curve_set(&[bad], &result.curves).is_err());
    assert_eq!(result.curves.len(), 2);
    assert!(build_curve_set(&[base], &result.curves).is_err());
}

#[test]
fn missing_dependencies_cycles_and_unused_versions_fail() {
    let base = recipe();
    let spread = spread_recipe(&base, "USD.spread");
    assert!(matches!(
        build_curve_set(std::slice::from_ref(&spread), &CurveSet::new()),
        Err(CurveContractError::MissingDependency(_))
    ));
    let mut a = spread.clone();
    a.key = key("a");
    let mut b = spread;
    b.key = key("b");
    a.dependencies = vec![b.key.clone()];
    b.dependencies = vec![a.key.clone()];
    a.source = CurveSource::ContinuousZeroSpread {
        base: b.key.clone(),
        spread: quote(0.01, QuoteUnit::DecimalRate),
    };
    b.source = CurveSource::ContinuousZeroSpread {
        base: a.key.clone(),
        spread: quote(0.01, QuoteUnit::DecimalRate),
    };
    assert!(matches!(
        build_curve_set(&[a, b], &CurveSet::new()),
        Err(CurveContractError::DependencyCycle(_))
    ));
    let mut r = base;
    r.dependencies = vec![key("unused")];
    assert!(build_curve_set(&[r], &CurveSet::new()).is_err());
}

#[test]
fn construction_and_inverse_work_are_bounded() {
    let r = recipe();
    assert!(matches!(
        build_curve_set(&vec![r.clone(); MAX_CURVES + 1], &CurveSet::new()),
        Err(CurveContractError::BoundExceeded(_))
    ));
    let mut excessive = r.clone();
    excessive.source = CurveSource::Nodes(vec![
        node(0.0, 1.0, QuoteUnit::DiscountFactor);
        MAX_CURVE_INPUTS + 1
    ]);
    assert!(matches!(
        build_curve_set(&[excessive], &CurveSet::new()),
        Err(CurveContractError::BoundExceeded(_))
    ));
    let mut r = r;
    r.reference_date = date("9999-12-31");
    let c = build(r);
    assert_eq!(
        c.tenor_to_date(0.0, TenorRounding::Nearest).unwrap(),
        date("9999-12-31")
    );
    assert!(c.tenor_to_date(0.01, TenorRounding::Nearest).is_err());
}

#[test]
fn unsupported_global_configuration_is_preserved_and_invalid_inputs_fail() {
    let settings = GlobalFitSettings {
        max_iterations: 100,
        objective_tolerance: 1e-10,
        initial_zeros: vec![0.04, 0.05],
        lower_zero: -0.1,
        upper_zero: 0.5,
        quote_scales: vec![1.0, 100.0],
        weights: vec![1.0, 2.0],
    };
    let mut r = recipe();
    r.calibration = CalibrationMode::Global(settings.clone());
    let encoded = serde_json::to_string(&r).unwrap();
    assert_eq!(serde_json::from_str::<CurveRecipe>(&encoded).unwrap(), r);
    assert!(matches!(
        build_curve_set(&[r.clone()], &CurveSet::new()),
        Err(CurveContractError::Unsupported { .. })
    ));
    let mut invalid_settings = settings;
    invalid_settings.weights[0] = f64::NAN;
    r.calibration = CalibrationMode::Global(invalid_settings);
    assert!(matches!(
        build_curve_set(&[r], &CurveSet::new()),
        Err(CurveContractError::InvalidInput { .. })
    ));
}

#[test]
fn dto_unknown_economic_fields_are_rejected_and_capabilities_are_honest() {
    let r = recipe();
    let mut value = serde_json::to_value(&r).unwrap();
    value["mystery_model"] = serde_json::json!("ignored?");
    assert!(serde_json::from_value::<CurveRecipe>(value).is_err());
    let capabilities = curve_capabilities();
    assert!(capabilities
        .iter()
        .any(|c| c.name == "dated_deposit_fra_piecewise"
            && c.status == CurveCapabilityStatus::Supported));
    assert!(capabilities
        .iter()
        .any(|c| c.name == "product_helper_global_calibration"
            && c.status == CurveCapabilityStatus::DescribedUnsupported));
}

#[test]
fn discrete_checked_query_honors_extrapolation_and_rejects_nan() {
    let c = DiscreteCurve::with_extrapolation(
        date("2024-01-01"),
        vec![1.0, 2.0],
        vec![0.04, 0.05],
        ValueType::zero_rate(Compounding::Continuous),
        Int::Linear,
        Ext::None,
    )
    .unwrap();
    assert!(c.checked_value_at(f64::NAN).is_err());
    assert!(c.checked_value_at(3.0).is_err());
    near(c.checked_value_at(1.5).unwrap(), 0.045);
}

proptest! {
    #![proptest_config(Config { cases:256, rng_seed:RngSeed::Fixed(0x056), ..Config::default() })]
    #[test]
    fn generated_negative_and_positive_rate_curves_match_analytic_discount_and_forward(rate in -0.1_f64..0.2_f64,t1 in 0.0_f64..4.0_f64,period in 0.01_f64..2.0_f64) {
        let mut r=recipe();r.interpolation=Int::Linear;r.representation=CurveRepresentation::ZeroRates{compounding:Compounding::Continuous};r.source=CurveSource::Nodes(vec![node(0.0,rate,QuoteUnit::DecimalRate),node(6.0,rate,QuoteUnit::DecimalRate)]);let c=build(r);
        let df=query(&c,CurveQuery::DiscountFactor{point:CurvePoint::Tenor(t1+period)});prop_assert!((df-(-rate*(t1+period)).exp()).abs()<=1e-12);
        let fwd=query(&c,CurveQuery::ForwardRate{start:CurvePoint::Tenor(t1),end:CurvePoint::Tenor(t1+period),compounding:Compounding::Continuous,day_count:Dc::Act365Fixed});prop_assert!((fwd-rate).abs()<=1e-11);
    }
}

#[test]
fn pinned_analytic_fixture_is_reachable_through_the_pure_contract() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/curve-contracts.json")).unwrap();
    let recipe: CurveRecipe = serde_json::from_value(fixture["recipe"].clone()).unwrap();
    let c = build(recipe);
    let absolute = fixture["absolute_tolerance"].as_f64().unwrap();
    let relative = fixture["relative_tolerance"].as_f64().unwrap();
    let expected = fixture["expected"]["discount_1_5"].as_f64().unwrap();
    let actual = query(
        &c,
        CurveQuery::DiscountFactor {
            point: CurvePoint::Tenor(1.5),
        },
    );
    assert!((actual - expected).abs() <= absolute + relative * expected.abs());
    let expected = fixture["expected"]["forward_continuous"].as_f64().unwrap();
    let actual = query(
        &c,
        CurveQuery::ForwardRate {
            start: CurvePoint::Tenor(0.5),
            end: CurvePoint::Tenor(1.5),
            compounding: Compounding::Continuous,
            day_count: Dc::Act365Fixed,
        },
    );
    assert!((actual - expected).abs() <= absolute + relative * expected.abs());
}

#[test]
fn checked_raw_tail_cannot_use_approximate_flat_forward_or_panic_on_bad_ufr() {
    for extrapolation in [
        Ext::FlatForward,
        Ext::UfrConvergence {
            ufr: 0.03,
            alpha: -0.1,
        },
    ] {
        let c = DiscreteCurve::with_extrapolation(
            date("2024-01-01"),
            vec![1.0, 2.0],
            vec![0.04, 0.05],
            ValueType::zero_rate(Compounding::Continuous),
            Int::Linear,
            extrapolation,
        )
        .unwrap();
        assert!(c.checked_value_at(3.0).is_err());
    }
}

#[test]
fn projection_helpers_must_match_the_declared_index_role() {
    let mut r = recipe();
    r.role = CurveRole::Projection {
        index_id: "USD.TERM.3M".into(),
    };
    r.calibration = CalibrationMode::Piecewise;
    r.source = CurveSource::MoneyMarket(vec![
        helper(
            "deposit",
            "2024-01-01",
            "2024-04-01",
            0.05,
            QuoteUnit::DecimalRate,
        ),
        MoneyMarketHelper {
            id: "fra".into(),
            terms: MoneyMarketTerms::Fra {
                index_id: "USD.TERM.6M".into(),
                fixing_date: date("2024-03-28"),
                start_date: date("2024-04-01"),
                end_date: date("2024-07-01"),
                settlement_date: date("2024-07-01"),
                settlement: FraSettlement::AtEnd,
                day_count: Dc::Act360,
            },
            quote: quote(0.055, QuoteUnit::DecimalRate),
            tolerance: 1e-10,
        },
    ]);
    assert!(
        matches!(build_curve_set(&[r],&CurveSet::new()),Err(CurveContractError::InvalidInput{field,..}) if field=="helper.index_id")
    );
}

#[test]
fn global_description_keeps_parameter_and_observation_dimensions_independent() {
    let mut r = recipe();
    r.calibration = CalibrationMode::Global(GlobalFitSettings {
        max_iterations: 100,
        objective_tolerance: 1e-10,
        initial_zeros: vec![0.04],
        lower_zero: -0.1,
        upper_zero: 0.5,
        quote_scales: vec![1.0, 100.0],
        weights: vec![1.0, 2.0],
    });
    assert!(matches!(
        build_curve_set(&[r], &CurveSet::new()),
        Err(CurveContractError::Unsupported { .. })
    ));
}

#[test]
fn supported_cubic_interpolation_checks_actual_discount_postconditions() {
    let mut r = recipe();
    r.interpolation = Int::CubicSpline;
    r.representation = CurveRepresentation::ZeroRates {
        compounding: Compounding::Continuous,
    };
    r.source = CurveSource::Nodes(
        (0..=3)
            .map(|t| node(f64::from(t), -0.01, QuoteUnit::DecimalRate))
            .collect(),
    );
    let c = build(r);
    near(
        query(
            &c,
            CurveQuery::DiscountFactor {
                point: CurvePoint::Tenor(1.5),
            },
        ),
        0.015_f64.exp(),
    );
    let mut r = recipe();
    r.interpolation = Int::CubicSpline;
    r.source = CurveSource::Nodes(vec![
        node(0.0, 1.0, QuoteUnit::DiscountFactor),
        node(1.0, 0.01, QuoteUnit::DiscountFactor),
        node(2.0, 0.01, QuoteUnit::DiscountFactor),
        node(3.0, 1.0, QuoteUnit::DiscountFactor),
    ]);
    let c = build(r);
    assert!(matches!(
        c.query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Tenor(1.5)
        }),
        Err(CurveContractError::Numerical(_))
    ));
}
