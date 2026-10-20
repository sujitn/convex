//! CVX-058 original-cash, native quote, separated credit/repo and rebuild proof.
#[path = "../../convex/examples/support/cash_fixture.rs"]
mod fixture;
#[path = "support/futures.rs"]
mod futures_fixture;
use convex_analytics::credit_calibration::{build_credit_curve_set, CdsCurveBuildInput};
use convex_analytics::{cash_calibration::*, futures::*};
use convex_bonds::contracts::BondCalendars;
use convex_core::{
    cashflows::{CashRounding, DateOffset, PaymentDateEntitlement},
    contracts::{ExactDecimal, Version},
    daycounts::DayCountConvention as Dc,
    Date,
};
use convex_credit::CdsCalendars;
use convex_curves::{contracts::*, ExtrapolationMethod as Ex, InterpolationMethod as Int};
use convex_rates::{CalendarBinding, ReferencePin};
use fixture::*;
use rust_decimal::{prelude::ToPrimitive, Decimal};

fn d(s: &str) -> Date {
    Date::parse(s).unwrap()
}
fn k(s: &str) -> CurveKey {
    CurveKey {
        id: s.into(),
        version: "synthetic-1".into(),
    }
}
fn bond(h: &CashCalibrationHelper) -> &CashBondHelper {
    let CashCalibrationHelper::Bond { input } = h else {
        panic!("missing full cash bond")
    };
    input
}
fn bond_mut(h: &mut CashCalibrationHelper) -> &mut CashBondHelper {
    let CashCalibrationHelper::Bond { input } = h else {
        panic!("missing full cash bond")
    };
    input
}
fn repo_mut(h: &mut CashCalibrationHelper) -> &mut RepoLoanHelper {
    let CashCalibrationHelper::Repo { input } = h else {
        panic!("missing full repo loan")
    };
    input
}
fn run(
    f: &Fixture,
    recipes: &[CashCurveRecipe],
) -> Result<CashCurveBuildOutput, CashCalibrationError> {
    build(recipes, &f.resources(recipes), &f.supplied().unwrap())
}
fn ordered_ids(r: &mut CashCurveRecipe) {
    r.curve.source = CurveSource::Helpers(r.helpers.iter().map(|h| h.id().into()).collect());
}
fn native_records(output: &CashCurveBuildOutput) {
    assert!(output.price_evaluations > 0 && output.price_evaluations <= MAX_CASH_EVALUATIONS);
    for c in &output.calibrations {
        assert_eq!(c.recipe.helpers.len(), c.repriced_helpers.len());
        assert_eq!(c.recipe.helpers.len(), c.curve.diagnostics().quotes.len());
        for (h, q) in c.recipe.helpers.iter().zip(&c.curve.diagnostics().quotes) {
            assert_eq!(h.id(), q.helper_id);
            assert_eq!(h.quote(), q.input);
            assert_eq!(q.input.unit, q.model.unit);
            assert_eq!(h.tolerance(), q.tolerance);
            assert!(q.residual.abs() <= q.tolerance);
            assert_eq!(q.model.value - q.input.value, q.residual);
        }
    }
}

#[test]
fn original_full_cash_and_native_quotes_match_independent_positive_negative_piecewise() {
    for id in ["positive", "negative"] {
        let f = Fixture::load(id).unwrap();
        let output = run(&f, &f.recipes).unwrap();
        compare_reference(&f, &output).unwrap();
        native_records(&output);
        assert_eq!(
            output.build_order,
            vec![
                k(&format!("{id}.GOV")),
                k(&format!("{id}.BASIS")),
                k(&format!("{id}.REPO"))
            ]
        );
        assert!(output
            .calibrations
            .iter()
            .all(|c| matches!(c.solver, CashSolverDiagnostics::Piecewise { .. })));
        if id == "negative" {
            assert!(
                output
                    .curves
                    .get(&k("negative.GOV"))
                    .unwrap()
                    .query(&CurveQuery::DiscountFactor {
                        point: CurvePoint::Date(d("2027-04-07"))
                    })
                    .unwrap()
                    .value
                    > 1.
            );
        }
    }
}
#[test]
fn scaled_identified_global_reprices_every_original_cash_and_repo_observation() {
    for id in ["positive", "negative"] {
        let f = Fixture::load(id).unwrap();
        let output = run(&f, &global(&f.recipes)).unwrap();
        compare_reference(&f, &output).unwrap();
        native_records(&output);
        for c in &output.calibrations {
            let CashSolverDiagnostics::Global(g) = &c.solver else {
                panic!("missing measured global fit")
            };
            assert_eq!(g.rank, Some(c.recipe.pillars.len()));
        }
    }
}
#[test]
fn actual_clean_dirty_and_each_native_yield_unit_calibrate_identical_cash() {
    for id in ["positive", "negative"] {
        let f = Fixture::load(id).unwrap();
        let expected = &f.row["government_expected"][1];
        for (kind, quote) in [
            (
                CashQuoteKind::CleanPrice,
                MarketQuote {
                    value: number(&expected["clean_per100"]).unwrap(),
                    unit: QuoteUnit::PricePer100,
                },
            ),
            (
                CashQuoteKind::DirtyPrice,
                MarketQuote {
                    value: number(&expected["dirty_per100"]).unwrap(),
                    unit: QuoteUnit::PricePer100,
                },
            ),
            (
                CashQuoteKind::CashflowYield {
                    convention: CashYieldConvention {
                        day_count: Dc::Act365Fixed,
                        compounding: convex_core::types::Compounding::SemiAnnual,
                        lower_yield: -0.9,
                        upper_yield: 1.,
                        max_iterations: 256,
                        price_tolerance: 1e-11,
                    },
                },
                MarketQuote {
                    value: number(&expected["cashflow_yield_decimal"]).unwrap(),
                    unit: QuoteUnit::DecimalRate,
                },
            ),
            (
                CashQuoteKind::CashflowYield {
                    convention: CashYieldConvention {
                        day_count: Dc::Act365Fixed,
                        compounding: convex_core::types::Compounding::SemiAnnual,
                        lower_yield: -0.9,
                        upper_yield: 1.,
                        max_iterations: 256,
                        price_tolerance: 1e-11,
                    },
                },
                MarketQuote {
                    value: 100. * number(&expected["cashflow_yield_decimal"]).unwrap(),
                    unit: QuoteUnit::PercentRate,
                },
            ),
            (
                CashQuoteKind::CashflowYield {
                    convention: CashYieldConvention {
                        day_count: Dc::Act365Fixed,
                        compounding: convex_core::types::Compounding::SemiAnnual,
                        lower_yield: -0.9,
                        upper_yield: 1.,
                        max_iterations: 256,
                        price_tolerance: 1e-11,
                    },
                },
                MarketQuote {
                    value: 10000. * number(&expected["cashflow_yield_decimal"]).unwrap(),
                    unit: QuoteUnit::BasisPoints,
                },
            ),
        ] {
            let mut recipes = f.recipes.clone();
            let h = bond_mut(&mut recipes[2].helpers[1]);
            h.quote_kind = kind;
            h.quote = quote;
            h.tolerance = match quote.unit {
                QuoteUnit::DecimalRate => 1e-12,
                QuoteUnit::PercentRate => 1e-10,
                _ => 1e-8,
            };
            let resources = bond_resources(h, &f.calendar);
            let converted = convert_cash_quote(h, &resources, d("2026-10-07")).unwrap();
            compare(
                converted.clean_per100,
                number(&expected["clean_per100"]).unwrap(),
                1e-8,
                1e-11,
            )
            .unwrap();
            compare(
                converted.dirty_per100,
                number(&expected["dirty_per100"]).unwrap(),
                1e-8,
                1e-11,
            )
            .unwrap();
            compare_reference(&f, &run(&f, &recipes).unwrap()).unwrap();
        }
    }
}
#[test]
fn bill_bank_discount_price_and_cashflow_yield_are_distinct_explicit_quotes() {
    let f = Fixture::load("positive").unwrap();
    let expected = &f.row["government_expected"][0];
    for kind in [
        CashQuoteKind::CleanPrice,
        CashQuoteKind::DirtyPrice,
        CashQuoteKind::CashflowYield {
            convention: CashYieldConvention {
                day_count: Dc::Act365Fixed,
                compounding: convex_core::types::Compounding::SemiAnnual,
                lower_yield: -0.9,
                upper_yield: 1.,
                max_iterations: 256,
                price_tolerance: 1e-11,
            },
        },
    ] {
        let mut recipes = f.recipes.clone();
        let h = bond_mut(&mut recipes[2].helpers[0]);
        h.quote_kind = kind;
        h.quote = if matches!(kind, CashQuoteKind::CashflowYield { .. }) {
            MarketQuote {
                value: number(&expected["cashflow_yield_decimal"]).unwrap(),
                unit: QuoteUnit::DecimalRate,
            }
        } else {
            MarketQuote {
                value: number(&expected["dirty_per100"]).unwrap(),
                unit: QuoteUnit::PricePer100,
            }
        };
        h.tolerance = 1e-10;
        compare_reference(&f, &run(&f, &recipes).unwrap()).unwrap();
    }
    assert_ne!(
        bond(&f.recipes[2].helpers[0]).quote.decimal_rate().unwrap(),
        number(&expected["cashflow_yield_decimal"]).unwrap()
    );
}
#[test]
fn par_coupon_is_a_full_cash_helper_and_cannot_be_substituted_for_zero_rate() {
    let f = Fixture::load("positive").unwrap();
    let output = run(&f, &f.recipes).unwrap();
    let c = output
        .calibrations
        .iter()
        .find(|c| matches!(c.curve.recipe().role, CurveRole::Government { .. }))
        .unwrap();
    let par = bond(&c.recipe.helpers[2]);
    let quoted_coupon = par.quote.decimal_rate().unwrap();
    compare(c.annualized_parameters[2], 0.042, 1e-9, 1e-10).unwrap();
    assert_ne!(c.annualized_parameters[2], quoted_coupon);
    let CashHelperValuation::Bond { value, .. } = &c.repriced_helpers[2] else {
        panic!("missing original par cash")
    };
    compare(value.clean_price, 100., 1e-8, 1e-11).unwrap();
    assert_eq!(value.projection.coupons.len(), 4);
    let mut wrong = c.curve.recipe().clone();
    wrong.key = k("incorrect.par-as-zero");
    wrong.calibration = CalibrationMode::Direct;
    wrong.dependencies.clear();
    wrong.source = CurveSource::Nodes(vec![
        CurveNode {
            pillar: CurvePoint::Date(wrong.reference_date),
            quote: MarketQuote {
                value: quoted_coupon,
                unit: QuoteUnit::DecimalRate,
            },
        },
        CurveNode {
            pillar: CurvePoint::Date(par.pillar),
            quote: MarketQuote {
                value: quoted_coupon,
                unit: QuoteUnit::DecimalRate,
            },
        },
    ]);
    let w = build_curve_set(&[wrong.clone()], &CurveSet::new())
        .unwrap()
        .curves;
    let original = par
        .terms
        .clone()
        .build_with_calendars(&BondCalendars {
            payment: &f.calendar,
            reset: &f.calendar,
            observation: &f.calendar,
            exercise: &f.calendar,
        })
        .unwrap();
    let value = convex_analytics::bond_contract::price_fixed_government(
        &original,
        par.settlement,
        par.on_payment,
        w.get(&wrong.key).unwrap(),
        "USD.SYNTHETIC.GOV",
    )
    .unwrap();
    assert!(compare(value.clean_price, 100., 1e-8, 1e-11).is_err());
}
#[test]
fn full_steps_principal_and_calendar_settlement_are_retained_without_averages() {
    let f = Fixture::load("positive").unwrap();
    let output = run(&f, &f.recipes).unwrap();
    let original = bond(&f.recipes[2].helpers[1]);
    assert_eq!(original.settlement, d("2026-10-13"));
    assert_eq!(original.trade_date, d("2026-10-07"));
    let gov = output
        .calibrations
        .iter()
        .find(|c| matches!(c.curve.recipe().role, CurveRole::Government { .. }))
        .unwrap();
    assert_eq!(bond(&gov.recipe.helpers[1]).terms, original.terms);
    let CashHelperValuation::Bond { value, .. } = &gov.repriced_helpers[1] else {
        panic!("missing actual security cash")
    };
    assert_eq!(value.projection.principal.len(), 2);
    assert_eq!(value.projection.principal[0].amount, Decimal::from(2000));
    assert_eq!(value.projection.principal[1].amount, Decimal::from(3000));
    assert_eq!(bond(&gov.recipe.helpers[2]).pillar, d("2028-10-09"));
    assert_eq!(
        bond(&gov.recipe.helpers[2]).terms.schedule.end_date,
        d("2028-10-07")
    );
}
#[test]
fn full_cds_curve_dependency_and_explicit_par_recovery_match_independent_cash() {
    let c = CreditFixture::load().unwrap();
    for recipes in [
        vec![c.recipe.clone()],
        global(std::slice::from_ref(&c.recipe)),
    ] {
        let output = build(&recipes, &resources(&recipes, &c.calendar), &c.supplied).unwrap();
        native_records(&output);
        let fitted = &output.calibrations[0];
        for (&actual, expected) in fitted.annualized_parameters.iter().zip(
            c.data["credit_case"]["expected_parameters"]
                .as_array()
                .unwrap(),
        ) {
            compare(actual, number(expected).unwrap(), 1e-9, 1e-10).unwrap();
        }
        for (value, expected) in fitted
            .repriced_helpers
            .iter()
            .zip(c.data["credit_case"]["expected"].as_array().unwrap())
        {
            let CashHelperValuation::Bond { value, basis, .. } = value else {
                panic!("missing original credit-bond cash")
            };
            compare_price(value, expected, &c.data["tolerances"]).unwrap();
            let basis = basis.as_ref().unwrap();
            assert!(basis.recovery_pv > 0.);
            assert_eq!(
                basis.price.present_value,
                basis.promised_pv + basis.recovery_pv
            );
            compare(
                basis.recovery_pv,
                number(&expected["recovery_pv"]).unwrap(),
                1e-6,
                1e-11,
            )
            .unwrap();
            compare(
                basis.promised_pv,
                number(&expected["promised_pv"]).unwrap(),
                1e-6,
                1e-11,
            )
            .unwrap();
        }
        assert_eq!(
            c.supplied
                .get(&c.cds.curve.key)
                .unwrap()
                .diagnostics()
                .quotes
                .len(),
            6
        );
        let b = &fitted.curve;
        assert!(b
            .query(&CurveQuery::DiscountFactor {
                point: CurvePoint::Date(recipes[0].pillars[0])
            })
            .is_err());
        assert!(b
            .query(&CurveQuery::SurvivalProbability {
                point: CurvePoint::Date(recipes[0].pillars[0])
            })
            .is_err());
    }
}
#[test]
fn repo_full_dates_haircut_and_each_coupon_horizon_match_independent_carry() {
    for id in ["positive", "negative"] {
        let f = Fixture::load(id).unwrap();
        let out = run(&f, &f.recipes).unwrap();
        for (t, expected) in f.forwards.iter().zip(f.row["carry"].as_array().unwrap()) {
            let r = f.forward_resources(t);
            let v = price_repo_bond_forward(&RepoBondForwardInputs {
                terms: t,
                resources: &r,
                delivery_calendar: f.delivery_calendar(),
                curves: &out.curves,
            })
            .unwrap();
            let e = &expected["expected"];
            for (actual, field) in [
                (v.spot.dirty_per100, "spot_dirty_per100"),
                (v.spot.accrued_per100, "spot_accrued_per100"),
                (v.delivery_accrued_per100, "delivery_accrued_per100"),
                (v.repo.growth, "repo_growth"),
                (v.repo_cost_per100, "repo_cost_per100"),
                (v.own_cash_cost_per100, "own_cost_per100"),
                (v.dirty_forward_per100, "dirty_forward_per100"),
                (v.clean_forward_per100, "clean_forward_per100"),
            ] {
                compare(actual, number(&e[field]).unwrap(), 1e-8, 1e-11).unwrap();
            }
            compare(
                v.futures_price_points(
                    ExactDecimal::parse(text(expected, "conversion_factor").unwrap()).unwrap(),
                )
                .unwrap(),
                number(&e["futures_price_points"]).unwrap(),
                1e-8,
                1e-11,
            )
            .unwrap();
            assert_eq!(v.coupons.len(), 1);
            assert_eq!(v.coupons[0].payment_date, d("2026-10-15"));
            for (a, e) in v.coupons.iter().zip(e["coupons"].as_array().unwrap()) {
                compare(
                    a.cash_per100,
                    number(&e["cash_per100"]).unwrap(),
                    1e-8,
                    1e-11,
                )
                .unwrap();
                compare(a.growth, number(&e["growth"]).unwrap(), 1e-10, 1e-11).unwrap();
            }
            assert_ne!(v.repo.start, v.coupons[0].payment_date);
            assert!(v.own_cash.is_some() == !t.haircut_fraction.decimal().is_zero());
            if id == "negative" {
                assert!(v.repo_cost_per100 < 0.);
            }
        }
    }
}
#[test]
fn unordered_inputs_are_deterministic_and_global_scales_follow_original_units() {
    let f = Fixture::load("positive").unwrap();
    for recipes in [f.recipes.clone(), global(&f.recipes)] {
        let first = run(&f, &recipes).unwrap();
        let mut changed = recipes.clone();
        changed.reverse();
        for r in &mut changed {
            r.helpers.reverse();
            ordered_ids(r);
            if let CalibrationMode::Global(g) = &mut r.curve.calibration {
                g.quote_scales.reverse();
                g.weights.reverse();
            }
        }
        let second = run(&f, &changed).unwrap();
        assert_eq!(first.build_order, second.build_order);
        for (a, b) in first.calibrations.iter().zip(&second.calibrations) {
            for (&a, &b) in a.annualized_parameters.iter().zip(&b.annualized_parameters) {
                compare(a, b, 1e-10, 1e-10).unwrap();
            }
        }
    }
}
#[test]
fn quote_rebuild_propagates_exact_versions_preserving_originals_and_last_good() {
    let f = Fixture::load("positive").unwrap();
    for recipes in [f.recipes.clone(), global(&f.recipes)] {
        let old = run(&f, &recipes).unwrap();
        let saved = old
            .calibrations
            .iter()
            .map(|c| c.annualized_parameters.clone())
            .collect::<Vec<_>>();
        let mut new = recipes.clone();
        let h = bond_mut(&mut new[2].helpers[0]);
        let original = h.clone();
        let replacement = h
            .with_quote(MarketQuote {
                value: h.quote.value + 0.01,
                unit: h.quote.unit,
            })
            .unwrap();
        assert_eq!(replacement.terms, original.terms);
        assert_eq!(replacement.settlement, original.settlement);
        *h = replacement;
        h.quote_version = Version::new(2).unwrap();
        for r in &mut new {
            r.curve.key.version = "synthetic-2".into();
            for dep in &mut r.curve.dependencies {
                dep.version = "synthetic-2".into();
            }
            if let Some(BondBasisModel::ReferenceZeroSpread { reference, .. }) = &mut r.basis_model
            {
                reference.version = "synthetic-2".into();
            }
        }
        let changed = run(&f, &new).unwrap();
        native_records(&changed);
        let gov = changed
            .calibrations
            .iter()
            .find(|c| matches!(c.curve.recipe().role, CurveRole::Government { .. }))
            .unwrap();
        let basis = changed
            .calibrations
            .iter()
            .find(|c| matches!(c.curve.recipe().role, CurveRole::IssuerSpread { .. }))
            .unwrap();
        assert!((gov.annualized_parameters[0] - saved[0][0]).abs() > 1e-5);
        assert!((basis.annualized_parameters[0] - saved[1][0]).abs() > 1e-5);
        for (c, p) in old.calibrations.iter().zip(saved) {
            assert_eq!(c.annualized_parameters, p);
            assert_eq!(c.curve.recipe().key.version, "synthetic-1");
        }
        let mut broken = new.clone();
        broken[0].curve.dependencies[0].version = "missing".into();
        assert!(run(&f, &broken).is_err());
        assert_eq!(old.curves.len(), 5);
    }
}
#[test]
fn missing_versions_unused_dependencies_cycles_and_existing_identity_fail_atomically() {
    let f = Fixture::load("positive").unwrap();
    let original = run(&f, &f.recipes).unwrap();
    for changed in 0..4 {
        let mut r = f.recipes.clone();
        match changed {
            0 => {
                r[0].curve.dependencies[0].version = "missing".into();
                if let Some(BondBasisModel::ReferenceZeroSpread { reference, .. }) =
                    &mut r[0].basis_model
                {
                    reference.version = "missing".into();
                }
            }
            1 => r[1].curve.dependencies.push(k("unused")),
            2 => {
                let dep = r[0].curve.key.clone();
                r[2].curve.dependencies.push(dep);
            }
            _ => r[2].curve.key = r[1].curve.key.clone(),
        };
        assert!(run(&f, &r).is_err());
    }
    assert!(build(&f.recipes, &f.resources(&f.recipes), &original.curves).is_err());
    assert_eq!(
        original
            .curves
            .get(&k("positive.GOV"))
            .unwrap()
            .recipe()
            .key
            .version,
        "synthetic-1"
    );
}
#[test]
fn original_trade_settlement_accrued_calendar_and_cash_pillar_errors_are_rejected() {
    let f = Fixture::load("positive").unwrap();
    for mutation in 0..6 {
        let mut r = f.recipes.clone();
        let h = bond_mut(&mut r[2].helpers[2]);
        match mutation {
            0 => h.settlement = d("2026-10-08"),
            1 => h.trade_date = d("2026-10-08"),
            2 => h.pillar = h.terms.schedule.end_date,
            3 => h.terms.currency = convex_core::Currency::EUR,
            4 => h.terms.profiles.coupon[0].value = Decimal::from(1),
            _ => h.quote.unit = QuoteUnit::PricePer100,
        };
        assert!(run(&f, &r).is_err());
    }
    let resources = f.resources(&f.recipes);
    let mut bad = f.recipes.clone();
    bond_mut(&mut bad[2].helpers[1]).terms.calendars.payment = "WRONG.v2".into();
    assert!(build(&bad, &resources, &f.supplied().unwrap()).is_err());
}
#[test]
fn overlaps_require_explicit_global_retention_and_inconsistency_cannot_drop_quotes() {
    let f = Fixture::load("positive").unwrap();
    let mut r = global(&f.recipes);
    let mut duplicate = r[1].helpers[0].clone();
    repo_mut(&mut duplicate).id = "retained.same-date".into();
    r[1].helpers.push(duplicate);
    ordered_ids(&mut r[1]);
    if let CalibrationMode::Global(g) = &mut r[1].curve.calibration {
        g.quote_scales.push(g.quote_scales[0]);
        g.weights.push(1.);
    }
    assert!(run(&f, &r).is_err());
    r[1].overlap = CashOverlapPolicy::RetainAllGlobal;
    let out = run(&f, &r).unwrap();
    let repo = out
        .calibrations
        .iter()
        .find(|c| matches!(c.curve.recipe().role, CurveRole::Repo { .. }))
        .unwrap();
    assert_eq!(repo.curve.diagnostics().quotes.len(), 4);
    repo_mut(&mut r[1].helpers[3]).quote.value += 0.01;
    assert!(run(&f, &r).is_err());
}
#[test]
fn rank_deficiency_and_iteration_failures_return_measured_failures_without_curves() {
    let f = Fixture::load("positive").unwrap();
    let mut r = global(&f.recipes);
    let template = r[1].helpers[0].clone();
    for (i, h) in r[1].helpers.iter_mut().enumerate() {
        *h = template.clone();
        repo_mut(h).id = format!("same-exposure.{i}");
    }
    r[1].overlap = CashOverlapPolicy::RetainAllGlobal;
    ordered_ids(&mut r[1]);
    assert!(matches!(run(&f, &r), Err(CashCalibrationError::Global(_))));
    let mut r = global(&f.recipes);
    if let CalibrationMode::Global(g) = &mut r[2].curve.calibration {
        g.max_iterations = 1;
        g.initial_zeros.fill(0.09);
    }
    assert!(matches!(run(&f, &r), Err(CashCalibrationError::Global(_))));
    let mut r = f.recipes.clone();
    r[2].bootstrap.as_mut().unwrap().max_iterations = 1;
    assert!(run(&f, &r).is_err());
}
#[test]
fn unsupported_methods_bad_native_units_and_resource_limits_never_fallback() {
    let f = Fixture::load("positive").unwrap();
    for mutation in 0..7 {
        let mut r = f.recipes.clone();
        match mutation {
            0 => r[2].curve.interpolation = Int::CubicSpline,
            1 => r[2].curve.representation = CurveRepresentation::DiscountFactors,
            2 => r[2].curve.calibration = CalibrationMode::Direct,
            3 => r[2].bootstrap.as_mut().unwrap().upper_parameter = f64::NAN,
            4 => repo_mut(&mut r[1].helpers[0]).quote.unit = QuoteUnit::UpfrontFraction,
            5 => r[2].pillars[0] = r[2].curve.reference_date,
            _ => bond_mut(&mut r[2].helpers[2]).terms.rounding = CashRounding::NearestEven(2),
        };
        assert!(run(&f, &r).is_err());
    }
    let r = vec![f.recipes[2].clone(); MAX_CURVES + 1];
    assert!(run(&f, &r).is_err());
    let mut r = vec![f.recipes[2].clone()];
    let first = r[0].helpers[0].clone();
    r[0].helpers.resize(MAX_CASH_HELPERS + 1, first);
    assert!(run(&f, &r).is_err());
    let mut h = bond(&f.recipes[2].helpers[0]).clone();
    h.terms
        .profiles
        .coupon
        .resize(4097, h.terms.profiles.coupon[0]);
    assert!(h.with_quote(h.quote).is_err());
}
#[test]
fn strict_original_native_json_roundtrip_rejects_inapplicable_fields() {
    let f = Fixture::load("positive").unwrap();
    let data = serde_json::to_value(&f.recipes).unwrap();
    assert_eq!(
        serde_json::from_value::<Vec<CashCurveRecipe>>(data.clone()).unwrap(),
        f.recipes
    );
    for field in ["hazard", "implicit_discount", "dropped_quote"] {
        let mut bad = data[0].clone();
        bad[field] = serde_json::json!(0);
        assert!(serde_json::from_value::<CashCurveRecipe>(bad).is_err());
    }
    let mut bad = serde_json::to_value(&f.recipes[1].helpers[0]).unwrap();
    bad["input"]["haircut"] = serde_json::json!(0);
    assert!(serde_json::from_value::<CashCalibrationHelper>(bad).is_err());
}
#[test]
fn explicit_credit_entity_seniority_recovery_roles_and_integration_are_enforced() {
    let c = CreditFixture::load().unwrap();
    for mutation in 0..6 {
        let mut r = vec![c.recipe.clone()];
        match mutation {
            0 => {
                bond_mut(&mut r[0].helpers[0])
                    .credit_identity
                    .as_mut()
                    .unwrap()
                    .entity_id = "WRONG".to_string().try_into().unwrap()
            }
            1 => bond_mut(&mut r[0].helpers[0]).credit_identity = None,
            2 => {
                if let Some(BondBasisModel::CreditParRecovery { recovery, .. }) =
                    &mut r[0].basis_model
                {
                    *recovery = 0.5;
                }
            }
            3 => {
                if let Some(BondBasisModel::CreditParRecovery { seniority, .. }) =
                    &mut r[0].basis_model
                {
                    *seniority = "Subordinated".into();
                }
            }
            4 => {
                if let CurveRole::IssuerSpread { model_id, .. } = &mut r[0].curve.role {
                    *model_id = "unimplemented".into();
                }
            }
            _ => {
                bond_mut(&mut r[0].helpers[0]).terms.dates.payment_delay =
                    DateOffset::CalendarDays(2)
            }
        };
        assert!(build(&r, &resources(&r, &c.calendar), &c.supplied).is_err());
    }
    let key = &c.cds.curve.dependencies[0];
    let mut recipe = c.supplied.get(key).unwrap().recipe().clone();
    recipe.interpolation = Int::Linear;
    let supplied = build_curve_set(&[recipe], &CurveSet::new()).unwrap().curves;
    let supplied = supplied
        .with_curve(c.supplied.get(&c.cds.curve.key).unwrap().as_ref().clone())
        .unwrap();
    assert!(build(
        std::slice::from_ref(&c.recipe),
        &resources(std::slice::from_ref(&c.recipe), &c.calendar),
        &supplied
    )
    .is_err());
}
#[test]
fn collateral_discount_government_and_repo_cannot_replace_named_roles() {
    let f = Fixture::load("positive").unwrap();
    let mut r = f.recipes.clone();
    if let Some(BondBasisModel::ReferenceZeroSpread { role, .. }) = &mut r[0].basis_model {
        *role = BondReferenceRole::Discount;
    }
    assert!(run(&f, &r).is_err());
    let out = run(&f, &f.recipes).unwrap();
    let gov = RepoFundingCurve {
        curve: k("positive.GOV"),
        collateral_id: "USD.SYNTHETIC.BOND".into(),
    };
    assert!(repo_rate_over_period(
        &out.curves,
        &gov,
        convex_core::Currency::USD,
        d("2026-10-13"),
        d("2026-12-16"),
        Dc::Act360
    )
    .is_err());
    let mut t = f.forwards[0].clone();
    t.repo.collateral_id = "wrong".into();
    let resources = f.forward_resources(&t);
    assert!(price_repo_bond_forward(&RepoBondForwardInputs {
        terms: &t,
        resources: &resources,
        delivery_calendar: f.delivery_calendar(),
        curves: &out.curves
    })
    .is_err());
}
#[test]
fn missing_haircut_reinvestment_wrong_delivery_and_repo_history_bounds_fail() {
    let f = Fixture::load("positive").unwrap();
    let out = run(&f, &f.recipes).unwrap();
    for mutation in 0..7 {
        let mut t = f.forwards[1].clone();
        match mutation {
            0 => t.own_cash = None,
            1 => t.haircut_fraction = ExactDecimal::parse("1").unwrap(),
            2 => t.delivery = d("2026-10-10"),
            3 => t.delivery = t.spot.settlement,
            4 => t.repo.curve.version = "missing".into(),
            5 => {
                if let RepoCouponCarry::Reinvested { funding } = &mut t.coupons {
                    funding.curve.version = "missing".into();
                }
            }
            _ => t.delivery = d("2027-10-07"),
        };
        let r = f.forward_resources(&t);
        assert!(price_repo_bond_forward(&RepoBondForwardInputs {
            terms: &t,
            resources: &r,
            delivery_calendar: f.delivery_calendar(),
            curves: &out.curves
        })
        .is_err());
    }
    let mut r = f.recipes.clone();
    repo_mut(&mut r[1].helpers[0]).start_date = d("2026-10-10");
    assert!(run(&f, &r).is_err());
}

fn dated_funding(
    id: &str,
    collateral: &str,
    rate: f64,
    basis: Dc,
    start: Date,
    end: Date,
    extra: &[Date],
) -> CurveRecipe {
    let mut dates = vec![d("2026-10-07"), start, end];
    dates.extend_from_slice(extra);
    dates.sort();
    dates.dedup();
    let end_df = 1. / (1. + rate * year_fraction(start, end, basis).unwrap());
    let nodes = dates
        .into_iter()
        .map(|date| {
            let value = if date <= start {
                1.
            } else if date == end {
                end_df
            } else {
                end_df * (1. + rate * year_fraction(date, end, basis).unwrap())
            };
            CurveNode {
                pillar: CurvePoint::Date(date),
                quote: MarketQuote {
                    value,
                    unit: QuoteUnit::DiscountFactor,
                },
            }
        })
        .collect();
    CurveRecipe {
        key: k(id),
        role: CurveRole::Repo {
            collateral_id: collateral.into(),
        },
        currency: convex_core::Currency::USD,
        reference_date: d("2026-10-07"),
        time_basis: Dc::Act365Fixed,
        representation: CurveRepresentation::DiscountFactors,
        interpolation: Int::LogLinear,
        extrapolation: Ex::None,
        dependencies: vec![],
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(nodes),
    }
}
#[test]
fn dated_repo_structures_reproduce_all_original_cvx023_cash_futures_alternatives() {
    let p = futures_fixture::BondPack::new();
    let deliverables = p.deliverables();
    let prior = price_bond_future(&p.inputs(&deliverables)).unwrap();
    for (i, b) in p.bonds.iter().enumerate() {
        let start = d("2026-10-08");
        let end = d("2026-12-08");
        let terms = b.terms().clone();
        let profile = b
            .fixed_projection(start, PaymentDateEntitlement::Exclude)
            .unwrap();
        let coupon_dates: Vec<_> = profile
            .coupons
            .iter()
            .filter(|c| c.payment_date > start && c.payment_date < end)
            .map(|c| c.payment_date)
            .collect();
        let mut curves = vec![dated_funding(
            "repo",
            "actual-security-repo",
            p.financing[i].repo_decimal.decimal().to_f64().unwrap(),
            p.financing[i].day_count,
            start,
            end,
            &[],
        )];
        let own = p.own[i].as_ref().map(|r| {
            curves.push(dated_funding(
                "own",
                "separate-own-cash",
                r.decimal_rate.decimal().to_f64().unwrap(),
                r.day_count,
                start,
                end,
                &[],
            ));
            RepoFundingCurve {
                curve: k("own"),
                collateral_id: "separate-own-cash".into(),
            }
        });
        let coupons = match &p.coupon[i] {
            FuturesCouponCarry::HeldCash { evidence } => RepoCouponCarry::HeldCash {
                evidence: evidence.clone(),
            },
            FuturesCouponCarry::Reinvested { rate } => {
                curves.push(dated_funding(
                    "coupon",
                    "separate-coupon-cash",
                    rate.decimal_rate.decimal().to_f64().unwrap(),
                    rate.day_count,
                    start,
                    end,
                    &coupon_dates,
                ));
                RepoCouponCarry::Reinvested {
                    funding: RepoFundingCurve {
                        curve: k("coupon"),
                        collateral_id: "separate-coupon-cash".into(),
                    },
                }
            }
        };
        let supplied = build_curve_set(&curves, &CurveSet::new()).unwrap().curves;
        let pin = ReferencePin {
            id: "supplied.delivery.calendar".to_string().try_into().unwrap(),
            version: Version::new(1).unwrap(),
        };
        let mut spot = CashBondHelper {
            id: format!("old.cash.{i}"),
            quote_version: Version::new(1).unwrap(),
            pillar: terms.schedule.end_date,
            trade_date: d("2026-10-07"),
            settlement: start,
            on_payment: PaymentDateEntitlement::Exclude,
            terms,
            credit_identity: None,
            quote_kind: CashQuoteKind::CleanPrice,
            quote: MarketQuote {
                value: p.prices[i].decimal().to_f64().unwrap(),
                unit: QuoteUnit::PricePer100,
            },
            tolerance: 1e-8,
        };
        // The earlier supplied futures fixture states purchase settlement directly.
        // Supply its actual synthetic T+1 trade convention for this checked helper;
        // coupon/principal/ex-date economics remain identical to that security.
        spot.terms.settlement_lag = DateOffset::BusinessDays(1);
        let forward = RepoBondForwardTerms {
            spot,
            delivery: end,
            delivery_calendar: pin.clone(),
            repo: RepoFundingCurve {
                curve: k("repo"),
                collateral_id: "actual-security-repo".into(),
            },
            haircut_fraction: p.financing[i].haircut_fraction,
            own_cash: own,
            coupons,
        };
        let resources = bond_resources(&forward.spot, &p.calendar);
        let output = price_repo_bond_forward(&RepoBondForwardInputs {
            terms: &forward,
            resources: &resources,
            delivery_calendar: CalendarBinding {
                pin: &pin,
                calendar: &p.calendar,
            },
            curves: &supplied,
        })
        .unwrap();
        let old = prior
            .alternatives
            .iter()
            .find(|a| a.instrument == p.basket.bonds[i].instrument)
            .unwrap();
        compare(
            output.repo_cost_per100,
            old.repo_cost_per100.decimal().to_f64().unwrap(),
            1e-8,
            1e-11,
        )
        .unwrap();
        compare(
            output.own_cash_cost_per100,
            old.own_cash_cost_per100.decimal().to_f64().unwrap(),
            1e-8,
            1e-11,
        )
        .unwrap();
        compare(
            output
                .futures_price_points(p.basket.bonds[i].conversion_factor)
                .unwrap(),
            old.forward_price_points.decimal().to_f64().unwrap(),
            1e-8,
            1e-11,
        )
        .unwrap();
    }
}
#[test]
fn shared_exact_default_mass_handles_negative_discount_rates_zero_hazard_and_invalid_probabilities()
{
    let value = convex_credit::discounted_default_mass(1., 1.02, 1., 0.98).unwrap();
    assert!(value > 0.02);
    assert_eq!(
        convex_credit::discounted_default_mass(1., 1.02, 1., 1.).unwrap(),
        0.
    );
    assert!(convex_credit::discounted_default_mass(1., 1., 0.9, 0.95).is_err());
    assert!(convex_credit::discounted_default_mass(0., 1., 1., 0.9).is_err());
}

#[test]
fn cds_quote_rebuild_changes_credit_and_residual_basis_without_merging_recovery_or_versions() {
    let c = CreditFixture::load().unwrap();
    let original = build(
        std::slice::from_ref(&c.recipe),
        &resources(std::slice::from_ref(&c.recipe), &c.calendar),
        &c.supplied,
    )
    .unwrap();
    let mut cds = c.cds.clone();
    cds.curve.key.version = "credit-bump-2".into();
    for h in &mut cds.helpers {
        h.terms.survival_curve = cds.curve.key.clone();
    }
    cds.helpers[0].quote.value += 1.0; // Actual original basis-point quotation.
    cds.helpers[0].quote_version = Version::new(2).unwrap();
    let bindings: Vec<_> = cds
        .helpers
        .iter()
        .map(|h| CdsCalendars {
            premium: convex_credit::CalendarBinding {
                pin: &h.terms.premium_calendar,
                calendar: &c.calendar,
            },
            settlement: convex_credit::CalendarBinding {
                pin: &h.terms.settlement_calendar,
                calendar: &c.calendar,
            },
        })
        .collect();
    let discount = CurveSet::new()
        .with_curve(
            c.supplied
                .get(&cds.curve.dependencies[0])
                .unwrap()
                .as_ref()
                .clone(),
        )
        .unwrap();
    let credit = build_credit_curve_set(
        &[CdsCurveBuildInput {
            recipe: &cds,
            calendars: &bindings,
        }],
        &discount,
        FitNumericalControls::default(),
    )
    .unwrap();
    assert_eq!(credit.calibrations[0].curve.recipe().role, c.cds.curve.role);
    assert_eq!(
        credit.calibrations[0].recipe.helpers[0].quote_version,
        Version::new(2).unwrap()
    );
    let mut basis = c.recipe.clone();
    basis.curve.key.version = "basis-rebuilt-2".into();
    let old_credit = c.cds.curve.key.clone();
    for key in &mut basis.curve.dependencies {
        if *key == old_credit {
            *key = cds.curve.key.clone();
        }
    }
    if let Some(BondBasisModel::CreditParRecovery { survival, .. }) = &mut basis.basis_model {
        *survival = cds.curve.key.clone();
    }
    let next = build(
        std::slice::from_ref(&basis),
        &resources(std::slice::from_ref(&basis), &c.calendar),
        &credit.curves,
    )
    .unwrap();
    native_records(&next);
    assert_eq!(next.calibrations[0].recipe.helpers, c.recipe.helpers);
    assert_eq!(
        next.calibrations[0].curve.recipe().dependencies,
        basis.curve.dependencies
    );
    assert!(next.calibrations[0]
        .annualized_parameters
        .iter()
        .zip(&original.calibrations[0].annualized_parameters)
        .any(|(a, b)| (a - b).abs() > 1e-9));
    assert!(build(
        std::slice::from_ref(&c.recipe),
        &resources(std::slice::from_ref(&c.recipe), &c.calendar),
        &credit.curves
    )
    .is_err());
    assert!(c.supplied.get(&old_credit).is_ok());
    assert!(c.supplied.get(&cds.curve.key).is_err());
    assert_eq!(original.calibrations[0].recipe, c.recipe);
}

#[test]
fn wrong_later_issuer_reference_fails_preflight_before_an_unbracketed_government_solve() {
    let f = Fixture::load("positive").unwrap();
    let mut recipes = f.recipes.clone();
    recipes[2].bootstrap.as_mut().unwrap().lower_parameter = 0.09;
    recipes[2].bootstrap.as_mut().unwrap().upper_parameter = 0.10;
    if let Some(BondBasisModel::ReferenceZeroSpread { role, .. }) = &mut recipes[0].basis_model {
        *role = BondReferenceRole::Discount;
    }
    assert!(
        matches!(run(&f, &recipes), Err(CashCalibrationError::Curve(CurveContractError::InvalidInput { field, .. })) if field == "reference role")
    );
}

#[test]
fn direct_signed_basis_has_explicit_extension_and_never_supplies_a_rate_or_probability() {
    let f = Fixture::load("positive").unwrap();
    let fitted = run(&f, &f.recipes).unwrap();
    let mut r = fitted
        .curves
        .get(&k("positive.BASIS"))
        .unwrap()
        .recipe()
        .clone();
    r.key = k("raw.basis");
    r.dependencies.clear();
    r.calibration = CalibrationMode::Direct;
    r.source = CurveSource::Nodes(vec![
        CurveNode {
            pillar: CurvePoint::Tenor(0.),
            quote: MarketQuote {
                value: -0.001,
                unit: QuoteUnit::DecimalRate,
            },
        },
        CurveNode {
            pillar: CurvePoint::Tenor(2.),
            quote: MarketQuote {
                value: 25.,
                unit: QuoteUnit::BasisPoints,
            },
        },
    ]);
    r.extrapolation = Ex::None;
    let a = build_curve_set(&[r.clone()], &CurveSet::new())
        .unwrap()
        .curves;
    compare(
        a.get(&r.key)
            .unwrap()
            .issuer_spread(CurvePoint::Tenor(1.))
            .unwrap(),
        0.00075,
        1e-14,
        0.,
    )
    .unwrap();
    assert!(a
        .get(&r.key)
        .unwrap()
        .issuer_spread(CurvePoint::Tenor(3.))
        .is_err());
    for query in [
        CurveQuery::DiscountFactor {
            point: CurvePoint::Tenor(1.),
        },
        CurveQuery::SurvivalProbability {
            point: CurvePoint::Tenor(1.),
        },
    ] {
        assert!(a.get(&r.key).unwrap().query(&query).is_err());
    }
    r.extrapolation = Ex::Flat;
    let flat = build_curve_set(&[r.clone()], &CurveSet::new())
        .unwrap()
        .curves;
    compare(
        flat.get(&r.key)
            .unwrap()
            .issuer_spread(CurvePoint::Tenor(3.))
            .unwrap(),
        0.0025,
        1e-14,
        0.,
    )
    .unwrap();
    r.extrapolation = Ex::Linear;
    let linear = build_curve_set(&[r.clone()], &CurveSet::new())
        .unwrap()
        .curves;
    compare(
        linear
            .get(&r.key)
            .unwrap()
            .issuer_spread(CurvePoint::Tenor(3.))
            .unwrap(),
        0.00425,
        1e-14,
        0.,
    )
    .unwrap();
    assert_eq!(
        curve_capabilities()
            .iter()
            .find(|c| c.name == "direct_issuer_basis")
            .unwrap()
            .status,
        CurveCapabilityStatus::Supported
    );
}

#[test]
fn yield_inverse_conventions_and_trial_brackets_fail_without_replacing_last_good_curves() {
    let f = Fixture::load("positive").unwrap();
    let good = run(&f, &f.recipes).unwrap();
    for mutation in 0..4 {
        let mut recipes = f.recipes.clone();
        let h = bond_mut(&mut recipes[0].helpers[1]);
        let CashQuoteKind::CashflowYield { convention } = &mut h.quote_kind else {
            panic!("dated yield helper required");
        };
        match mutation {
            0 => convention.day_count = Dc::ActActIcma,
            1 => convention.max_iterations = 0,
            2 => convention.price_tolerance = f64::NAN,
            _ => {
                let y = h.quote.decimal_rate().unwrap();
                convention.lower_yield = y - 0.00001;
                convention.upper_yield = y + 0.00001;
            }
        }
        assert!(run(&f, &recipes).is_err());
    }
    compare_reference(&f, &good).unwrap();
    assert_eq!(good.calibrations[1].recipe, f.recipes[0]);
}
