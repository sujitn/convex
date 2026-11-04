use convex_core::types::Date;
use convex_core::{daycounts::DayCountConvention, types::Compounding};
use convex_curves::contracts::*;
use convex_curves::{DiscreteCurve, InterpolationMethod, TermStructure, ValueType};
use convex_math::interpolation::{Interpolator, PiecewiseConstant};
use serde_json::{json, Value};

fn reference() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/curve-method-reference.json"
    ))
    .unwrap()
}

fn recipe(grid: &Value, extension: Value) -> CurveRecipe {
    let method = grid["method"].as_str().unwrap();
    let nodes: Vec<Value> = grid["knots"]
        .as_array()
        .unwrap()
        .iter()
        .zip(grid["zeros"].as_array().unwrap())
        .enumerate()
        .map(|(i, (t, z))| {
            let zero = z.as_str().unwrap().parse::<f64>().unwrap();
            json!({"pillar":{"Tenor":t.as_str().unwrap().parse::<f64>().unwrap()},
                "quote":{"value":if i%2==0 {zero} else {zero*100.0},
                    "unit":if i%2==0 {"DecimalRate"} else {"PercentRate"}}})
        })
        .collect();
    serde_json::from_value(json!({"key":{"id":grid["id"],"version":"method.v1"},
        "role":{"kind":"Discount","collateral_currency":"USD"},"currency":"USD",
        "reference_date":"2024-02-29","time_basis":"Act365Fixed",
        "representation":{"ZeroRates":{"compounding":"Continuous"}},
        "interpolation":method,"extrapolation":extension,"dependencies":[],"calibration":"Direct",
        "source":{method:{"nodes":nodes}}}))
    .unwrap()
}

fn build(recipe: &CurveRecipe) -> std::sync::Arc<BuiltCurve> {
    build_curve_set(std::slice::from_ref(recipe), &CurveSet::new())
        .unwrap()
        .curves
        .get(&recipe.key)
        .unwrap()
        .clone()
}

fn near(actual: f64, expected: &Value, discount: bool) {
    let target = expected.as_str().unwrap().parse::<f64>().unwrap();
    let tolerance = 1e-12 + if discount { 1e-12 * target.abs() } else { 0.0 };
    assert!(
        (actual - target).abs() <= tolerance,
        "{actual} differs from {target}"
    );
}

#[test]
fn independent_step_and_flat_forward_points_sides_tails_and_discount_identities() {
    for grid in reference()["grids"].as_array().unwrap() {
        let r = recipe(grid, json!("None"));
        let curve = build(&r);
        assert_eq!(curve.recipe(), &r);
        for point in grid["points"].as_array().unwrap() {
            let t = point["time"].as_str().unwrap().parse::<f64>().unwrap();
            near(
                curve
                    .query(&CurveQuery::Raw {
                        point: CurvePoint::Tenor(t),
                    })
                    .unwrap()
                    .value,
                &point["zero"],
                false,
            );
            near(
                curve
                    .query(&CurveQuery::DiscountFactor {
                        point: CurvePoint::Tenor(t),
                    })
                    .unwrap()
                    .value,
                &point["discount"],
                true,
            );
            if grid["method"] == "FlatForward" {
                for (name, side) in [("left", ForwardSide::Left), ("right", ForwardSide::Right)] {
                    let query = curve.query(&CurveQuery::FlatForwardRate {
                        point: CurvePoint::Tenor(t),
                        side,
                    });
                    if point[name].is_null() {
                        assert!(query.is_err());
                    } else {
                        near(query.unwrap().value, &point[name], false);
                    }
                }
            } else {
                assert!(curve
                    .query(&CurveQuery::FlatForwardRate {
                        point: CurvePoint::Tenor(t),
                        side: ForwardSide::Right
                    })
                    .is_err());
            }
        }
        for tail in grid["tails"].as_array().unwrap() {
            let r = recipe(grid, tail["extension"].clone());
            let curve = build(&r);
            let t = tail["time"].as_str().unwrap().parse::<f64>().unwrap();
            near(
                curve
                    .query(&CurveQuery::Raw {
                        point: CurvePoint::Tenor(t),
                    })
                    .unwrap()
                    .value,
                &tail["zero"],
                false,
            );
            let df = curve
                .query(&CurveQuery::DiscountFactor {
                    point: CurvePoint::Tenor(t),
                })
                .unwrap();
            near(df.value, &tail["discount"], true);
            assert!(df.extrapolated);
        }
        let p = &grid["interval"];
        let forward = curve
            .query(&CurveQuery::ForwardRate {
                start: CurvePoint::Tenor(p["start"].as_str().unwrap().parse().unwrap()),
                end: CurvePoint::Tenor(p["end"].as_str().unwrap().parse().unwrap()),
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            })
            .unwrap();
        near(forward.value, &p["forward"], false);
    }
}

#[test]
fn step_derivatives_and_original_raw_flat_forward_substitution_are_explicit() {
    let step = PiecewiseConstant::new(vec![0.0, 1.0, 2.0], vec![0.03, 0.02, -0.01]).unwrap();
    assert_eq!(step.interpolate(0.999999).unwrap(), 0.03);
    assert_eq!(step.interpolate(1.0).unwrap(), 0.02);
    assert_eq!(step.interpolate(2.0).unwrap(), -0.01);
    assert_eq!(step.derivative(1.5).unwrap(), 0.0);
    assert!(step.derivative(1.0).is_err());
    for bad in [f64::NAN, f64::INFINITY, -1.0, 3.0] {
        assert!(step.interpolate(bad).is_err());
    }
    assert!(PiecewiseConstant::new(vec![0.0, 0.0], vec![0.02, 0.03]).is_err());
    assert!(PiecewiseConstant::new(vec![0.0, 1.0], vec![0.02, f64::NAN]).is_err());
    assert!(PiecewiseConstant::new(vec![0.0], vec![0.02]).is_err());
    let maximum =
        PiecewiseConstant::new((0..65_536).map(f64::from).collect(), vec![0.03; 65_536]).unwrap();
    assert_eq!(maximum.interpolate(65_535.0).unwrap(), 0.03);
    assert!(
        PiecewiseConstant::new((0..65_537).map(f64::from).collect(), vec![0.03; 65_537]).is_err()
    );
    let original = DiscreteCurve::with_extrapolation(
        Date::from_ymd(2024, 2, 29).unwrap(),
        vec![1.0, 2.0],
        vec![0.02, 0.03],
        ValueType::zero_rate(Compounding::Continuous),
        InterpolationMethod::Linear,
        convex_curves::ExtrapolationMethod::FlatForward,
    )
    .unwrap();
    assert!(original.checked_value_at(3.0).is_err());
    assert!(original.value_at(3.0).is_nan());
}

#[test]
fn full_date_basis_origin_tail_and_recipe_validation_remain_explicit() {
    let grid = &reference()["grids"][1];
    let mut r = recipe(grid, json!("FlatForward"));
    let nodes = match &mut r.source {
        CurveSource::FlatForward { nodes } => nodes,
        _ => unreachable!(),
    };
    for (node, days) in nodes.iter_mut().zip([0, 365, 730, 1825]) {
        node.pillar = CurvePoint::Date(r.reference_date.checked_add_days(days).unwrap());
    }
    let curve = build(&r);
    let q = CurveQuery::FlatForwardRate {
        point: CurvePoint::Date(r.reference_date.checked_add_days(730).unwrap()),
        side: ForwardSide::Left,
    };
    near(
        curve.query(&q).unwrap().value,
        &grid["points"][8]["left"],
        false,
    );
    let mut invalid = r.clone();
    invalid.interpolation = InterpolationMethod::Linear;
    assert!(build_curve_set(&[invalid], &CurveSet::new()).is_err());
    let mut invalid = r.clone();
    invalid.representation = CurveRepresentation::DiscountFactors;
    assert!(build_curve_set(&[invalid], &CurveSet::new()).is_err());
    let mut invalid = r.clone();
    if let CurveSource::FlatForward { nodes } = &mut invalid.source {
        nodes[0].quote.value += 0.01;
    }
    assert!(build_curve_set(&[invalid], &CurveSet::new()).is_err());
    let mut short = r.clone();
    if let CurveSource::FlatForward { nodes } = &mut short.source {
        nodes.remove(0);
    }
    let short = build(&short);
    let raw = short
        .query(&CurveQuery::Raw {
            point: CurvePoint::Tenor(0.0),
        })
        .unwrap();
    assert!(raw.extrapolated);
    let forward = short
        .query(&CurveQuery::FlatForwardRate {
            point: CurvePoint::Tenor(0.0),
            side: ForwardSide::Right,
        })
        .unwrap();
    assert!(forward.extrapolated);
    let df = short
        .query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Tenor(0.0),
        })
        .unwrap();
    assert_eq!(df.value, 1.0);
    assert!(!df.extrapolated);
}

#[test]
fn original_piecewise_constant_tag_must_hold_the_actual_interval_value() {
    let curve = DiscreteCurve::new(
        Date::from_ymd(2024, 2, 29).unwrap(),
        vec![1.0, 2.0],
        vec![0.01, 0.03],
        ValueType::HazardRate,
        InterpolationMethod::PiecewiseConstant,
    )
    .unwrap();
    assert_eq!(curve.checked_value_at(1.5).unwrap(), 0.01);
    assert_eq!(curve.value_at(1.5), 0.01);
}
