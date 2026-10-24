//! Independent frozen analytic economics over the original native curve owners.
use convex_curves::contracts::*;
use serde_json::Value;

fn reference() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/curve-family-reference.json"
    ))
    .unwrap()
}
fn supplied() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!(
        "../../../tests/fixtures/library/curve-families.json"
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
fn curves(case: &Value) -> CurveSet {
    let recipes: Vec<CurveRecipe> =
        serde_json::from_value(case["request"]["inputs"]["curves"].clone()).unwrap();
    build_curve_set(&recipes, &CurveSet::new()).unwrap().curves
}
fn close(actual: f64, expected: &Value, bound: f64) {
    let target = expected.as_str().unwrap().parse::<f64>().unwrap();
    assert!(
        (actual - target).abs() <= bound,
        "actual {actual:.16e}; target {target:.16e}; bound {bound}"
    );
}
#[test]
fn predeclared_exact_models_negative_rates_origins_horizons_and_forwards() {
    let r = reference();
    let cases = supplied();
    for c in r["cases"].as_array().unwrap() {
        for (suffix, field) in [
            ("zero", "zero"),
            ("discount", "discount"),
            ("forward", "instantaneous_forward"),
        ] {
            let id = format!("curve.model.{suffix}.{}", c["id"].as_str().unwrap());
            let input = cases.iter().find(|v| v["id"] == id).unwrap();
            let set = curves(input);
            let key: CurveKey =
                serde_json::from_value(input["request"]["operation"]["curve"].clone()).unwrap();
            let query: CurveQuery =
                serde_json::from_value(input["request"]["operation"]["query"].clone()).unwrap();
            let curve = set.get(&key).unwrap();
            let output = curve.query(&query).unwrap();
            let bound = r["bounds"][if suffix == "discount" {
                "discount_absolute"
            } else {
                "rate_absolute"
            }]
            .as_f64()
            .unwrap();
            close(output.value, &c[field], bound);
            assert!(!output.extrapolated);
            assert!(
                curve.interpolation_grid().is_err(),
                "parametric grid substitution"
            );
        }
    }
}
#[test]
fn original_dated_observation_units_revisions_and_all_native_residuals_survive() {
    for case in supplied().into_iter().filter(|c| {
        c["id"]
            .as_str()
            .unwrap()
            .starts_with("curve.model.observations.")
    }) {
        let set = curves(&case);
        let key: CurveKey =
            serde_json::from_value(case["request"]["inputs"]["curves"][0]["key"].clone()).unwrap();
        let curve = set.get(&key).unwrap();
        let d = curve.structured_diagnostics().unwrap();
        assert_eq!(d.observations.len(), 10);
        for (i, o) in d.observations.iter().enumerate() {
            assert_eq!(o.input.id, format!("zero.{i}"));
            assert_eq!(o.input.version, format!("quote.{}", i + 7));
            assert_eq!(o.model.unit, o.input.quote.unit);
            assert_eq!(o.residual, o.model.value - o.input.quote.value);
            assert!(o.residual.abs() <= o.input.tolerance);
        }
        assert_eq!(
            serde_json::to_value(curve.recipe()).unwrap(),
            case["request"]["inputs"]["curves"][0]
        );
    }
}
#[test]
fn bounded_original_parametric_fits_identify_parameters_and_reprice_frozen_targets() {
    let r = reference();
    for c in supplied()
        .into_iter()
        .filter(|c| c["id"].as_str().unwrap().starts_with("curve.model.fit."))
    {
        let recipe: CurveRecipe =
            serde_json::from_value(c["request"]["operation"]["recipe"].clone()).unwrap();
        let settings: ParametricFitSettings =
            serde_json::from_value(c["request"]["operation"]["settings"].clone()).unwrap();
        let out = fit_parametric_curve(&recipe, &settings).unwrap();
        let actual = out
            .curve
            .structured_diagnostics()
            .unwrap()
            .parameters
            .as_ref()
            .unwrap();
        let id = c["id"]
            .as_str()
            .unwrap()
            .strip_prefix("curve.model.fit.")
            .unwrap();
        let id = id.strip_prefix("exact_start.").unwrap_or(id);
        let target = r["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == id)
            .unwrap();
        let mut target = target.clone();
        target.as_object_mut().unwrap().remove("id");
        let target: ParametricModel = serde_json::from_value(target).unwrap();
        let beta_count = if actual.parameters().len() == 4 { 3 } else { 4 };
        for (i, (a, b)) in actual
            .parameters()
            .iter()
            .zip(target.parameters())
            .enumerate()
        {
            assert!(
                (a - b).abs()
                    <= r["bounds"][if i < beta_count {
                        "fit_beta_absolute"
                    } else {
                        "fit_tau_absolute"
                    }]
                    .as_f64()
                    .unwrap(),
                "{id}: actual {a} target {b}"
            );
        }
        assert_eq!(
            serde_json::to_value(&out.input_recipe).unwrap(),
            serde_json::to_value(&recipe).unwrap()
        );
        assert_eq!(out.settings, settings);
        assert_eq!(
            out.solver.rank,
            Some(beta_count + if beta_count == 3 { 1 } else { 2 })
        );
        assert!(out.solver.evaluations > 0);
        assert!(out.solver.iterations <= settings.max_iterations);
        assert_eq!(out.solver.native_residuals.len(), 10);
        for c in r["cases"].as_array().unwrap().iter().filter(|c| {
            c["id"]
                .as_str()
                .unwrap()
                .starts_with(&(id.to_owned() + "."))
        }) {
            let t = c["tenor"].as_str().unwrap().parse().unwrap();
            let price = out
                .curve
                .query(&CurveQuery::DiscountFactor {
                    point: CurvePoint::Tenor(t),
                })
                .unwrap();
            close(
                price.value,
                &c["discount"],
                r["bounds"]["discount_absolute"].as_f64().unwrap(),
            );
        }
    }
}
#[test]
fn complete_native_segment_boundaries_and_delegated_raw_values_match_independent_controls() {
    let r = reference();
    let cases = supplied();
    for c in r["segments"].as_array().unwrap() {
        for kind in ["Raw", "DiscountFactor"] {
            let id = format!("curve.segment.{kind}.{}", c["tenor"].as_str().unwrap());
            let case = cases.iter().find(|c| c["id"] == id).unwrap();
            let set = curves(case);
            let query: CurveQuery =
                serde_json::from_value(case["request"]["operation"]["query"].clone()).unwrap();
            let key: CurveKey =
                serde_json::from_value(case["request"]["operation"]["curve"].clone()).unwrap();
            let curve = set.get(&key).unwrap();
            close(
                curve.query(&query).unwrap().value,
                &c[if kind == "Raw" { "zero" } else { "discount" }],
                1e-10,
            );
            assert_eq!(curve.structured_diagnostics().unwrap().joins.len(), 1);
            assert!(curve.interpolation_grid().is_err());
        }
    }
    for fallback in ["Clamp", "FlatExtrapolation"] {
        for c in r["delegated"].as_array().unwrap() {
            let id = format!("curve.delegate.{fallback}.{}", c["tenor"].as_str().unwrap());
            let case = cases.iter().find(|c| c["id"] == id).unwrap();
            let set = curves(case);
            let key: CurveKey =
                serde_json::from_value(case["request"]["operation"]["curve"].clone()).unwrap();
            let curve = set.get(&key).unwrap();
            let t = c["tenor"].as_str().unwrap().parse().unwrap();
            close(
                curve
                    .query(&CurveQuery::Raw {
                        point: CurvePoint::Tenor(t),
                    })
                    .unwrap()
                    .value,
                &c["zero"],
                1e-10,
            );
            if t > 0.0 {
                close(
                    curve
                        .query(&CurveQuery::DiscountFactor {
                            point: CurvePoint::Tenor(t),
                        })
                        .unwrap()
                        .value,
                    &c["discount"],
                    3e-10,
                );
            }
        }
    }
}
