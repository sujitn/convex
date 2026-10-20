//! Independent Decimal100 complete-contract first/second parameter differences.
use convex_analytics::floating_coupon::*;
use convex_bonds::contracts::*;
use convex_core::{contracts::UtcTimestamp, fixings::FixingSeries};
use convex_curves::contracts::*;
use serde_json::Value;
use std::cell::Cell;

#[test]
fn complete_parameter_derivatives_match_independent_cash_and_refinement() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/coupon-parameter-derivatives.json")).unwrap();
    for row in fixture["cases"].as_array().unwrap() {
        let input = &row["input"];
        let terms: BondTerms = serde_json::from_value(input["bond"]["terms"].clone()).unwrap();
        let bond = terms.build().unwrap();
        let context: BondProjectionContext =
            serde_json::from_value(input["context"].clone()).unwrap();
        let options: TermCouponOptionInputs =
            serde_json::from_value(input["options"].clone()).unwrap();
        let history =
            FixingSeries::new(serde_json::from_value(row["resources"]["fixings"].clone()).unwrap())
                .unwrap();
        let recipes: Vec<CurveRecipe> =
            serde_json::from_value(row["resources"]["curves"].clone()).unwrap();
        let curves = build_curve_set(&recipes, &CurveSet::new()).unwrap().curves;
        let calls = Cell::new(0);
        let prepared = prepare_term_coupon_model(
            TermCouponModelInputs {
                bond: &bond,
                context: &context,
                history: &history,
                options: &options,
                projection: serde_json::from_value(input["projection"].clone()).unwrap(),
                discount: serde_json::from_value(input["discount"].clone()).unwrap(),
            },
            &curves,
            |_, day| {
                calls.set(calls.get() + 1);
                Ok(UtcTimestamp::parse(&format!("{day}T16:00:00Z")).unwrap())
            },
        )
        .unwrap();
        let original = serde_json::to_value(prepared.baseline()).unwrap();
        let coordinate: CouponParameterCoordinate =
            serde_json::from_value(row["coordinate"].clone()).unwrap();
        let settings: CouponParameterDerivativeSettings =
            serde_json::from_value(row["settings"].clone()).unwrap();
        let result = prepared
            .parameter_derivatives(&curves, &coordinate, settings)
            .unwrap();
        let value = serde_json::to_value(&result).unwrap();
        for name in ["base", "plus", "minus", "half_plus", "half_minus"] {
            let expected = row["expected"][name]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!(
                (value[name]["present_value"].as_f64().unwrap() - expected).abs()
                    <= fixture["absolute_pv_bound"].as_f64().unwrap(),
                "{} {name}",
                row["id"]
            );
            for field in ["terms", "context", "discount_curve"] {
                assert_eq!(value[name][field], original[field]);
            }
            for (a, b) in value[name]["segments"]
                .as_array()
                .unwrap()
                .iter()
                .zip(original["segments"].as_array().unwrap())
            {
                for field in [
                    "start",
                    "end",
                    "request",
                    "rate",
                    "principal",
                    "year_fraction",
                    "entitled",
                ] {
                    assert_eq!(a[field], b[field]);
                }
            }
        }
        for (name, bound) in [
            ("first", "absolute_first_bound"),
            ("half_first", "absolute_first_bound"),
            ("second", "absolute_second_bound"),
            ("half_second", "absolute_second_bound"),
        ] {
            let actual = value[name].as_f64().unwrap();
            let expected = row["expected"][name]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!(
                (actual - expected).abs() <= fixture[bound].as_f64().unwrap(),
                "{} {name}: {actual} != {expected}",
                row["id"]
            );
        }
        assert_eq!(value["base"], original);
        assert_eq!(result.coordinate, coordinate);
        assert!(result.first_resolution > 0.0);
        assert!(result.second_resolution > 0.0);
        assert_eq!(calls.get(), 3);
        if matches!(
            coordinate.parameter,
            CouponModelParameter::NormalFactorLoading { factor: 0 }
        ) {
            let first = CouponParameterDerivativeSettings {
                bump: 0.0005,
                ..settings
            };
            assert!(matches!(
                prepared.parameter_derivatives(&curves, &coordinate, first),
                Err(TermCouponOptionError::UnstableDerivative { order: 1, .. })
            ));
            let second = CouponParameterDerivativeSettings {
                absolute_second_tolerance: 1e-8,
                relative_second_tolerance: 0.0,
                ..settings
            };
            assert!(matches!(
                prepared.parameter_derivatives(&curves, &coordinate, second),
                Err(TermCouponOptionError::UnstableDerivative { order: 2, .. })
            ));
        }
        for bump in [0.0, -settings.bump, f64::NAN, f64::INFINITY] {
            let invalid = CouponParameterDerivativeSettings { bump, ..settings };
            assert!(prepared
                .parameter_derivatives(&curves, &coordinate, invalid)
                .is_err());
        }
        let mut wrong = coordinate.clone();
        wrong.source_version = convex_core::contracts::Version::new(2).unwrap();
        assert!(prepared
            .parameter_derivatives(&curves, &wrong, settings)
            .is_err());
        assert_eq!(
            serde_json::to_value(prepared.price(&curves).unwrap()).unwrap(),
            original
        );
        assert_eq!(calls.get(), 3);
    }
}
