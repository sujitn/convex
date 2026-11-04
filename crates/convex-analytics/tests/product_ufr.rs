//! Full original product callback composition. Quotes in this file are native
//! integration controls; independent financial answers live in product-ufr.json.
#[path = "../../convex/examples/support/rate_fixture.rs"]
mod fixture;
use convex_analytics::rate_calibration::*;
use convex_core::{fixings::FixingSeries, Date};
use convex_curves::{contracts::*, ExtrapolationMethod};
use fixture::Fixture;
use serde_json::Value;

fn composition(f: &Fixture, negative: bool) -> (Vec<RateCurveRecipe>, CurveSet, CurveSet) {
    let data: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/curves/product-ufr-admitted.json"
    ))
    .unwrap();
    let case = &data["cases"][if negative { 9 } else { 6 }];
    let template: RateCurveRecipe =
        serde_json::from_value(case["inputs"]["calibration"]["rates"][0]["recipe"].clone())
            .unwrap();
    let base_template: CurveRecipe =
        serde_json::from_value(case["inputs"]["curves"][0].clone()).unwrap();
    let target: UfrParameters = serde_json::from_value(case["parameters"].clone()).unwrap();
    let mut recipes = f.recipes.clone();
    let mut bases = Vec::new();
    let mut direct = Vec::new();
    for recipe in &mut recipes {
        let calendar = match &recipe.helpers[0].terms {
            RateCalibrationTerms::MoneyMarket(h) => h.calendar.clone(),
            _ => panic!("original calendar"),
        };
        for (j, helper) in template.helpers.iter().enumerate() {
            let mut helper = helper.clone();
            helper.id = format!("{}.tail.deposit.{j}", recipe.curve.key.id);
            let RateCalibrationTerms::MoneyMarket(terms) = &mut helper.terms else {
                panic!("original deposit")
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
        recipe.bootstrap = None;
        let CalibrationMode::Ufr(settings) = &template.curve.calibration else {
            panic!("explicit UFR policy")
        };
        let mut settings = settings.as_ref().clone();
        settings.base.id = format!("{}.analytic.base", recipe.curve.key.id);
        settings.last_liquid_point = CurvePoint::Tenor(0.25);
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
        recipe.curve.dependencies.push(settings.base.clone());
        recipe.curve.extrapolation = ExtrapolationMethod::UfrConvergence {
            ufr: settings.initial.ufr,
            alpha: settings.initial.alpha,
        };
        recipe.curve.calibration = CalibrationMode::Ufr(Box::new(settings.clone()));
        let mut base = base_template.clone();
        base.key = settings.base.clone();
        base.role = recipe.curve.role.clone();
        bases.push(base);
        direct.push(
            settings
                .trial_recipe(&recipe.curve, &target.parameters())
                .unwrap(),
        );
    }
    let supplied = build_curve_set(&bases, &CurveSet::new()).unwrap().curves;
    let mut curves = build_curve_set(&direct, &supplied).unwrap().curves;
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
            // Original direct diagnostics require exactly the supplied policy.
            // Declare the reference target for quote generation only; the fit
            // below retains the original distinct nonzero initial parameters.
            let mut quoted = recipe.clone();
            let CalibrationMode::Ufr(settings) = &mut quoted.curve.calibration else {
                panic!("UFR settings")
            };
            settings.initial = target;
            quoted.curve.extrapolation = ExtrapolationMethod::UfrConvergence {
                ufr: target.ufr,
                alpha: target.alpha,
            };
            reprice_rate_helpers(
                &RateCurveBuildInput {
                    recipe: &quoted,
                    resources,
                },
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
    (recipes, supplied, curves)
}

#[test]
fn exact_ufr_dependencies_use_original_fra_irs_ois_futures_and_basis_callbacks() {
    for negative in [false, true] {
        let f = Fixture::load(if negative {
            "nonflat-negative"
        } else {
            "nonflat-positive"
        })
        .unwrap();
        let (recipes, supplied, reference) = composition(&f, negative);
        let resources = f.resources(&recipes);
        let inputs: Vec<_> = recipes
            .iter()
            .zip(&resources)
            .map(|(recipe, resources)| RateCurveBuildInput { recipe, resources })
            .collect();
        let output = build_rate_curve_set(
            &inputs,
            &supplied,
            &f.history,
            f.context().unwrap(),
            FitNumericalControls::default(),
            |index, at| f.publication(index, at),
        )
        .unwrap();
        assert_eq!(output.calibrations.len(), 3);
        assert_eq!(output.build_order.len(), 4);
        assert!(output.price_evaluations <= MAX_RATE_PRICE_EVALUATIONS);
        for fitted in &output.calibrations {
            assert_eq!(fitted.repriced_helpers.len(), 5);
            assert!(matches!(fitted.solver, RateSolverDiagnostics::Ufr { .. }));
            assert_eq!(fitted.curve.recipe(), &fitted.recipe.curve);
            for quote in &fitted.curve.diagnostics().quotes {
                assert!(quote.residual.abs() <= quote.tolerance);
            }
            for on in ["2026-10-30", "2027-10-11", "2033-10-07", "2046-10-07"] {
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
            panic!("full future")
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
fn original_due_history_preflight_precedes_ufr_trial_and_preserves_last_good() {
    let f = Fixture::load("nonflat-positive").unwrap();
    let (recipes, supplied, _) = composition(&f, false);
    let before: Vec<_> = supplied.keys().cloned().collect();
    let resources = f.resources(&recipes);
    let inputs: Vec<_> = recipes
        .iter()
        .zip(&resources)
        .map(|(recipe, resources)| RateCurveBuildInput { recipe, resources })
        .collect();
    let error = build_rate_curve_set(
        &inputs,
        &supplied,
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
    assert_eq!(supplied.keys().cloned().collect::<Vec<_>>(), before);
}
