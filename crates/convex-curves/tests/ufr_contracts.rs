//! Full native join, source conservation, independent dated controls and migration.
use convex_core::{daycounts::DayCountConvention, types::Date};
use convex_curves::contracts::*;
use convex_curves::{
    DiscreteCurve, ExtrapolationMethod, InterpolationMethod, TermStructure, ValueType,
};
use serde_json::Value;

fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}
fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("../../../tests/fixtures/library/ufr.json")).unwrap()
        ["cases"]
        .as_array()
        .unwrap()
        .clone()
}
fn build(case: &Value) -> std::sync::Arc<BuiltCurve> {
    let recipes: Vec<CurveRecipe> =
        serde_json::from_value(case["request"]["inputs"]["curves"].clone()).unwrap();
    let built = build_curve_set(&recipes, &CurveSet::new()).unwrap();
    assert_eq!(
        built.curves.get(&recipes[0].key).unwrap().recipe(),
        &recipes[0]
    );
    built.curves.get(&recipes[0].key).unwrap().clone()
}

#[test]
fn independent_full_curve_and_actual_dated_tail_values_preserve_original_units() {
    let reference: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/ufr-reference.json"
    ))
    .unwrap();
    let all = cases();
    let rate = number(&reference["bounds"]["rate_absolute"]);
    for model in reference["models"]
        .as_array()
        .unwrap()
        .iter()
        .chain(reference["dated"].as_array().unwrap())
    {
        let id = format!("ufr.describe.{}", model["id"].as_str().unwrap());
        let source = all.iter().find(|c| c["id"] == id).unwrap();
        let curve = build(source);
        let diag = curve.ufr_diagnostics().unwrap();
        assert!((diag.boundary_zero - number(&model["boundary_zero"])).abs() <= rate);
        assert!((diag.boundary_forward - number(&model["boundary_forward"])).abs() <= rate);
        assert_eq!(
            curve.diagnostics().dependencies,
            curve.recipe().dependencies
        );
        assert!(curve.interpolation_grid().is_err());
        for p in model["points"].as_array().unwrap() {
            let point = if p["date"].is_string() {
                CurvePoint::Date(serde_json::from_value(p["date"].clone()).unwrap())
            } else {
                CurvePoint::Tenor(number(&p["time"]))
            };
            for (q, field) in [
                (CurveQuery::Raw { point }, "zero"),
                (CurveQuery::DiscountFactor { point }, "discount"),
                (CurveQuery::UfrForward { point }, "forward"),
            ] {
                let result = curve.query(&q).unwrap();
                let target = number(&p[field]);
                let bound = if field == "discount" {
                    number(&reference["bounds"]["discount_absolute"])
                        + number(&reference["bounds"]["discount_relative"]) * target.abs()
                } else {
                    rate
                };
                assert!(
                    (result.value - target).abs() <= bound,
                    "{} {field}: {} vs {target}",
                    model["id"],
                    result.value
                );
                assert_eq!(result.curve, curve.recipe().key);
                assert_eq!(result.time_basis, curve.recipe().time_basis);
                let t = match point {
                    CurvePoint::Tenor(t) => t,
                    CurvePoint::Date(d) => curve.date_to_tenor(d).unwrap(),
                };
                assert_eq!(
                    result.extrapolated,
                    field == "forward" || t > diag.boundary_tenor
                );
                if field == "forward" {
                    assert_eq!(
                        result.compounding,
                        Some(convex_core::types::Compounding::Continuous)
                    );
                    assert_eq!(result.day_count, Some(curve.recipe().time_basis));
                }
            }
        }
    }
}

#[test]
fn join_uses_actual_left_interval_and_source_remains_immutable() {
    let all = cases();
    let curve = build(
        all.iter()
            .find(|c| c["id"] == "ufr.describe.knot_cutoff")
            .unwrap(),
    );
    let join = curve.ufr_diagnostics().unwrap();
    assert!((join.boundary_forward - 0.02).abs() < 1e-14);
    assert_eq!(join.boundary_tenor, 10.0);
    assert_eq!(curve.domain().maximum_tenor, 10.0);
    assert!(
        (curve
            .query(&CurveQuery::Raw {
                point: CurvePoint::Tenor(5.0)
            })
            .unwrap()
            .value
            - 0.02)
            .abs()
            < 1e-14
    );
    assert_eq!(
        curve
            .query(&CurveQuery::DiscountFactor {
                point: CurvePoint::Tenor(0.0)
            })
            .unwrap()
            .value,
        1.0
    );
    let source: Vec<CurveRecipe> = serde_json::from_value(
        all.iter()
            .find(|c| c["id"] == "ufr.describe.knot_cutoff")
            .unwrap()["request"]["inputs"]["curves"]
            .clone(),
    )
    .unwrap();
    let original = build_curve_set(&source, &CurveSet::new()).unwrap();
    let base = original.curves.get(&source[1].key).unwrap();
    let right = base
        .query(&CurveQuery::FlatForwardRate {
            point: CurvePoint::Tenor(10.0),
            side: ForwardSide::Right,
        })
        .unwrap();
    assert!((right.value - join.boundary_forward).abs() > 0.009);
    assert!(curve
        .query(&CurveQuery::UfrForward {
            point: CurvePoint::Tenor(9.0)
        })
        .is_err());
}

#[test]
fn compatibility_ufr_has_no_short_end_derivative_or_invalid_parameter_fallback() {
    let date = Date::from_ymd(2024, 2, 29).unwrap();
    let make = |method, alpha| {
        DiscreteCurve::with_extrapolation(
            date,
            vec![1.0, 2.0, 3.0],
            vec![0.02, 0.025, 0.03],
            ValueType::continuous_zero(DayCountConvention::Act365Fixed),
            method,
            ExtrapolationMethod::UfrConvergence { ufr: 0.042, alpha },
        )
        .unwrap()
    };
    let linear = make(InterpolationMethod::Linear, 0.1);
    assert!(linear.checked_value_at(0.5).is_err());
    assert!(linear.value_at(0.5).is_nan());
    assert_eq!(linear.value_at(4.0), linear.checked_value_at(4.0).unwrap());
    for curve in [
        make(InterpolationMethod::Linear, 0.0),
        make(InterpolationMethod::PiecewiseConstant, 0.1),
    ] {
        assert!(curve.checked_value_at(4.0).is_err());
        assert!(curve.value_at(4.0).is_nan());
    }
}
