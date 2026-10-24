//! Full product composition checks. Quotes here come from native repricing, so
//! these are integration checks, not independent financial control answers.
#[path = "../../convex/examples/support/rate_fixture.rs"]
mod fixture;
use convex_analytics::rate_calibration::*;
use convex_core::{fixings::FixingSeries, Date};
use convex_curves::{contracts::*, ExtrapolationMethod};
use fixture::Fixture;
use serde_json::Value;

fn composition(f: &Fixture, negative: bool) -> (Vec<RateCurveRecipe>, CurveSet) {
    let data: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/curves/product-parametric-conditioned.json"
    ))
    .unwrap();
    let case = &data["cases"][if negative { 3 } else { 0 }];
    let template: RateCurveRecipe =
        serde_json::from_value(case["inputs"]["calibration"]["rates"][0]["recipe"].clone())
            .unwrap();
    let target: ParametricModel = serde_json::from_value(case["model"].clone()).unwrap();
    let mut recipes = f.recipes.clone();
    let mut direct = Vec::new();
    for (i, recipe) in recipes.iter_mut().enumerate() {
        let calendar = match &recipe.helpers[0].terms {
            RateCalibrationTerms::MoneyMarket(h) => h.calendar.clone(),
            _ => panic!("original calendar"),
        };
        for (j, helper) in template.helpers.iter().enumerate() {
            let mut helper = helper.clone();
            helper.id = format!("{}.parametric-deposit.{j}", recipe.curve.key.id);
            let RateCalibrationTerms::MoneyMarket(terms) = &mut helper.terms else {
                panic!("complete original deposit");
            };
            terms.curve = recipe.curve.key.clone();
            terms.calendar = calendar.clone();
            recipe.helpers.push(helper);
        }
        recipe.pillars = recipe.helpers.iter().map(|h| h.pillar).collect();
        recipe.pillars.sort();
        recipe.pillars.dedup();
        recipe.curve.source =
            CurveSource::Helpers(recipe.helpers.iter().map(|h| h.id.clone()).collect());
        recipe.curve.interpolation = template.curve.interpolation;
        recipe.curve.extrapolation = ExtrapolationMethod::None;
        recipe.bootstrap = None;
        let CalibrationMode::Parametric(settings) = &template.curve.calibration else {
            panic!("explicit native parameter policy");
        };
        let mut settings = settings.as_ref().clone();
        settings.weights = vec![1.; recipe.helpers.len()];
        settings.quote_scales = recipe
            .helpers
            .iter()
            .map(|h| match h.quote.unit {
                QuoteUnit::PercentRate | QuoteUnit::FuturePrice100MinusRate => 100.,
                QuoteUnit::BasisPoints => 10000.,
                _ => 1.,
            })
            .collect();
        recipe.curve.calibration = CalibrationMode::Parametric(Box::new(settings));
        let mut model = target.parameters();
        model[0] += i as f64 * 0.003;
        let mut curve = recipe.curve.clone();
        curve.calibration = CalibrationMode::Direct;
        curve.dependencies.clear();
        curve.source = CurveSource::Parametric {
            parameters: target.with_parameters(&model).unwrap(),
            horizon: CurvePoint::Date(*recipe.pillars.last().unwrap()),
            observations: Vec::new(),
        };
        direct.push(curve);
    }
    let mut curves = build_curve_set(&direct, &CurveSet::new()).unwrap().curves;
    let mut alias = direct[0].clone();
    let view = recipes[0].overnight_projection.as_ref().unwrap();
    alias.key = view.key.clone();
    alias.role = CurveRole::Projection {
        index_id: view.index_id.clone(),
    };
    alias.dependencies = vec![direct[0].key.clone()];
    alias.source = CurveSource::OvernightProjection {
        discount: direct[0].key.clone(),
        index_id: view.index_id.clone(),
    };
    curves = build_curve_set(&[alias], &curves).unwrap().curves;
    let resources = f.resources(&recipes);
    let observations: Vec<_> = recipes
        .iter()
        .zip(&resources)
        .map(|(recipe, resources)| {
            reprice_rate_helpers(
                &RateCurveBuildInput { recipe, resources },
                &curves,
                &f.history,
                f.context().unwrap(),
                |index, at| f.publication(index, at),
            )
            .unwrap()
        })
        .collect();
    drop(resources);
    for (recipe, observations) in recipes.iter_mut().zip(observations) {
        for (helper, quote) in recipe.helpers.iter_mut().zip(observations.quotes) {
            *helper = helper.with_quote(quote.model).unwrap();
        }
    }
    (recipes, curves)
}

#[test]
fn exact_parametric_dependencies_use_original_fra_irs_ois_futures_and_basis_callbacks() {
    for negative in [false, true] {
        let f = Fixture::load(if negative {
            "nonflat-negative"
        } else {
            "nonflat-positive"
        })
        .unwrap();
        let (recipes, reference) = composition(&f, negative);
        let resources = f.resources(&recipes);
        let inputs: Vec<_> = recipes
            .iter()
            .zip(&resources)
            .map(|(recipe, resources)| RateCurveBuildInput { recipe, resources })
            .collect();
        let output = build_rate_curve_set(
            &inputs,
            &CurveSet::new(),
            &f.history,
            f.context().unwrap(),
            FitNumericalControls {
                jacobian_step: 1e-8,
                ..FitNumericalControls::default()
            },
            |index, at| f.publication(index, at),
        )
        .unwrap();
        assert_eq!(output.calibrations.len(), 3);
        assert_eq!(output.build_order.len(), 4);
        assert!(output.price_evaluations <= MAX_RATE_PRICE_EVALUATIONS);
        for fitted in &output.calibrations {
            assert_eq!(fitted.repriced_helpers.len(), 7);
            assert!(matches!(
                fitted.solver,
                RateSolverDiagnostics::Parametric { .. }
            ));
            assert_eq!(fitted.curve.recipe(), &fitted.recipe.curve);
            assert!(fitted.curve.interpolation_grid().is_err());
            for quote in &fitted.curve.diagnostics().quotes {
                assert!(quote.residual.abs() <= quote.tolerance);
            }
            for on in ["2026-10-30", "2027-10-11", "2029-10-08", "2034-10-09"] {
                let query = CurveQuery::DiscountFactor {
                    point: CurvePoint::Date(Date::parse(on).unwrap()),
                };
                let actual = fitted.curve.query(&query).unwrap().value;
                let expected = reference
                    .get(&fitted.recipe.curve.key)
                    .unwrap()
                    .query(&query)
                    .unwrap()
                    .value;
                assert!((actual - expected).abs() <= 1e-8);
            }
        }
        let RateHelperValuation::RateFuture(future) = &output.calibrations[0].repriced_helpers[1]
        else {
            panic!("full original future valuation");
        };
        assert_eq!(future.known_days, 21);
        assert_eq!(future.projected_days, 70);
        assert!(matches!(
            &output.calibrations[0].repriced_helpers[2],
            RateHelperValuation::Swap(_)
        ));
        assert!(matches!(
            &output.calibrations[1].repriced_helpers[2],
            RateHelperValuation::Swap(_)
        ));
        assert!(matches!(
            &output.calibrations[2].repriced_helpers[2],
            RateHelperValuation::BasisSwap(_)
        ));
    }
}

#[test]
fn original_due_fixing_and_exact_calendar_preflight_precede_parametric_trials() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let (recipes, _) = composition(&f, false);
    let resources = f.resources(&recipes);
    let inputs: Vec<_> = recipes
        .iter()
        .zip(&resources)
        .map(|(recipe, resources)| RateCurveBuildInput { recipe, resources })
        .collect();
    let error = build_rate_curve_set(
        &inputs,
        &CurveSet::new(),
        &FixingSeries::new(Vec::new()).unwrap(),
        f.context().unwrap(),
        FitNumericalControls::default(),
        |index, at| f.publication(index, at),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        RateCalibrationError::Cashflow(convex_core::cashflows::CashflowError::MissingFixings(_))
    ));
}

#[test]
fn independent_full_cash_controls_also_check_original_supplied_models_without_fitting() {
    use convex_analytics::cash_calibration::*;
    use convex_bonds::contracts::BondCalendars;
    use convex_core::calendars::{DynamicCalendar, WeekendType};
    let data: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/curves/product-parametric-conditioned.json"
    ))
    .unwrap();
    let calendar = DynamicCalendar::new("explicit fictional calendar", WeekendType::SaturdaySunday);
    for case in data["cases"].as_array().unwrap() {
        if !case["id"].as_str().unwrap().contains(".government.") {
            continue;
        }
        let recipe: CashCurveRecipe =
            serde_json::from_value(case["inputs"]["calibration"]["cash"][0]["recipe"].clone())
                .unwrap();
        let mut direct = recipe.curve.clone();
        direct.calibration = CalibrationMode::Direct;
        direct.source = CurveSource::Parametric {
            parameters: serde_json::from_value(case["model"].clone()).unwrap(),
            horizon: CurvePoint::Date(*recipe.pillars.last().unwrap()),
            observations: Vec::new(),
        };
        let supplied = build_curve_set(&[direct], &CurveSet::new()).unwrap().curves;
        let resources: Vec<_> = recipe
            .helpers
            .iter()
            .map(|h| match h {
                CashCalibrationHelper::Bond { input } => CashHelperResources::Bond {
                    ids: &input.terms.calendars,
                    calendars: BondCalendars {
                        payment: &calendar,
                        reset: &calendar,
                        observation: &calendar,
                        exercise: &calendar,
                    },
                },
                _ => panic!("original full cash terms"),
            })
            .collect();
        let result = reprice_cash_helpers(
            &CashCurveBuildInput {
                recipe: &recipe,
                resources: &resources,
            },
            &supplied,
        )
        .unwrap();
        println!("{}: {:?}", case["id"], result.quotes);
        assert!(result.within_tolerance);
    }
}
