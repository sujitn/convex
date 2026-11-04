//! CVX-060 supplied analytic contracts; no live data or model certificate.
use convex_core::{
    daycounts::DayCountConvention as Dc,
    types::{Compounding as C, Currency, Date},
};
use convex_curves::{contracts::*, ExtrapolationMethod as Ext, InterpolationMethod as Int};
use convex_math::optimization::FitFailureReason;
use proptest::{
    prelude::*,
    test_runner::{Config, RngSeed},
};

fn date(s: &str) -> Date {
    Date::parse(s).unwrap()
}
fn key(id: &str) -> CurveKey {
    CurveKey {
        id: id.into(),
        version: "analytic-v1".into(),
    }
}
fn node(t: f64, value: f64, unit: QuoteUnit) -> CurveNode {
    CurveNode {
        pillar: CurvePoint::Tenor(t),
        quote: MarketQuote { value, unit },
    }
}
fn base() -> CurveRecipe {
    CurveRecipe {
        key: key("USD.negative"),
        currency: Currency::USD,
        role: CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        reference_date: date("2024-01-01"),
        time_basis: Dc::Act365Fixed,
        representation: CurveRepresentation::DiscountFactors,
        interpolation: Int::LogLinear,
        extrapolation: Ext::None,
        calibration: CalibrationMode::Direct,
        dependencies: vec![],
        source: CurveSource::Nodes(vec![
            node(0.0, 1.0, QuoteUnit::DiscountFactor),
            node(1.0, 0.02_f64.exp(), QuoteUnit::DiscountFactor),
            node(3.0, 0.06_f64.exp(), QuoteUnit::DiscountFactor),
        ]),
    }
}
fn set(recipe: &CurveRecipe) -> CurveSet {
    build_curve_set(std::slice::from_ref(recipe), &CurveSet::new())
        .unwrap()
        .curves
}
fn df(curve: &BuiltCurve, point: CurvePoint) -> f64 {
    curve
        .query(&CurveQuery::DiscountFactor { point })
        .unwrap()
        .value
}
fn near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-11 + 1e-11 * expected.abs(),
        "{actual} != {expected}"
    );
}
fn view(base: &CurveRecipe, id: &str) -> CurveRecipe {
    let mut r = base.clone();
    r.key = key(id);
    r.dependencies = vec![base.key.clone()];
    r.source = CurveSource::DiscountEquivalent {
        base: base.key.clone(),
    };
    r
}
fn rolled(base: &CurveRecipe, id: &str, reference: &str, policy: RollPolicy) -> CurveRecipe {
    let mut r = view(base, id);
    r.reference_date = date(reference);
    r.representation = CurveRepresentation::DiscountFactors;
    r.source = CurveSource::Rolled {
        base: base.key.clone(),
        policy,
    };
    r
}
fn helper(id: &str, end: &str, quote: f64, unit: QuoteUnit, tolerance: f64) -> MoneyMarketHelper {
    MoneyMarketHelper {
        id: id.into(),
        terms: MoneyMarketTerms::Deposit {
            start_date: date("2024-01-01"),
            maturity_date: date(end),
            day_count: Dc::Act360,
        },
        quote: MarketQuote { value: quote, unit },
        tolerance,
    }
}
fn global() -> (CurveRecipe, Vec<Date>) {
    let mut r = base();
    r.interpolation = Int::Linear;
    r.representation = CurveRepresentation::ZeroRates {
        compounding: C::Continuous,
    };
    let dates = vec![date("2024-07-01"), date("2025-01-01")];
    let helpers: Vec<_> = dates
        .iter()
        .enumerate()
        .map(|(i, end)| {
            let days = r.reference_date.days_between(end) as f64;
            let rate = (-0.02 * days / 365.0).exp_m1() / (days / 360.0);
            helper(
                &format!("deposit-{i}"),
                &end.to_string(),
                rate * [1.0, 10_000.0][i],
                [QuoteUnit::DecimalRate, QuoteUnit::BasisPoints][i],
                [1e-10, 1e-6][i],
            )
        })
        .collect();
    r.source = CurveSource::MoneyMarket(helpers);
    r.calibration = CalibrationMode::Global(GlobalFitSettings {
        max_iterations: 100,
        objective_tolerance: 1e-11,
        initial_zeros: vec![0.03, 0.01],
        lower_zero: -0.05,
        upper_zero: 0.1,
        quote_scales: vec![1.0, 10_000.0],
        weights: vec![1.0, 2.0],
    });
    (r, dates)
}

#[test]
fn negative_discount_curve_matches_pinned_high_precision_fixture() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/curve-numerics.json")).unwrap();
    assert_eq!(fixture["fixture_id"], "CVX-060-analytic-numerics-v1");
    let r = base();
    let curves = set(&r);
    let curve = curves.get(&r.key).unwrap();
    for case in fixture["discount_queries"].as_array().unwrap() {
        let d = date(case["date"].as_str().unwrap());
        let expected = case["discount"].as_f64().unwrap();
        assert!(expected > 1.0);
        near(df(curve, CurvePoint::Date(d)), expected);
    }
}

#[test]
fn all_compounding_modes_preserve_discounts_including_negative_rates() {
    for c in [
        C::Continuous,
        C::Simple,
        C::Annual,
        C::SemiAnnual,
        C::Quarterly,
        C::Monthly,
        C::Daily,
    ] {
        for tau in [1.0 / 365.0, 0.25, 2.0, 10.0] {
            for rate in [-0.02, 0.0, 0.04] {
                let discount = discount_from_rate(rate, tau, c).unwrap();
                near(rate_from_discount(discount, tau, c).unwrap(), rate);
                near(
                    discount_from_rate(
                        convert_rate(rate, tau, c, C::Continuous).unwrap(),
                        tau,
                        C::Continuous,
                    )
                    .unwrap(),
                    discount,
                );
                if rate < 0.0 {
                    assert!(discount > 1.0);
                }
            }
        }
    }
}

#[test]
fn very_short_daily_rate_does_not_cancel_to_zero() {
    let discount = 1.00000000000001_f64;
    let tau = 1e-6;
    let continuous = -discount.ln() / tau;
    let daily = rate_from_discount(discount, tau, C::Daily).unwrap();
    assert!(daily < 0.0);
    assert!((daily - continuous).abs() <= 1e-18);
    near(discount_from_rate(daily, tau, C::Daily).unwrap(), discount);
}

#[test]
fn very_short_rate_conversion_preserves_information_before_df_rounding() {
    let tau = 1e-18;
    for rate in [-0.01, 0.01] {
        let continuous = convert_rate(rate, tau, C::Simple, C::Continuous).unwrap();
        assert!((continuous - rate).abs() < 1e-15);
        let daily = convert_rate(continuous, tau, C::Continuous, C::Daily).unwrap();
        let expected_daily = 365.0 * (continuous / 365.0).exp_m1();
        assert!((daily - expected_daily).abs() < 1e-15);
        assert!(
            (convert_rate(daily, tau, C::Daily, C::Continuous).unwrap() - continuous).abs() < 1e-15
        );
    }
}

#[test]
fn existing_piecewise_fit_retains_negative_rate_compatibility_and_failed_output_rules() {
    use convex_curves::calibration::{Deposit, InstrumentSet, PiecewiseBootstrapper};
    let reference = date("2024-01-01");
    let a = Deposit::new(reference, date("2024-07-01"), -0.01, Dc::Act365Fixed);
    let b = Deposit::new(reference, date("2025-01-01"), -0.02, Dc::Act365Fixed);
    let ordered = InstrumentSet::new().with(a.clone()).with(b.clone());
    let reversed = InstrumentSet::new().with(b).with(a);
    let first = PiecewiseBootstrapper::new()
        .bootstrap(reference, &ordered)
        .unwrap();
    let second = PiecewiseBootstrapper::new()
        .bootstrap(reference, &reversed)
        .unwrap();
    assert!(first.converged);
    assert!(first.rms_error < 1e-12);
    assert_eq!(first.residuals, second.residuals);
    assert!(first.curve.values().iter().all(|&v| v < 0.0));
    assert!(PiecewiseBootstrapper::new()
        .tolerance(0.0)
        .bootstrap(reference, &ordered)
        .is_err());
}

#[test]
fn existing_global_budget_failure_does_not_return_a_converged_curve() {
    use convex_curves::calibration::{Deposit, FitterConfig, GlobalFitter, InstrumentSet};
    let reference = date("2024-01-01");
    let inputs = InstrumentSet::new()
        .with(Deposit::new(
            reference,
            date("2024-07-01"),
            -0.01,
            Dc::Act365Fixed,
        ))
        .with(Deposit::new(
            reference,
            date("2025-01-01"),
            -0.02,
            Dc::Act365Fixed,
        ));
    let failed = GlobalFitter::with_config(
        FitterConfig::new()
            .with_max_iterations(1)
            .with_tolerance(1e-30),
    )
    .fit(reference, &inputs)
    .unwrap_err();
    assert!(failed.to_string().contains("requested tolerance"));
}

#[test]
fn conversion_zero_invalid_bases_and_extreme_arithmetic_are_explicit() {
    assert_eq!(discount_from_rate(-0.1, 0.0, C::Continuous).unwrap(), 1.0);
    assert!(rate_from_discount(1.0, 0.0, C::Continuous).is_err());
    assert!(convert_rate(0.01, 0.0, C::Simple, C::Continuous).is_err());
    assert!(discount_from_rate(-1.0, 1.0, C::Annual).is_err());
    assert!(discount_from_rate(-0.5, 2.0, C::Simple).is_err());
    assert!(discount_from_rate(10.0, 150.0, C::Continuous).is_err());
    assert!(discount_from_rate(-10.0, 150.0, C::Continuous).is_err());
    for bad in [f64::NAN, f64::INFINITY, -1.0] {
        assert!(rate_from_discount(bad, 1.0, C::Daily).is_err());
        assert!(discount_from_rate(0.01, bad, C::Continuous).is_err());
    }
}

#[test]
fn exact_representation_views_preserve_curved_interpolation_at_off_grid_points() {
    let mut r = base();
    r.source = CurveSource::Nodes(vec![
        node(0.0, -0.02, QuoteUnit::DecimalRate),
        node(1.0, -0.01, QuoteUnit::DecimalRate),
        node(3.0, 0.05, QuoteUnit::DecimalRate),
    ]);
    r.representation = CurveRepresentation::ZeroRates {
        compounding: C::Continuous,
    };
    r.interpolation = Int::CubicSpline;
    let original = set(&r);
    let mut equivalent = view(&r, "equivalent");
    equivalent.representation = CurveRepresentation::DiscountFactors;
    let output = build_curve_set(std::slice::from_ref(&equivalent), &original).unwrap();
    let source = output.curves.get(&r.key).unwrap();
    let curve = output.curves.get(&equivalent.key).unwrap();
    for t in [0.0, 0.001, 0.25, 0.9, 1.9, 2.5, 3.0] {
        near(
            df(source, CurvePoint::Tenor(t)),
            df(curve, CurvePoint::Tenor(t)),
        );
    }
    assert_eq!(curve.diagnostics().method, "exact_discount_equivalent_view");
    assert_eq!(curve.diagnostics().dependencies, [r.key]);
    assert!(curve.diagnostics().quotes.is_empty());
}

#[test]
fn discount_views_to_all_zero_compoundings_retain_native_curve_semantics() {
    let r = base();
    let original = set(&r);
    for c in [
        C::Continuous,
        C::Simple,
        C::Annual,
        C::SemiAnnual,
        C::Quarterly,
        C::Monthly,
        C::Daily,
    ] {
        let mut equivalent = view(&r, "zero");
        equivalent.representation = CurveRepresentation::ZeroRates { compounding: c };
        let output = build_curve_set(std::slice::from_ref(&equivalent), &original).unwrap();
        let curve = output.curves.get(&equivalent.key).unwrap();
        for t in [0.1, 0.5, 2.0] {
            near(df(curve, CurvePoint::Tenor(t)), (0.02 * t).exp());
        }
        assert!(curve
            .query(&CurveQuery::Raw {
                point: CurvePoint::Tenor(0.0)
            })
            .is_err());
        assert_eq!(df(curve, CurvePoint::Tenor(0.0)), 1.0);
    }
}

#[test]
fn representation_views_reject_hidden_convention_role_and_version_changes() {
    let r = base();
    let original = set(&r);
    let mut bad = view(&r, "bad");
    bad.time_basis = Dc::Act360;
    assert!(build_curve_set(&[bad], &original).is_err());
    let mut bad = view(&r, "bad");
    bad.currency = Currency::EUR;
    assert!(build_curve_set(&[bad], &original).is_err());
    let mut bad = view(&r, "bad");
    bad.role = CurveRole::Projection {
        index_id: "index".into(),
    };
    assert!(build_curve_set(&[bad], &original).is_err());
    let mut bad = view(&r, "bad");
    bad.reference_date = date("2024-01-02");
    assert!(build_curve_set(&[bad], &original).is_err());
    let mut bad = view(&r, "bad");
    bad.interpolation = Int::Linear;
    assert!(build_curve_set(&[bad], &original).is_err());
    let mut bad = view(&r, "bad");
    bad.extrapolation = Ext::Flat;
    assert!(build_curve_set(&[bad], &original).is_err());
    let mut bad = view(&r, "bad");
    bad.dependencies[0].version = "missing".into();
    bad.source = CurveSource::DiscountEquivalent {
        base: bad.dependencies[0].clone(),
    };
    assert!(matches!(
        build_curve_set(&[bad], &original),
        Err(CurveContractError::MissingDependency(_))
    ));
}

#[test]
fn sticky_forward_roll_agrees_with_actual_date_discounts_across_leap_boundaries() {
    for dc in [Dc::Act360, Dc::Act365Fixed, Dc::ActActIsda] {
        let mut r = base();
        r.reference_date = date("2023-12-31");
        r.time_basis = dc;
        let original = set(&r);
        let rolled = rolled(&r, "roll", "2024-03-01", RollPolicy::StickyForward);
        let output = build_curve_set(std::slice::from_ref(&rolled), &original).unwrap();
        let before = output.curves.get(&r.key).unwrap();
        let after = output.curves.get(&rolled.key).unwrap();
        let anchor = df(before, CurvePoint::Date(rolled.reference_date));
        assert_eq!(df(after, CurvePoint::Tenor(0.0)), 1.0);
        for d in ["2024-03-02", "2024-12-31", "2025-01-01"] {
            let point = CurvePoint::Date(date(d));
            near(df(after, point), df(before, point) / anchor);
            let tenor = after.date_to_tenor(date(d)).unwrap();
            assert_eq!(
                after.tenor_to_date(tenor, TenorRounding::Nearest).unwrap(),
                date(d)
            );
        }
        assert_eq!(after.recipe().time_basis, dc);
        assert!(after.domain().maximum_tenor < before.domain().maximum_tenor);
    }
}

#[test]
fn sticky_zero_and_sticky_forward_have_distinct_declared_nonflat_results() {
    let mut r = base();
    r.representation = CurveRepresentation::ZeroRates {
        compounding: C::Continuous,
    };
    r.interpolation = Int::Linear;
    r.source = CurveSource::Nodes(vec![
        node(0.0, -0.02, QuoteUnit::DecimalRate),
        node(1.0, 0.02, QuoteUnit::DecimalRate),
        node(3.0, 0.08, QuoteUnit::DecimalRate),
    ]);
    let original = set(&r);
    let zero = rolled(&r, "zero", "2024-07-01", RollPolicy::StickyZero);
    let forward = rolled(&r, "forward", "2024-07-01", RollPolicy::StickyForward);
    let output = build_curve_set(&[zero.clone(), forward.clone()], &original).unwrap();
    let source = output.curves.get(&r.key).unwrap();
    let z = output.curves.get(&zero.key).unwrap();
    let f = output.curves.get(&forward.key).unwrap();
    let t = 1.1;
    let offset = source.date_to_tenor(zero.reference_date).unwrap();
    near(
        df(z, CurvePoint::Tenor(t)),
        df(source, CurvePoint::Tenor(t)),
    );
    near(
        df(f, CurvePoint::Tenor(t)),
        df(source, CurvePoint::Tenor(offset + t)) / df(source, CurvePoint::Tenor(offset)),
    );
    assert!((df(z, CurvePoint::Tenor(t)) - df(f, CurvePoint::Tenor(t))).abs() > 0.001);
}

#[test]
fn rolling_rejects_backward_dates_exhausted_horizons_and_unselected_tail_rolls() {
    let mut r = base();
    r.extrapolation = Ext::Flat;
    let original = set(&r);
    for d in ["2023-12-31", "2027-01-01", "2030-01-01"] {
        let recipe = rolled(&r, "bad", d, RollPolicy::StickyForward);
        assert!(build_curve_set(&[recipe], &original).is_err());
    }
    let mut invalid = rolled(&r, "bad", "2024-07-01", RollPolicy::StickyForward);
    invalid.representation = CurveRepresentation::ZeroRates {
        compounding: C::Continuous,
    };
    assert!(build_curve_set(&[invalid], &original).is_err());
}

#[test]
fn rolled_tail_retains_base_policy_and_query_ceiling() {
    let mut r = base();
    r.extrapolation = Ext::Flat;
    let original = set(&r);
    let recipe = rolled(&r, "roll", "2024-07-01", RollPolicy::StickyForward);
    let output = build_curve_set(std::slice::from_ref(&recipe), &original).unwrap();
    let curve = output.curves.get(&recipe.key).unwrap();
    let q = curve
        .query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Tenor(4.0),
        })
        .unwrap();
    assert!(q.extrapolated);
    near(
        q.value,
        df(output.curves.get(&r.key).unwrap(), CurvePoint::Tenor(3.0))
            / df(
                output.curves.get(&r.key).unwrap(),
                CurvePoint::Date(recipe.reference_date),
            ),
    );
    assert!(curve
        .query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Tenor(150.1)
        })
        .is_err());
}

#[test]
fn nested_views_and_spreads_stop_at_a_bound_across_multiple_build_calls() {
    let mut recipe = base();
    let mut curves = set(&recipe);
    for i in 1..=MAX_CURVE_DERIVATIONS {
        recipe = view(&recipe, &format!("view-{i}"));
        curves = build_curve_set(std::slice::from_ref(&recipe), &curves)
            .unwrap()
            .curves;
    }
    let invalid = view(&recipe, "too-deep");
    assert!(matches!(
        build_curve_set(&[invalid], &curves),
        Err(CurveContractError::BoundExceeded("derived curve nesting"))
    ));
    assert_eq!(curves.len(), MAX_CURVE_DERIVATIONS + 1);
}

#[test]
fn global_dated_helpers_reprice_native_quotes_and_preserve_negative_parameters() {
    let (r, dates) = global();
    let output = fit_money_market_curve(&r, &dates, FitNumericalControls::default()).unwrap();
    assert_eq!(output.curve.recipe(), &r);
    assert_eq!(output.solver.rank, Some(2));
    assert!(output.solver.condition_estimate.unwrap() < 10.0);
    assert!(output.solver.scaled_rms.unwrap() <= 1e-11);
    assert!(output
        .curve
        .diagnostics()
        .quotes
        .iter()
        .all(|q| q.residual.abs() <= q.tolerance));
    for &d in &dates {
        near(
            df(&output.curve, CurvePoint::Date(d)),
            (0.02 * year_fraction(r.reference_date, d, Dc::Act365Fixed).unwrap()).exp(),
        );
    }
    let set = CurveSet::new().with_curve(output.curve).unwrap();
    assert_eq!(set.len(), 1);
    let derived = view(&r, "view");
    // This view has Direct mode; the base retains its original Global recipe.
    let mut derived = derived;
    derived.calibration = CalibrationMode::Direct;
    assert_eq!(build_curve_set(&[derived], &set).unwrap().curves.len(), 2);
}

#[test]
fn global_parameter_and_helper_pairs_sort_without_quote_scale_drift() {
    let (r, dates) = global();
    let first = fit_money_market_curve(&r, &dates, FitNumericalControls::default()).unwrap();
    let mut reordered = r;
    if let CurveSource::MoneyMarket(helpers) = &mut reordered.source {
        helpers.reverse();
    }
    if let CalibrationMode::Global(settings) = &mut reordered.calibration {
        settings.initial_zeros.reverse();
        settings.quote_scales.reverse();
        settings.weights.reverse();
    }
    let mut dates = dates;
    dates.reverse();
    let second =
        fit_money_market_curve(&reordered, &dates, FitNumericalControls::default()).unwrap();
    assert_eq!(first.solver, second.solver);
    assert_eq!(first.curve.diagnostics(), second.curve.diagnostics());
}

#[test]
fn global_fra_uses_its_actual_forward_period_and_native_unit() {
    let (mut r, dates) = global();
    let start = dates[0];
    let end = dates[1];
    let days = start.days_between(&end) as f64;
    let rate = (-0.02 * days / 365.0).exp_m1() / (days / 360.0);
    if let CurveSource::MoneyMarket(helpers) = &mut r.source {
        helpers[1].terms = MoneyMarketTerms::Fra {
            index_id: "USD.TERM.6M".into(),
            fixing_date: date("2024-06-27"),
            start_date: start,
            end_date: end,
            settlement_date: start,
            settlement: FraSettlement::AtStart,
            day_count: Dc::Act360,
        };
        helpers[1].quote.value = rate * 10_000.0;
    }
    let output = fit_money_market_curve(&r, &dates, FitNumericalControls::default()).unwrap();
    assert_eq!(output.solver.rank, Some(2));
    let fra = &output.curve.diagnostics().quotes[1];
    assert_eq!(fra.input.unit, QuoteUnit::BasisPoints);
    assert!(fra.residual.abs() <= fra.tolerance);
    let model = output
        .curve
        .query(&CurveQuery::ForwardRate {
            start: CurvePoint::Date(start),
            end: CurvePoint::Date(end),
            compounding: C::Simple,
            day_count: Dc::Act360,
        })
        .unwrap();
    near(model.value, rate);
}

#[test]
fn overlapping_global_observations_are_retained_and_inconsistency_fails() {
    let (mut r, dates) = global();
    if let CurveSource::MoneyMarket(helpers) = &mut r.source {
        let mut extra = helpers[0].clone();
        extra.id = "overlapping-observation".into();
        helpers.push(extra);
    }
    if let CalibrationMode::Global(settings) = &mut r.calibration {
        settings.quote_scales.push(1.0);
        settings.weights.push(1.0);
    }
    let output = fit_money_market_curve(&r, &dates, FitNumericalControls::default()).unwrap();
    assert_eq!(output.curve.diagnostics().quotes.len(), 3);
    assert_eq!(output.solver.rank, Some(2));
    if let CurveSource::MoneyMarket(helpers) = &mut r.source {
        helpers[2].quote.value += 0.01;
    }
    let error = fit_money_market_curve(&r, &dates, FitNumericalControls::default()).unwrap_err();
    let CurveFitError::Solver(failed) = error else {
        panic!("expected inconsistent quote solver failure")
    };
    assert_eq!(failed.diagnostics.native_residuals.len(), 3);
    assert!(matches!(
        failed.reason,
        FitFailureReason::Stagnation | FitFailureReason::IterationLimit
    ));
}

#[test]
fn global_bounds_iterations_callback_and_dimensions_are_declared_failures() {
    let (mut r, dates) = global();
    if let CalibrationMode::Global(settings) = &mut r.calibration {
        settings.max_iterations = 1;
    }
    let failure = fit_money_market_curve(&r, &dates, FitNumericalControls::default()).unwrap_err();
    match failure {
        CurveFitError::Solver(failed) => {
            assert!(matches!(failed.reason, FitFailureReason::IterationLimit));
            assert_eq!(failed.diagnostics.iterations, 1);
        }
        _ => panic!("expected actual solver limit"),
    }
    if let CalibrationMode::Global(settings) = &mut r.calibration {
        settings.max_iterations = 100;
        settings.lower_zero = 0.0;
    }
    assert!(matches!(
        fit_money_market_curve(&r, &dates, FitNumericalControls::default()),
        Err(CurveFitError::Solver(_))
    ));
    assert!(
        fit_money_market_curve(&r, &[dates[0], dates[0]], FitNumericalControls::default()).is_err()
    );
    assert!(fit_money_market_curve(&r, &[], FitNumericalControls::default()).is_err());
    assert!(fit_money_market_curve(
        &r,
        &dates,
        FitNumericalControls {
            svd_iterations: 0,
            ..Default::default()
        }
    )
    .is_err());
}

#[test]
fn extra_unobserved_parameters_are_diagnosed_as_ill_conditioned() {
    let (mut r, mut dates) = global();
    dates[1] = date("2026-01-01");
    if let CurveSource::MoneyMarket(helpers) = &mut r.source {
        helpers[1] = helpers[0].clone();
        helpers[1].id = "different-input".into();
    }
    let failure = fit_money_market_curve(&r, &dates, FitNumericalControls::default()).unwrap_err();
    match failure {
        CurveFitError::Solver(failed) => {
            assert!(matches!(failed.reason, FitFailureReason::IllConditioned));
            assert_eq!(failed.diagnostics.rank, Some(1));
        }
        _ => panic!("expected rank failure"),
    }
}

#[test]
fn failed_fit_or_later_build_keeps_previous_set_immutable() {
    let (mut r, dates) = global();
    let original = set(&base());
    if let CurveSource::MoneyMarket(helpers) = &mut r.source {
        helpers[1].quote.value = f64::NAN;
    }
    assert!(fit_money_market_curve(&r, &dates, FitNumericalControls::default()).is_err());
    assert_eq!(original.len(), 1);
    let valid = view(&base(), "valid");
    let mut invalid = view(&base(), "invalid");
    invalid.currency = Currency::EUR;
    assert!(build_curve_set(&[valid, invalid], &original).is_err());
    assert_eq!(original.len(), 1);
}

#[test]
fn checked_method_domains_do_not_silently_clip_or_substitute() {
    let mut r = base();
    r.interpolation = Int::Linear;
    r.extrapolation = Ext::Linear;
    r.source = CurveSource::Nodes(vec![
        node(0.0, 1.0, QuoteUnit::DiscountFactor),
        node(1.0, 0.1, QuoteUnit::DiscountFactor),
    ]);
    let curves = set(&r);
    let curve = curves.get(&r.key).unwrap();
    assert!(curve
        .query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Tenor(2.0)
        })
        .is_err());
    for method in [
        Int::FlatForward,
        Int::MonotoneConvex,
        Int::NelsonSiegel,
        Int::Svensson,
    ] {
        r.interpolation = method;
        assert!(matches!(
            build_curve_set(std::slice::from_ref(&r), &CurveSet::new()),
            Err(CurveContractError::Unsupported { .. })
        ));
    }
}

#[test]
fn unsorted_nodes_normalize_and_duplicate_near_extreme_nodes_fail_atomically() {
    let mut r = base();
    let first = set(&r);
    if let CurveSource::Nodes(nodes) = &mut r.source {
        nodes.reverse();
    }
    let second = set(&r);
    assert_eq!(
        first.get(&r.key).unwrap().diagnostics(),
        second.get(&r.key).unwrap().diagnostics()
    );
    for nodes in [
        vec![
            node(0.0, 1.0, QuoteUnit::DiscountFactor),
            node(0.0, 1.0, QuoteUnit::DiscountFactor),
        ],
        vec![
            node(0.0, 1.0, QuoteUnit::DiscountFactor),
            node(1e-12, 1.0, QuoteUnit::DiscountFactor),
        ],
        vec![
            node(0.0, 1.0, QuoteUnit::DiscountFactor),
            node(151.0, 1.0, QuoteUnit::DiscountFactor),
        ],
        vec![
            node(0.0, 1.0, QuoteUnit::DiscountFactor),
            node(1.0, f64::INFINITY, QuoteUnit::DiscountFactor),
        ],
    ] {
        r.source = CurveSource::Nodes(nodes);
        assert!(build_curve_set(std::slice::from_ref(&r), &CurveSet::new()).is_err());
    }
}

#[test]
fn dated_forward_conversion_preserves_actual_daycount_and_reports_conventions() {
    let r = base();
    let curves = set(&r);
    let curve = curves.get(&r.key).unwrap();
    let start = date("2024-02-29");
    let end = date("2025-03-01");
    for dc in [
        Dc::Act360,
        Dc::Act365Fixed,
        Dc::ActActIsda,
        Dc::Thirty360US,
        Dc::Thirty360E,
    ] {
        let q = curve
            .query(&CurveQuery::ForwardRate {
                start: CurvePoint::Date(start),
                end: CurvePoint::Date(end),
                compounding: C::Daily,
                day_count: dc,
            })
            .unwrap();
        let tau = year_fraction(start, end, dc).unwrap();
        near(
            discount_from_rate(q.value, tau, C::Daily).unwrap(),
            df(curve, CurvePoint::Date(end)) / df(curve, CurvePoint::Date(start)),
        );
        assert_eq!(q.day_count, Some(dc));
        assert_eq!(q.time_basis, Dc::Act365Fixed);
        assert_eq!(q.compounding, Some(C::Daily));
    }
    assert!(curve
        .query(&CurveQuery::ForwardRate {
            start: CurvePoint::Tenor(0.1),
            end: CurvePoint::Tenor(1.1),
            compounding: C::Simple,
            day_count: Dc::Act360
        })
        .is_err());
}

proptest! {
    #![proptest_config(Config { cases: 256, rng_seed: RngSeed::Fixed(0x060c), ..Config::default() })]
    #[test]
    fn negative_rates_and_roll_discount_identities(rate_bps in -200_i32..200, roll_days in 1_i64..300, query_days in 301_i64..700) {
        let rate = f64::from(rate_bps) / 10_000.0;
        let mut r = base(); r.source = CurveSource::Nodes(vec![node(0.0, 1.0, QuoteUnit::DiscountFactor),
            node(1.0, (-rate).exp(), QuoteUnit::DiscountFactor), node(3.0, (-3.0 * rate).exp(), QuoteUnit::DiscountFactor)]);
        let original = set(&r); let roll_date = r.reference_date.checked_add_days(roll_days).unwrap();
        let query_date = r.reference_date.checked_add_days(query_days).unwrap();
        let rolled = rolled(&r, "roll", &roll_date.to_string(), RollPolicy::StickyForward);
        let output = build_curve_set(std::slice::from_ref(&rolled), &original).unwrap();
        let after = output.curves.get(&rolled.key).unwrap(); let before = output.curves.get(&r.key).unwrap();
        let expected = df(before, CurvePoint::Date(query_date)) / df(before, CurvePoint::Date(roll_date));
        prop_assert!((df(after, CurvePoint::Date(query_date)) - expected).abs() <= 1e-11);
        prop_assert_eq!(after.recipe().time_basis, before.recipe().time_basis);
    }
}
