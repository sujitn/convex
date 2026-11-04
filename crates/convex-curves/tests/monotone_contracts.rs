//! Actual dated native source, independent economics and explicit tail sides.
#[path = "../../../tests/fixtures/packed.rs"]
mod packed;
use convex_core::types::Compounding;
use convex_curves::contracts::*;
use serde_json::Value;

fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}
fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(packed::text("tests/fixtures/library/monotone.json")).unwrap()
        ["cases"]
        .as_array()
        .unwrap()
        .clone()
}
fn build(case: &Value) -> std::sync::Arc<BuiltCurve> {
    let recipes: Vec<CurveRecipe> =
        serde_json::from_value(case["request"]["inputs"]["curves"].clone()).unwrap();
    let built = build_curve_set(&recipes, &CurveSet::new()).unwrap();
    let curve = built.curves.get(&recipes[0].key).unwrap().clone();
    assert_eq!(curve.recipe(), &recipes[0]);
    curve
}

#[test]
fn complete_dated_sources_match_frozen_model_and_preserve_native_units() {
    let reference: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/monotone-reference.json"
    ))
    .unwrap();
    let all = cases();
    for model in reference["models"]
        .as_array()
        .unwrap()
        .iter()
        .chain(reference["dated"].as_array().unwrap())
    {
        let id = format!("monotone.describe.{}", model["id"].as_str().unwrap());
        let curve = build(all.iter().find(|c| c["id"] == id).unwrap());
        assert_eq!(
            curve.monotone_diagnostics().unwrap().model,
            reference["model"]
        );
        assert_eq!(curve.domain().minimum_tenor, 0.0);
        assert_eq!(
            curve.interpolation_grid().unwrap().0.len(),
            model["knots"].as_array().unwrap().len()
        );
        for point in model["points"]
            .as_array()
            .unwrap()
            .iter()
            .chain(model["dated_points"].as_array().into_iter().flatten())
        {
            let p = if point["date"].is_string() {
                CurvePoint::Date(serde_json::from_value(point["date"].clone()).unwrap())
            } else {
                CurvePoint::Tenor(number(&point["time"]))
            };
            let t = match p {
                CurvePoint::Date(d) => curve.date_to_tenor(d).unwrap(),
                CurvePoint::Tenor(t) => t,
            };
            for (query, field) in [
                (CurveQuery::Raw { point: p }, "zero"),
                (CurveQuery::DiscountFactor { point: p }, "discount"),
                (
                    CurveQuery::MonotoneConvexForward {
                        point: p,
                        side: ForwardSide::Right,
                    },
                    "forward",
                ),
            ] {
                let result = curve.query(&query).unwrap();
                let target = number(&point[field]);
                let bound = if field == "discount" {
                    number(&reference["bounds"]["discount_absolute"])
                        + number(&reference["bounds"]["discount_relative"]) * target.abs()
                } else {
                    number(&reference["bounds"]["rate_absolute"])
                };
                assert!(
                    (result.value - target).abs() <= bound,
                    "{} {field} at {t}: {} vs {target}",
                    model["id"],
                    result.value
                );
                assert_eq!(result.curve, curve.recipe().key);
                assert_eq!(result.time_basis, curve.recipe().time_basis);
                assert_eq!(
                    result.extrapolated,
                    t > curve.domain().maximum_tenor
                        || (field == "forward" && t == curve.domain().maximum_tenor)
                );
                if field == "forward" {
                    assert_eq!(result.compounding, Some(Compounding::Continuous));
                    assert_eq!(result.day_count, Some(curve.recipe().time_basis));
                }
            }
        }
    }
}

#[test]
fn terminal_sides_consume_the_declared_economics_without_grid_aliases() {
    let all = cases();
    for tail in ["None", "Flat", "FlatForward"] {
        let mut c = all
            .iter()
            .find(|c| c["id"] == "monotone.describe.steep")
            .unwrap()
            .clone();
        c["request"]["inputs"]["curves"][0]["extrapolation"] = serde_json::json!(tail);
        let curve = build(&c);
        let end = curve.domain().maximum_tenor;
        let point = CurvePoint::Tenor(end);
        let left = curve
            .query(&CurveQuery::MonotoneConvexForward {
                point,
                side: ForwardSide::Left,
            })
            .unwrap();
        assert!(!left.extrapolated);
        let right = curve.query(&CurveQuery::MonotoneConvexForward {
            point,
            side: ForwardSide::Right,
        });
        if tail == "None" {
            assert!(right.is_err());
        } else {
            let right = right.unwrap();
            assert!(right.extrapolated);
            let expected = if tail == "Flat" {
                curve
                    .interpolation_grid()
                    .unwrap()
                    .1
                    .last()
                    .copied()
                    .unwrap()
            } else {
                left.value
            };
            assert_eq!(right.value, expected);
        }
        assert!(curve
            .query(&CurveQuery::MonotoneConvexForward {
                point: CurvePoint::Tenor(0.0),
                side: ForwardSide::Left
            })
            .is_err());
        assert!(
            !curve
                .query(&CurveQuery::Raw {
                    point: CurvePoint::Tenor(0.0)
                })
                .unwrap()
                .extrapolated
        );
    }
}
