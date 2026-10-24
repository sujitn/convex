//! Exact native state identity must not collapse to recipes or finite PV samples.
use convex_curves::contracts::*;
use serde_json::Value;

fn cases() -> Value {
    serde_json::from_str(include_str!(
        "../../convex-analytics/tests/fixtures/coupon-curve-state.json"
    ))
    .unwrap()
}

#[test]
fn independent_rebuilds_and_clones_match_every_complete_original_family() {
    let f = cases();
    for case in f["cases"].as_array().unwrap() {
        let recipes: Vec<CurveRecipe> =
            serde_json::from_value(case["resources"]["curves"].clone()).unwrap();
        let a = build_curve_set(&recipes, &CurveSet::new()).unwrap().curves;
        let b = build_curve_set(&recipes, &CurveSet::new()).unwrap().curves;
        for key in a.keys() {
            let left = a.get(key).unwrap();
            let right = b.get(key).unwrap();
            assert!(!std::sync::Arc::ptr_eq(left, right));
            assert!(
                left.same_valuation_state(right).unwrap(),
                "{} {}",
                case["id"],
                key.id
            );
            assert!(left.same_valuation_state(&left.as_ref().clone()).unwrap());
        }
    }
}

#[test]
fn exact_original_dependency_state_survives_an_unchanged_parent_recipe() {
    let f = cases();
    let mut checked = 0;
    for case in f["cases"].as_array().unwrap().iter().step_by(2) {
        let mut recipes: Vec<CurveRecipe> =
            serde_json::from_value(case["resources"]["curves"].clone()).unwrap();
        if recipes.len() == 2 {
            continue;
        }
        let a = build_curve_set(&recipes, &CurveSet::new()).unwrap().curves;
        match &mut recipes[0].source {
            CurveSource::Parametric {
                parameters: ParametricModel::NelsonSiegel { beta0, .. },
                ..
            } => *beta0 += 0.001,
            CurveSource::Nodes(nodes) => {
                for node in nodes {
                    node.quote.value += 0.001;
                }
            }
            _ => panic!("explicit original dependency family"),
        }
        let b = build_curve_set(&recipes, &CurveSet::new()).unwrap().curves;
        let key = &recipes[1].key;
        assert_eq!(a.get(key).unwrap().recipe(), b.get(key).unwrap().recipe());
        assert!(
            !a.get(key)
                .unwrap()
                .same_valuation_state(b.get(key).unwrap())
                .unwrap(),
            "{}",
            case["id"]
        );
        checked += 1;
    }
    assert_eq!(checked, 7);
}

#[test]
fn same_product_recipe_and_repricing_metadata_cannot_hide_changed_fitted_grid() {
    let f = cases();
    let mut recipe: CurveRecipe =
        serde_json::from_value(f["cases"][0]["resources"]["curves"][0].clone()).unwrap();
    recipe.source = CurveSource::Helpers(vec!["actual.helper.1".into(), "actual.helper.2".into()]);
    recipe.calibration = CalibrationMode::Piecewise;
    let quotes: Vec<QuoteRepricing> = serde_json::from_value(serde_json::json!([
        {"helper_id":"actual.helper.1","pillar":"2024-02-15","input":{"value":0.03,"unit":"DecimalRate"},"model":{"value":0.03,"unit":"DecimalRate"},"residual":0.0,"tolerance":1e-12},
        {"helper_id":"actual.helper.2","pillar":"2024-04-15","input":{"value":0.04,"unit":"DecimalRate"},"model":{"value":0.04,"unit":"DecimalRate"},"residual":0.0,"tolerance":1e-12}
    ])).unwrap();
    let nodes: Vec<CurveNode> = [0.03, 0.03, 0.04]
        .into_iter()
        .zip(["2024-01-15", "2024-02-15", "2024-04-15"])
        .map(|(value, date)| CurveNode {
            pillar: CurvePoint::Date(convex_core::Date::parse(date).unwrap()),
            quote: MarketQuote {
                value,
                unit: QuoteUnit::DecimalRate,
            },
        })
        .collect();
    let a = build_fitted_rate_curve(&recipe, &nodes, quotes.clone(), 2, &CurveSet::new()).unwrap();
    let rebuilt =
        build_fitted_rate_curve(&recipe, &nodes, quotes.clone(), 2, &CurveSet::new()).unwrap();
    assert!(a.same_valuation_state(&rebuilt).unwrap());
    let mut changed = nodes.clone();
    changed[2].quote.value += 0.001;
    let b = build_fitted_rate_curve(&recipe, &changed, quotes, 2, &CurveSet::new()).unwrap();
    assert_eq!(a.recipe(), b.recipe());
    assert_eq!(a.diagnostics(), b.diagnostics());
    assert!(!a.same_valuation_state(&b).unwrap());
}

#[test]
fn full_product_recipe_does_not_replace_actual_fitted_parametric_parameters() {
    let f = cases();
    let mut recipe: CurveRecipe =
        serde_json::from_value(f["cases"][0]["resources"]["curves"][0].clone()).unwrap();
    recipe.interpolation = convex_curves::InterpolationMethod::NelsonSiegel;
    let initial = ParametricModel::NelsonSiegel {
        beta0: 0.03,
        beta1: 0.0,
        beta2: 0.0,
        tau: 2.0,
    };
    recipe.calibration = CalibrationMode::Parametric(Box::new(ProductParametricSettings {
        initial,
        lower: vec![-0.1, -0.1, -0.1, 0.1],
        upper: vec![0.1, 0.1, 0.1, 10.0],
        parameter_scales: vec![0.01, 0.01, 0.01, 1.0],
        quote_scales: vec![0.01; 4],
        weights: vec![1.0; 4],
        max_iterations: 10,
        objective_tolerance: 1e-8,
    }));
    let quotes: Vec<_> = ["2024-02-15", "2024-04-15", "2024-07-15", "2025-01-15"]
        .into_iter()
        .enumerate()
        .map(|(i, date)| QuoteRepricing {
            helper_id: format!("original.product.{i}"),
            pillar: convex_core::Date::parse(date).unwrap(),
            input: MarketQuote {
                value: 0.03,
                unit: QuoteUnit::DecimalRate,
            },
            model: MarketQuote {
                value: 0.03,
                unit: QuoteUnit::DecimalRate,
            },
            residual: 0.0,
            tolerance: 1e-12,
        })
        .collect();
    recipe.source = CurveSource::Helpers(quotes.iter().map(|q| q.helper_id.clone()).collect());
    let horizon = convex_core::Date::parse("2025-01-15").unwrap();
    let a = build_fitted_parametric_curve(
        &recipe,
        horizon,
        &[0.03, 0.0, 0.0, 2.0],
        quotes.clone(),
        2,
        &CurveSet::new(),
    )
    .unwrap();
    let b = build_fitted_parametric_curve(
        &recipe,
        horizon,
        &[0.031, 0.0, 0.0, 2.0],
        quotes,
        2,
        &CurveSet::new(),
    )
    .unwrap();
    assert_eq!(a.recipe(), b.recipe());
    assert_eq!(a.diagnostics(), b.diagnostics());
    assert!(!a.same_valuation_state(&b).unwrap());
}
