//! Independently declared full native PV targets and actual model parameters.
use convex_analytics::floating_coupon::*;
use convex_bonds::contracts::*;
use convex_core::{contracts::*, fixings::FixingSeries};
use convex_curves::contracts::*;
use serde_json::Value;
use std::cell::Cell;

#[test]
fn full_parameter_inverse_matches_original_independent_prices_and_parameters() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/coupon-parameter-inverse.json")).unwrap();
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
        let target: ModelMoney = serde_json::from_value(row["target"].clone()).unwrap();
        let settings: CouponParameterInverseSettings =
            serde_json::from_value(row["settings"].clone()).unwrap();
        let original = serde_json::to_value(prepared.baseline()).unwrap();
        let result = prepared
            .parameter_price_inverse(&curves, &coordinate, target, settings)
            .unwrap();
        assert!(
            (result.parameter - row["expected_parameter"].as_f64().unwrap()).abs()
                <= fixture["absolute_parameter_bound"].as_f64().unwrap(),
            "{}",
            row["id"]
        );
        assert!(
            (result.valuation.present_value - row["expected_pv"].as_f64().unwrap()).abs()
                <= fixture["absolute_pv_bound"].as_f64().unwrap()
        );
        assert!(
            (result.base.present_value - row["expected_base_pv"].as_f64().unwrap()).abs()
                <= fixture["absolute_pv_bound"].as_f64().unwrap()
        );
        assert_eq!(serde_json::to_value(&result.base).unwrap(), original);
        assert_eq!(result.coordinate, coordinate);
        assert_eq!(result.target, target);
        assert!(!result.globally_unique);
        assert_eq!(
            result.native_residual,
            result.valuation.present_value - target.value().get()
        );
        assert!(result.native_residual.abs() <= settings.price_tolerance);
        assert!(
            result.price_resolution > 0. && result.price_resolution <= settings.price_tolerance
        );
        assert_eq!(result.price_evaluations, result.evaluations.len());
        assert!(result.price_evaluations <= settings.max_price_evaluations);
        assert!(result.iterations <= settings.max_iterations);
        assert_eq!(result.evaluations[0].parameter, result.original_parameter);
        for point in result.evaluations.iter().skip(1) {
            assert!((settings.lower..=settings.upper).contains(&point.parameter));
            assert_eq!(point.residual, point.present_value - target.value().get());
        }
        let last = result.evaluations.last().unwrap();
        assert_eq!(last.parameter, result.parameter);
        assert_eq!(last.present_value, result.valuation.present_value);
        if let Some(endpoint) = row["expected_endpoint"].as_u64() {
            assert!(matches!(
                result.termination,
                CouponInverseTermination::EndpointPriceTolerance
            ));
            assert_eq!(
                result.parameter,
                if endpoint == 0 {
                    settings.lower
                } else {
                    settings.upper
                }
            );
        } else if matches!(result.termination, CouponInverseTermination::RefinedBracket) {
            assert!(
                result.upper.parameter - result.lower.parameter <= settings.parameter_tolerance
            );
        }
        for value in [&result.base, &result.valuation] {
            let value = serde_json::to_value(value).unwrap();
            assert_eq!(value["terms"], input["bond"]["terms"]);
            assert_eq!(value["context"], input["context"]);
            assert_eq!(value["discount_curve"], input["discount"]);
        }
        assert_eq!(calls.get(), 3);
        if row["expected_endpoint"].is_null() {
            let short = CouponParameterInverseSettings {
                max_iterations: 1,
                ..settings
            };
            assert!(matches!(
                prepared.parameter_price_inverse(&curves, &coordinate, target, short),
                Err(TermCouponOptionError::InverseConvergence { iterations: 1, .. })
            ));
            let bounded = CouponParameterInverseSettings {
                max_price_evaluations: 4,
                ..settings
            };
            assert!(matches!(
                prepared.parameter_price_inverse(&curves, &coordinate, target, bounded),
                Err(TermCouponOptionError::InverseWork {
                    price_evaluations: 4,
                    limit: 4
                })
            ));
        }
        let unresolved = CouponParameterInverseSettings {
            price_tolerance: 1e-15,
            ..settings
        };
        assert!(matches!(
            prepared.parameter_price_inverse(&curves, &coordinate, target, unresolved),
            Err(TermCouponOptionError::InverseResolution { .. })
        ));
        for tolerance in [0., -1., f64::NAN, f64::INFINITY] {
            let invalid = CouponParameterInverseSettings {
                price_tolerance: tolerance,
                ..settings
            };
            assert!(prepared
                .parameter_price_inverse(&curves, &coordinate, target, invalid)
                .is_err());
        }
        assert_eq!(calls.get(), 3);
        assert_eq!(
            serde_json::to_value(prepared.price(&curves).unwrap()).unwrap(),
            original
        );
    }
}
