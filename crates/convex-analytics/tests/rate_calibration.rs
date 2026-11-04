//! CVX-057 original-unit mixed-curve, independent cash, rebuild and failure proof.
#[path = "../../convex/examples/support/rate_fixture.rs"]
mod fixture;
use convex_analytics::{futures::*, rate_calibration::*};
use convex_core::{
    cashflows::{CashRounding, CashflowError},
    contracts::{ExactDecimal, Version},
    fixings::{FixingSeries, RateOrigin},
    futures::{FuturesConvexityPolicy, FuturesSettlement, RateAveraging},
    Date,
};
use convex_curves::{contracts::*, ExtrapolationMethod, InterpolationMethod};
use convex_rates::{basis::*, pricing::SwapValuationInputs, FloatingLegTerms};
use fixture::*;
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde_json::Value;

fn d(s: &str) -> Date {
    Date::parse(s).unwrap()
}
fn e(s: &str) -> Decimal {
    ExactDecimal::parse(s).unwrap().decimal()
}
fn reference(v: &Value) -> f64 {
    v.as_str().unwrap().parse().unwrap()
}
fn close(actual: f64, expected: f64, absolute: f64, relative: f64) {
    assert!(
        actual.is_finite() && (actual - expected).abs() <= absolute + relative * expected.abs(),
        "actual {actual:.15} expected {expected:.15}"
    );
}
fn basis(f: &Fixture) -> (&BasisSwapTerms, BasisQuotedLeg) {
    let RateCalibrationTerms::BasisSwap { terms, quoted_leg } = &f.recipes[2].helpers[2].terms
    else {
        panic!("missing full basis");
    };
    (terms, *quoted_leg)
}
fn build<'a>(
    f: &'a Fixture,
    recipes: &'a [RateCurveRecipe],
    resources: &'a [Vec<RateHelperResources<'a>>],
    history: &FixingSeries,
) -> Result<RateCurveBuildOutput, RateCalibrationError> {
    let inputs: Vec<_> = recipes
        .iter()
        .zip(resources)
        .map(|(recipe, resources)| RateCurveBuildInput { recipe, resources })
        .collect();
    build_rate_curve_set(
        &inputs,
        &CurveSet::new(),
        history,
        f.context().unwrap(),
        FitNumericalControls::default(),
        |i, d| f.publication(i, d),
    )
}
fn validate_products(f: &Fixture, output: &RateCurveBuildOutput) {
    compare_reference(f, output).unwrap();
    assert_eq!(
        output
            .build_order
            .iter()
            .map(|k| k.id.as_str())
            .collect::<Vec<_>>(),
        ["D.OIS", "P.SOFR", "P.3M", "P.6M"]
    );
    assert_eq!(output.calibrations.len(), 3);
    let expected = &f.row["expected"];
    for c in &output.calibrations {
        assert_eq!(c.repriced_helpers.len(), c.recipe.helpers.len());
        assert_eq!(c.curve.diagnostics().quotes.len(), c.recipe.helpers.len());
        for (h, q) in c.recipe.helpers.iter().zip(&c.curve.diagnostics().quotes) {
            assert_eq!(h.quote, q.input);
            assert_eq!(q.input.unit, q.model.unit);
            assert_eq!(q.tolerance, h.tolerance);
            assert_eq!(q.helper_id, h.id);
        }
    }
    let RateHelperValuation::RateFuture(future) = &output.calibrations[0].repriced_helpers[1]
    else {
        panic!("missing future evidence");
    };
    assert_eq!(future.known_days, 21);
    assert_eq!(future.projected_days, 70);
    close(
        future.model_price_points.decimal().to_f64().unwrap(),
        reference(&expected["future_price"]),
        1e-10,
        0.,
    );
    for (v, r) in future
        .observations
        .iter()
        .zip(expected["future_observations"].as_array().unwrap())
    {
        assert_eq!(v.requirement.observation_date.to_string(), r["observation"]);
        assert_eq!(v.rate_end.to_string(), r["rate_end"]);
        close(
            v.decimal_rate.decimal().to_f64().unwrap(),
            reference(&r["rate"]),
            1e-12,
            0.,
        );
        assert_eq!(
            matches!(v.origin, FuturesRateOrigin::Published { .. }),
            r["known"].as_bool().unwrap()
        );
    }
    let RateHelperValuation::Swap(ois) = &output.calibrations[0].repriced_helpers[2] else {
        panic!("missing OIS");
    };
    close(ois.present_value.value().get(), 0., 1e-4, 0.);
    close(
        ois.floating_present_value.value().get(),
        reference(&expected["ois_floating_pv"]),
        1e-4,
        1e-11,
    );
    for (c, r) in ois
        .cashflows
        .floating
        .iter()
        .zip(expected["ois_coupons"].as_array().unwrap())
    {
        assert_eq!(c.period.payment_date().to_string(), r["payment"]);
        close(
            c.amount.to_f64().unwrap(),
            reference(&r["amount"]),
            1e-4,
            1e-11,
        );
    }
    let RateHelperValuation::Swap(irs) = &output.calibrations[1].repriced_helpers[2] else {
        panic!("missing IRS");
    };
    close(irs.present_value.value().get(), 0., 1e-4, 0.);
    close(
        irs.floating_present_value.value().get(),
        reference(&expected["irs_floating_pv"]),
        1e-4,
        1e-11,
    );
    assert!(irs
        .cashflows
        .rates
        .iter()
        .any(|r| matches!(r.resolved.origin, RateOrigin::Published(_))));
    for (c, r) in irs
        .cashflows
        .floating
        .iter()
        .zip(expected["irs_coupons"].as_array().unwrap())
    {
        assert_eq!(c.period.payment_date().to_string(), r["payment"]);
        close(
            c.amount.to_f64().unwrap(),
            reference(&r["amount"]),
            1e-4,
            1e-11,
        );
    }
    let RateHelperValuation::BasisSwap(basis) = &output.calibrations[2].repriced_helpers[2] else {
        panic!("missing basis");
    };
    close(basis.present_value.value().get(), 0., 1e-4, 0.);
    close(
        basis.pay.present_value.value().get(),
        reference(&expected["basis_pay_pv"]),
        1e-4,
        1e-11,
    );
    for (c, r) in basis
        .pay
        .cashflows
        .iter()
        .zip(expected["basis_pay_coupons"].as_array().unwrap())
    {
        assert_eq!(c.period.payment_date().to_string(), r["payment"]);
        close(
            c.signed_amount.to_f64().unwrap(),
            -reference(&r["amount"]),
            1e-4,
            1e-11,
        );
    }
    assert!(basis
        .pay
        .rates
        .iter()
        .all(|r| r.request.projection_curve.id == "P.3M"));
    assert!(basis
        .receive
        .rates
        .iter()
        .all(|r| r.request.projection_curve.id == "P.6M"));
}
#[test]
fn original_native_mixed_quotes_and_independent_full_cash_reprice_piecewise() {
    for id in ["nonflat-positive", "nonflat-negative"] {
        let f = Fixture::load(id).unwrap();
        let output = f.fit(&f.recipes).unwrap();
        validate_products(&f, &output);
        assert!(output
            .calibrations
            .iter()
            .all(|c| matches!(c.solver, RateSolverDiagnostics::Piecewise { .. })));
    }
}
#[test]
fn identified_scaled_global_fit_reprices_all_original_units_and_separate_curves() {
    for id in ["nonflat-positive", "nonflat-negative"] {
        let f = Fixture::load(id).unwrap();
        let output = f.fit(&f.global(InterpolationMethod::Linear)).unwrap();
        validate_products(&f, &output);
        for c in &output.calibrations {
            let RateSolverDiagnostics::Global(s) = &c.solver else {
                panic!("missing measured fit");
            };
            assert!(s.evaluations > 0);
        }
    }
}
#[test]
fn original_and_recipe_order_do_not_choose_a_different_financial_solution() {
    let f = Fixture::load("nonflat-positive").unwrap();
    for global in [false, true] {
        let mut recipes = if global {
            f.global(InterpolationMethod::Linear)
        } else {
            f.recipes.clone()
        };
        for r in &mut recipes {
            r.helpers.reverse();
            if let CurveSource::Helpers(ids) = &mut r.curve.source {
                ids.reverse();
            }
            if let CalibrationMode::Global(s) = &mut r.curve.calibration {
                s.quote_scales.reverse();
                s.weights.reverse();
            }
        }
        recipes.reverse();
        validate_products_reordered(&f, &f.fit(&recipes).unwrap());
    }
}
fn validate_products_reordered(f: &Fixture, o: &RateCurveBuildOutput) {
    compare_reference(f, o).unwrap();
    assert_eq!(
        o.build_order
            .iter()
            .map(|k| k.id.as_str())
            .collect::<Vec<_>>(),
        ["D.OIS", "P.SOFR", "P.3M", "P.6M"]
    );
    for c in &o.calibrations {
        for (h, q) in c.recipe.helpers.iter().zip(&c.curve.diagnostics().quotes) {
            assert_eq!(q.helper_id, h.id);
            assert_eq!(q.input, h.quote);
        }
    }
}
#[test]
fn missing_due_histories_across_products_fail_before_any_fit_or_forecast() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let empty_futures =
        FuturesFixings::new(vec![], FuturesFixingSelection::FirstPublished).unwrap();
    let mut resources = f.resources(&f.recipes);
    let RateHelperResources::RateFuture { fixings, .. } = &mut resources[0][1] else {
        panic!()
    };
    *fixings = &empty_futures;
    let err = build(
        &f,
        &f.recipes,
        &resources,
        &FixingSeries::new(vec![]).unwrap(),
    )
    .unwrap_err();
    let RateCalibrationError::Cashflow(CashflowError::MissingFixings(missing)) = err else {
        panic!("expected complete missing-history preflight")
    };
    assert!(missing.iter().any(|r| r.index_id.as_str() == "USD.SOFR"));
    assert!(missing.iter().any(|r| r.index_id.as_str() == "USD.TERM3M"));
    assert!(missing.iter().any(|r| r.index_id.as_str() == "USD.TERM6M"));
}
#[test]
fn publication_schedule_freezes_once_per_unique_index_date_for_whole_graph() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let resources = f.resources(&f.recipes);
    let inputs: Vec<_> = f
        .recipes
        .iter()
        .zip(&resources)
        .map(|(recipe, resources)| RateCurveBuildInput { recipe, resources })
        .collect();
    let mut calls = std::collections::BTreeMap::new();
    let output = build_rate_curve_set(
        &inputs,
        &CurveSet::new(),
        &f.history,
        f.context().unwrap(),
        FitNumericalControls::default(),
        |i, d| {
            let n = calls.entry((i.clone(), d)).or_insert(0);
            *n += 1;
            assert_eq!(*n, 1);
            f.publication(i, d)
        },
    )
    .unwrap();
    assert_eq!(calls.len(), output.publications.len());
    assert!(output.price_evaluations > calls.len() / 4);
}
#[test]
fn explicit_global_overlap_retains_every_quote_and_rejects_inconsistency() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let mut recipes = f.global(InterpolationMethod::Linear);
    let r = &mut recipes[1];
    let mut duplicate = r.helpers[0].clone();
    duplicate.id = "second-source-same-pillar".into();
    r.helpers.push(duplicate);
    if let CurveSource::Helpers(ids) = &mut r.curve.source {
        ids.push("second-source-same-pillar".into());
    }
    if let CalibrationMode::Global(s) = &mut r.curve.calibration {
        s.quote_scales.push(100.);
        s.weights.push(1.);
    }
    assert!(f
        .fit(&recipes)
        .unwrap_err()
        .to_string()
        .contains("overlapping"));
    recipes[1].overlap_policy = RateOverlapPolicy::RetainAllGlobal;
    let output = f.fit(&recipes).unwrap();
    compare_reference(&f, &output).unwrap();
    assert_eq!(output.calibrations[1].curve.diagnostics().quotes.len(), 4);
    recipes[1].helpers[3].quote.value += 0.2;
    assert!(f.fit(&recipes).is_err());
}
#[test]
fn piecewise_duplicate_pillars_and_wrong_resource_or_calendar_versions_fail() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let mut r = f.recipes.clone();
    r[1].helpers[1].pillar = r[1].helpers[0].pillar;
    assert!(f.fit(&r).is_err());
    let mut resources = f.resources(&f.recipes);
    let RateCalibrationTerms::MoneyMarket(t) = &f.recipes[0].helpers[0].terms else {
        panic!()
    };
    let mut wrong = t.calendar.clone();
    wrong.version = Version::new(2).unwrap();
    resources[0][0] = RateHelperResources::MoneyMarket(convex_rates::CalendarBinding {
        pin: &wrong,
        calendar: &f.calendar,
    });
    assert!(build(&f, &f.recipes, &resources, &f.history)
        .unwrap_err()
        .to_string()
        .contains("calendar"));
}
#[test]
fn missing_exact_dependencies_and_late_cycles_return_no_partial_set() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let mut recipes = f.recipes.clone();
    let absent = CurveKey {
        id: "D.OIS".into(),
        version: "absent-revision".into(),
    };
    recipes[1].curve.dependencies = vec![absent.clone()];
    let RateCalibrationTerms::Swap(t) = &mut recipes[1].helpers[2].terms else {
        panic!()
    };
    t.collateral.discount_curve = absent;
    assert!(f
        .fit(&recipes)
        .unwrap_err()
        .to_string()
        .contains("missing dependency"));
    let mut recipes = f.recipes.clone();
    recipes[1].curve.dependencies = vec![recipes[2].curve.key.clone()];
    let key = recipes[2].curve.key.clone();
    let RateCalibrationTerms::Swap(t) = &mut recipes[1].helpers[2].terms else {
        panic!()
    };
    t.collateral.discount_curve = key;
    assert!(f.fit(&recipes).unwrap_err().to_string().contains("cyclic"));
}
#[test]
fn index_and_collateral_roles_never_substitute_single_curve_discounting() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let mut recipes = f.recipes.clone();
    let key = recipes[0].curve.key.clone();
    let RateCalibrationTerms::Swap(t) = &mut recipes[1].helpers[2].terms else {
        panic!()
    };
    let FloatingLegTerms::Term(t) = &mut t.floating else {
        panic!()
    };
    t.projection_curve = key;
    assert!(f
        .fit(&recipes)
        .unwrap_err()
        .to_string()
        .contains("projection"));
    let mut recipes = f.recipes.clone();
    recipes[0].overnight_projection = None;
    assert!(f.fit(&recipes).is_err());
}
#[test]
fn original_delayed_payment_horizons_cannot_hide_behind_extrapolation() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let mut recipes = f.recipes.clone();
    recipes[0].curve.extrapolation = ExtrapolationMethod::Flat;
    recipes[0].pillars[2] = d("2027-10-07");
    recipes[0].helpers[2].pillar = d("2027-10-07");
    assert!(f.fit(&recipes).unwrap_err().to_string().contains("pillar"));
}
#[test]
fn a_known_fra_is_not_a_curve_parameter_and_global_rank_failure_is_explicit() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let mut r = f.recipes[1].clone();
    r.helpers = vec![r.helpers[1].clone()];
    r.pillars = vec![r.helpers[0].pillar];
    r.curve.dependencies.clear();
    r.helpers[0].quote = MarketQuote {
        value: 0.0371,
        unit: QuoteUnit::DecimalRate,
    };
    let RateCalibrationTerms::MoneyMarket(t) = &mut r.helpers[0].terms else {
        panic!()
    };
    let MoneyMarketTerms::Fra { fixing_date, .. } = &mut t.terms else {
        panic!()
    };
    *fixing_date = d("2026-10-05");
    r.curve.source = CurveSource::Helpers(vec![r.helpers[0].id.clone()]);
    assert!(f
        .fit(&[r.clone()])
        .unwrap_err()
        .to_string()
        .contains("identification"));
    r.bootstrap = None;
    r.curve.calibration = CalibrationMode::Global(GlobalFitSettings {
        max_iterations: 10,
        objective_tolerance: 1e-12,
        initial_zeros: vec![0.02],
        lower_zero: -0.08,
        upper_zero: 0.15,
        quote_scales: vec![1.],
        weights: vec![1.],
    });
    let error = f.fit(&[r]).unwrap_err().to_string();
    assert!(
        error.contains("rank") || error.contains("identif"),
        "{error}"
    );
}
#[test]
fn rounded_or_clipped_helpers_and_unadmitted_convexity_fail_instead_of_approximating() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let mut recipes = f.recipes.clone();
    let RateCalibrationTerms::Swap(t) = &mut recipes[1].helpers[2].terms else {
        panic!()
    };
    t.fixed.leg.rounding = CashRounding::NearestEven(2);
    assert!(f.fit(&recipes).is_err());
    let mut recipes = f.recipes.clone();
    let RateCalibrationTerms::RateFuture(t) = &mut recipes[0].helpers[1].terms else {
        panic!()
    };
    let FuturesSettlement::Rate { convexity, .. } = &mut t.contract.settlement else {
        panic!()
    };
    *convexity = FuturesConvexityPolicy::NotAdmitted;
    assert!(f
        .fit(&recipes)
        .unwrap_err()
        .to_string()
        .contains("convexity"));
}
#[test]
fn full_native_recipes_roundtrip_without_losing_profiles_dates_or_quote_units() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let json = serde_json::to_string(&f.recipes).unwrap();
    let loaded: Vec<RateCurveRecipe> = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded, f.recipes);
    compare_reference(&f, &f.fit(&loaded).unwrap()).unwrap();
    let mut invalid: Value = serde_json::from_str(&json).unwrap();
    invalid[2]["helpers"][2]["terms"]["optional_cap"] = serde_json::json!("not-admitted");
    assert!(serde_json::from_value::<Vec<RateCurveRecipe>>(invalid).is_err());
}
#[test]
fn two_leg_basis_cash_par_replacement_and_step_shift_use_shared_full_pricers() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let (terms, _) = basis(&f);
    let calendars = BasisSwapCalendars {
        pay: f.float_calendars(&terms.pay),
        receive: f.float_calendars(&terms.receive),
    };
    let contract = terms.clone().build(&calendars).unwrap();
    let curves = f.reference_curves().unwrap();
    let inputs = SwapValuationInputs {
        curves: &curves,
        fixings: &f.history,
    };
    let value = price_basis_swap(
        &contract,
        &inputs,
        f.context().unwrap().cashflows,
        |i, d| f.publication(i, d),
    )
    .unwrap();
    close(value.present_value.value().get(), 0., 1e-4, 0.);
    let BasisParQuotes::Available {
        pay_spread,
        pay_spread_shift,
        ..
    } = value.par
    else {
        panic!()
    };
    for replacement in [true, false] {
        let mut changed = terms.clone();
        let FloatingLegTerms::Term(pay) = &mut changed.pay else {
            panic!()
        };
        let rate = Decimal::from_f64_retain(if replacement {
            pay_spread.value().get()
        } else {
            pay_spread_shift.value().get()
        })
        .unwrap();
        for p in &mut pay.profiles.spreads {
            p.value = if replacement { rate } else { p.value + rate };
        }
        let changed = changed.build(&calendars).unwrap();
        close(
            price_basis_swap(&changed, &inputs, f.context().unwrap().cashflows, |i, d| {
                f.publication(i, d)
            })
            .unwrap()
            .present_value
            .value()
            .get(),
            0.,
            1e-4,
            0.,
        );
    }
    let mut inverse = terms.clone();
    std::mem::swap(&mut inverse.pay, &mut inverse.receive);
    let calendars = BasisSwapCalendars {
        pay: f.float_calendars(&inverse.pay),
        receive: f.float_calendars(&inverse.receive),
    };
    let inverse = inverse.clone().build(&calendars).unwrap();
    let opposite = price_basis_swap(&inverse, &inputs, f.context().unwrap().cashflows, |i, d| {
        f.publication(i, d)
    })
    .unwrap();
    close(
        value.pay.present_value.value().get() + opposite.receive.present_value.value().get(),
        0.,
        1e-8,
        0.,
    );
    assert_eq!(value.pay.cashflows.len(), 4);
    assert_eq!(value.receive.cashflows.len(), 2);
}
#[test]
fn cubic_global_is_an_explicit_supported_fit_with_all_original_native_residuals() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let output = f.fit(&f.global(InterpolationMethod::CubicSpline)).unwrap();
    for c in &output.calibrations {
        assert_eq!(
            c.curve.recipe().interpolation,
            InterpolationMethod::CubicSpline
        );
        for q in &c.curve.diagnostics().quotes {
            assert!(q.residual.abs() <= q.tolerance);
        }
    }
}
#[test]
fn bounded_iterations_zero_weights_and_unsupported_methods_never_admit_a_curve() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let mut recipes = f.global(InterpolationMethod::Linear);
    for r in &mut recipes {
        if let CalibrationMode::Global(s) = &mut r.curve.calibration {
            s.max_iterations = 1;
        }
    }
    assert!(f.fit(&recipes).is_err());
    let mut recipes = f.global(InterpolationMethod::Linear);
    if let CalibrationMode::Global(s) = &mut recipes[0].curve.calibration {
        s.weights[0] = 0.;
    }
    assert!(f.fit(&recipes).is_err());
    let mut recipes = f.recipes.clone();
    recipes[0].curve.interpolation = InterpolationMethod::CubicSpline;
    assert!(f.fit(&recipes).is_err());
    let mut recipes = f.recipes.clone();
    recipes[0].curve.representation = CurveRepresentation::DiscountFactors;
    assert!(f.fit(&recipes).is_err());
    let mut recipes = f.recipes.clone();
    recipes[0].helpers[0].tolerance = f64::NAN;
    assert!(f.fit(&recipes).is_err());
}
#[test]
fn arithmetic_future_calibration_retains_actual_weights_and_full_convexity() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let mut recipes = f.recipes.clone();
    let h = &mut recipes[0].helpers[1];
    let RateCalibrationTerms::RateFuture(t) = &mut h.terms else {
        panic!()
    };
    let FuturesSettlement::Rate {
        accrual: convex_core::futures::RateAccrual::Overnight { averaging, .. },
        ..
    } = &mut t.contract.settlement
    else {
        panic!()
    };
    *averaging = RateAveraging::Arithmetic;
    h.quote.value = reference(&f.row["expected"]["arithmetic_future_price"]);
    let output = f.fit(&recipes).unwrap();
    compare_reference(&f, &output).unwrap();
    let RateHelperValuation::RateFuture(v) = &output.calibrations[0].repriced_helpers[1] else {
        panic!()
    };
    assert_eq!(v.known_days, 21);
    assert_eq!(v.projected_days, 70);
    assert_eq!(v.applied_forward_adjustment.decimal(), e("0.00015"));
}

#[test]
fn single_term_future_fits_its_separate_fixing_and_value_dates_in_original_points() {
    for id in ["nonflat-positive", "nonflat-negative"] {
        let f = Fixture::load(id).unwrap();
        for global in [false, true] {
            let mut recipes = if global {
                f.global(InterpolationMethod::Linear)
            } else {
                f.recipes.clone()
            };
            let h = &mut recipes[1].helpers[1];
            h.id = "3M-term-future".into();
            h.terms = serde_json::from_value(f.row["term_future_terms"].clone()).unwrap();
            h.quote = MarketQuote {
                value: reference(&f.row["expected"]["term_future_price"]),
                unit: QuoteUnit::FuturePrice100MinusRate,
            };
            h.tolerance = 1e-10;
            recipes[1].curve.source =
                CurveSource::Helpers(recipes[1].helpers.iter().map(|h| h.id.clone()).collect());
            if let CalibrationMode::Global(s) = &mut recipes[1].curve.calibration {
                s.quote_scales[1] = 100.;
            }
            let output = f.fit(&recipes).unwrap();
            compare_reference(&f, &output).unwrap();
            let RateHelperValuation::RateFuture(v) = &output.calibrations[1].repriced_helpers[1]
            else {
                panic!()
            };
            assert_eq!(v.observations.len(), 1);
            let r = &v.observations[0];
            assert_eq!(r.requirement.observation_date, d("2026-10-28"));
            assert_eq!(r.rate_start, d("2026-10-30"));
            assert_eq!(r.rate_end, d("2026-12-16"));
            assert!(matches!(
                r.origin,
                FuturesRateOrigin::FutureProjection { .. }
            ));
            assert_eq!(v.known_days, 0);
            assert_eq!(v.projected_days, 47);
        }
    }
}

// Ingestion owns new immutable revisions and all dependent exact mappings.
fn repin(recipes: &mut [RateCurveRecipe], version: &str) {
    let key = |k: &mut CurveKey| k.version = version.into();
    for r in recipes {
        key(&mut r.curve.key);
        for d in &mut r.curve.dependencies {
            key(d);
        }
        if let Some(p) = &mut r.overnight_projection {
            key(&mut p.key);
        }
        for h in &mut r.helpers {
            match &mut h.terms {
                RateCalibrationTerms::MoneyMarket(t) => key(&mut t.curve),
                RateCalibrationTerms::Swap(t) => {
                    key(&mut t.collateral.discount_curve);
                    match &mut t.floating {
                        FloatingLegTerms::Term(t) => key(&mut t.projection_curve),
                        FloatingLegTerms::Overnight(t) => key(&mut t.projection_curve),
                    }
                }
                RateCalibrationTerms::RateFuture(t) => {
                    key(&mut t.projection_curve);
                    t.projection_stamp.version = Version::new(version.parse().unwrap()).unwrap();
                }
                RateCalibrationTerms::BasisSwap { terms: t, .. } => {
                    key(&mut t.collateral.discount_curve);
                    for leg in [&mut t.pay, &mut t.receive] {
                        match leg {
                            FloatingLegTerms::Term(t) => key(&mut t.projection_curve),
                            FloatingLegTerms::Overnight(t) => key(&mut t.projection_curve),
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn complete_quote_rebuild_propagates_dependencies_without_changing_known_revisions_or_last_good() {
    let f = Fixture::load("nonflat-positive").unwrap();
    for global in [false, true] {
        let original = if global {
            f.global(InterpolationMethod::Linear)
        } else {
            f.recipes.clone()
        };
        let base = f.fit(&original).unwrap();
        let grids: Vec<_> = base
            .calibrations
            .iter()
            .map(|c| c.curve.interpolation_grid().unwrap().1.to_vec())
            .collect();
        // +1bp deposit, -0.01 future points, +1bp IRS and +1 native basis point.
        for (curve, helper, amount) in [(0, 0, 0.0001), (0, 1, -0.01), (1, 2, 0.0001), (2, 2, 1.)] {
            let mut bumped = original.clone();
            let h = &bumped[curve].helpers[helper];
            let quote = MarketQuote {
                value: h.quote.value + amount,
                unit: h.quote.unit,
            };
            bumped[curve].helpers[helper] = h.with_quote(quote).unwrap();
            repin(&mut bumped, "2");
            let built = f.fit(&bumped).unwrap();
            assert_eq!(base.calibrations[curve].recipe, original[curve]);
            assert_eq!(
                built.calibrations[curve].recipe.helpers[helper].quote,
                quote
            );
            assert!(built.build_order.iter().all(|k| k.version == "2"));
            for (i, c) in built.calibrations.iter().enumerate() {
                let zeros = c.curve.interpolation_grid().unwrap().1;
                assert_eq!(
                    base.calibrations[i].curve.interpolation_grid().unwrap().1,
                    grids[i]
                );
                if i < curve {
                    assert_eq!(zeros, grids[i]);
                } else if i == curve || curve == 1 || (curve == 0 && helper == 1) {
                    assert!(
                        zeros
                            .iter()
                            .zip(&grids[i])
                            .any(|(a, b)| (a - b).abs() > 1e-10),
                        "bumped {curve}/{helper}, dependent {i}"
                    );
                }
                assert!(c
                    .curve
                    .diagnostics()
                    .dependencies
                    .iter()
                    .all(|k| k.version == "2"));
                assert!(c
                    .curve
                    .diagnostics()
                    .quotes
                    .iter()
                    .all(|q| q.residual.abs() <= q.tolerance));
            }
            let RateHelperValuation::RateFuture(old) = &base.calibrations[0].repriced_helpers[1]
            else {
                panic!()
            };
            let RateHelperValuation::RateFuture(new) = &built.calibrations[0].repriced_helpers[1]
            else {
                panic!()
            };
            for (a, b) in old.observations.iter().zip(&new.observations) {
                if matches!(a.origin, FuturesRateOrigin::Published { .. }) {
                    assert_eq!(a.origin, b.origin);
                    assert_eq!(a.decimal_rate, b.decimal_rate);
                }
            }
            // Reusing a supplied last-good set cannot replace its identity, even
            // with another version. Failure leaves every original curve intact.
            let resources = f.resources(&bumped);
            let inputs: Vec<_> = bumped
                .iter()
                .zip(&resources)
                .map(|(recipe, resources)| RateCurveBuildInput { recipe, resources })
                .collect();
            assert!(build_rate_curve_set(
                &inputs,
                &base.curves,
                &f.history,
                f.context().unwrap(),
                FitNumericalControls::default(),
                |i, d| f.publication(i, d)
            )
            .is_err());
        }
    }
}

#[test]
fn original_quote_profile_mismatch_and_non_rate_replacement_fail_without_rounding_tolerance() {
    let f = Fixture::load("nonflat-positive").unwrap();
    for (curve, helper) in [(1, 2), (2, 2)] {
        let mut recipes = f.recipes.clone();
        recipes[curve].helpers[helper].quote.value += 0.00001;
        assert!(f.fit(&recipes).is_err());
        assert!(recipes[curve].helpers[helper]
            .with_quote(MarketQuote {
                value: 99.,
                unit: QuoteUnit::PricePer100,
            })
            .is_err());
    }
}

#[test]
fn dimension_and_economic_timestamp_bounds_fail_before_publication_callbacks() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let resources = f.resources(&f.recipes);
    let input = RateCurveBuildInput {
        recipe: &f.recipes[0],
        resources: &resources[0],
    };
    let inputs: Vec<_> = (0..=MAX_RATE_FIT_HELPERS)
        .map(|_| RateCurveBuildInput {
            recipe: input.recipe,
            resources: input.resources,
        })
        .collect();
    let mut calls = 0;
    let result = build_rate_curve_set(
        &inputs,
        &CurveSet::new(),
        &f.history,
        f.context().unwrap(),
        FitNumericalControls::default(),
        |i, d| {
            calls += 1;
            f.publication(i, d)
        },
    );
    assert!(result.is_err());
    assert_eq!(calls, 0);
    let mut context = f.context().unwrap();
    context.cashflows.fixings.valuation_date = d("2026-10-08");
    assert!(build_rate_curve_set(
        &[input],
        &CurveSet::new(),
        &f.history,
        context,
        FitNumericalControls::default(),
        |i, d| {
            calls += 1;
            f.publication(i, d)
        }
    )
    .is_err());
    assert_eq!(calls, 0);
}

#[test]
fn direct_basis_preflights_both_histories_and_expired_cash_has_no_par_inverse() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let (terms, _) = basis(&f);
    let calendars = BasisSwapCalendars {
        pay: f.float_calendars(&terms.pay),
        receive: f.float_calendars(&terms.receive),
    };
    let contract = terms.clone().build(&calendars).unwrap();
    let empty = FixingSeries::new(vec![]).unwrap();
    let no_curves = CurveSet::new();
    let missing = price_basis_swap(
        &contract,
        &SwapValuationInputs {
            curves: &no_curves,
            fixings: &empty,
        },
        f.context().unwrap().cashflows,
        |i, d| f.publication(i, d),
    )
    .unwrap_err();
    let convex_rates::SwapError::Cashflow(CashflowError::MissingFixings(missing)) = missing else {
        panic!()
    };
    assert_eq!(
        missing
            .iter()
            .map(|r| r.index_id.as_str())
            .collect::<Vec<_>>(),
        ["USD.TERM3M", "USD.TERM6M"]
    );
    let mut context = f.context().unwrap().cashflows;
    context.fixings.valuation_date = d("2027-10-12");
    context.fixings.known_at =
        convex_core::contracts::UtcTimestamp::parse("2027-10-12T12:00:00Z").unwrap();
    let value = price_basis_swap(
        &contract,
        &SwapValuationInputs {
            curves: &no_curves,
            fixings: &empty,
        },
        context,
        |i, d| f.publication(i, d),
    )
    .unwrap();
    assert_eq!(value.present_value.value().get(), 0.);
    assert!(value.pay.cashflows.is_empty() && value.receive.cashflows.is_empty());
    assert!(matches!(value.par, BasisParQuotes::Unavailable { .. }));
}
