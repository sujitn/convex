//! Independently declared full-price one-sided derivatives and actual model bounds.
use convex_analytics::floating_coupon::*;
use convex_bonds::contracts::*;
use convex_core::{contracts::*, fixings::FixingSeries};
use convex_curves::contracts::*;
use serde_json::Value;
use std::cell::Cell;

#[test]
fn original_directional_parameters_preserve_full_prices_and_refinement() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/coupon-parameter-directional.json")).unwrap();
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
        let coordinate: CouponParameterCoordinate =
            serde_json::from_value(row["coordinate"].clone()).unwrap();
        let settings: CouponDirectionalDerivativeSettings =
            serde_json::from_value(row["settings"].clone()).unwrap();
        let original = serde_json::to_value(prepared.baseline()).unwrap();
        let result = prepared
            .parameter_directional_derivatives(&curves, &coordinate, settings)
            .unwrap();
        let actual = serde_json::to_value(&result).unwrap();
        for field in ["first", "half_first", "second", "half_second"] {
            let bound = if field.contains("second") {
                fixture["absolute_second_bound"].as_f64().unwrap()
            } else {
                fixture["absolute_first_bound"].as_f64().unwrap()
            };
            assert!(
                (actual[field].as_f64().unwrap()
                    - row["expected"][field]
                        .as_str()
                        .unwrap()
                        .parse::<f64>()
                        .unwrap())
                .abs()
                    <= bound,
                "{}/{field}: {}",
                row["id"],
                actual[field]
            );
        }
        assert_eq!(result.coordinate, coordinate);
        assert_eq!(actual["settings"], row["settings"]);
        assert_eq!(result.price_evaluations, 6);
        assert_eq!(result.prices.len(), 6);
        assert_eq!(actual["prices"][0]["valuation"], original);
        assert!(result.first_resolution > 0. && result.second_resolution > 0.);
        for (price, expected) in result
            .prices
            .iter()
            .zip(row["expected"]["prices"].as_array().unwrap())
        {
            assert_eq!(
                price.multiplier,
                expected["multiplier"]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap()
            );
            assert!(
                (price.parameter
                    - expected["parameter"]
                        .as_str()
                        .unwrap()
                        .parse::<f64>()
                        .unwrap())
                .abs()
                    < 1e-14
            );
            assert!(
                (price.valuation.present_value
                    - expected["present_value"]
                        .as_str()
                        .unwrap()
                        .parse::<f64>()
                        .unwrap())
                .abs()
                    <= fixture["absolute_pv_bound"].as_f64().unwrap()
            );
            let value = serde_json::to_value(&price.valuation).unwrap();
            assert_eq!(value["terms"], input["bond"]["terms"]);
            assert_eq!(value["context"], input["context"]);
            assert_eq!(value["discount_curve"], input["discount"]);
        }
        assert_eq!(calls.get(), 3);
        for bump in [0., -1., f64::NAN, f64::INFINITY] {
            let invalid = CouponDirectionalDerivativeSettings {
                derivative: CouponParameterDerivativeSettings {
                    bump,
                    ..settings.derivative
                },
                ..settings
            };
            assert!(prepared
                .parameter_directional_derivatives(&curves, &coordinate, invalid)
                .is_err());
        }
        if row["id"].as_str().unwrap().contains("constant_collar") {
            for order in [1, 2] {
                let mut invalid = settings;
                invalid.derivative.bump = 1e-8;
                if order == 1 {
                    invalid.derivative.absolute_first_tolerance = 1e-10;
                    invalid.derivative.relative_first_tolerance = 0.;
                } else {
                    invalid.derivative.relative_second_tolerance = 0.;
                }
                assert!(
                    matches!(prepared.parameter_directional_derivatives(&curves, &coordinate, invalid), Err(TermCouponOptionError::DerivativeResolution { order: actual, .. }) if actual == order)
                );
            }
        }
        assert_eq!(calls.get(), 3);
        assert_eq!(
            serde_json::to_value(prepared.price(&curves).unwrap()).unwrap(),
            original
        );
    }
}
