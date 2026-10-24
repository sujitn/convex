//! Direct native diagnostics reusing separately declared original rate controls.
#[path = "../../convex/examples/support/rate_fixture.rs"]
mod fixture;
use convex_analytics::rate_calibration::*;
use convex_core::{cashflows::CashflowError, fixings::FixingSeries};
use convex_curves::contracts::*;
use std::collections::BTreeMap;

#[test]
fn original_independent_quotes_and_full_products_reprice_without_a_solver() {
    for id in ["nonflat-positive", "nonflat-negative"] {
        let f = fixture::Fixture::load(id).unwrap();
        let curves = f.reference_curves().unwrap();
        let resources = f.resources(&f.recipes);
        let before: Vec<_> = curves.keys().cloned().collect();
        for (recipe, resources) in f.recipes.iter().zip(&resources) {
            let mut calls = BTreeMap::new();
            let out = reprice_rate_helpers(
                &RateCurveBuildInput { recipe, resources },
                &curves,
                &f.history,
                f.context().unwrap(),
                |index, date| {
                    *calls.entry((index.clone(), date)).or_insert(0usize) += 1;
                    f.publication(index, date)
                },
            )
            .unwrap();
            assert_eq!(out.price_evaluations, recipe.helpers.len());
            assert_eq!(out.valuations.len(), recipe.helpers.len());
            assert!(out.within_tolerance);
            for (actual, original) in out.quotes.iter().zip(&recipe.helpers) {
                assert_eq!(actual.input, original.quote);
                assert_eq!(actual.model.unit, original.quote.unit);
                assert!(actual.residual.abs() <= original.tolerance);
            }
            assert!(calls.values().all(|n| *n == 1));
            assert_eq!(
                serde_json::to_value(&out.recipe).unwrap(),
                serde_json::to_value(recipe).unwrap()
            );
        }
        assert_eq!(curves.keys().cloned().collect::<Vec<_>>(), before);
    }
}
#[test]
fn missing_due_history_precedes_even_an_insufficient_query_domain() {
    let f = fixture::Fixture::load("nonflat-positive").unwrap();
    let resources = f.resources(&f.recipes);
    let mut direct = f
        .reference_curves()
        .unwrap()
        .get(&f.recipes[1].curve.key)
        .unwrap()
        .recipe()
        .clone();
    if let CurveSource::Nodes(nodes) = &mut direct.source {
        nodes.truncate(2);
    }
    let curves = build_curve_set(&[direct], &CurveSet::new()).unwrap().curves;
    // Supply the discount curve, but leave projection too short for the IRS.
    let discount = f
        .reference_curves()
        .unwrap()
        .get(&f.recipes[0].curve.key)
        .unwrap()
        .clone();
    let curves = curves.with_curve(discount.as_ref().clone()).unwrap();
    let error = reprice_rate_helpers(
        &RateCurveBuildInput {
            recipe: &f.recipes[1],
            resources: &resources[1],
        },
        &curves,
        &FixingSeries::new(vec![]).unwrap(),
        f.context().unwrap(),
        |i, d| f.publication(i, d),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        RateCalibrationError::Cashflow(CashflowError::MissingFixings(_))
    ));
}
#[test]
fn changed_supplied_curve_methods_and_resource_counts_fail_before_pricing() {
    let f = fixture::Fixture::load("nonflat-positive").unwrap();
    let resources = f.resources(&f.recipes);
    let curves = f.reference_curves().unwrap();
    let input = RateCurveBuildInput {
        recipe: &f.recipes[0],
        resources: &resources[0][..1],
    };
    assert!(
        reprice_rate_helpers(&input, &curves, &f.history, f.context().unwrap(), |i, d| f
            .publication(i, d))
        .is_err()
    );
    let mut recipe = f.recipes[0].clone();
    recipe.curve.extrapolation = convex_curves::ExtrapolationMethod::Flat;
    let input = RateCurveBuildInput {
        recipe: &recipe,
        resources: &resources[0],
    };
    assert!(
        reprice_rate_helpers(&input, &curves, &f.history, f.context().unwrap(), |i, d| f
            .publication(i, d))
        .is_err()
    );
}
