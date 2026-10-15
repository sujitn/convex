//! Independent full-contract native/canonical parameter risk and complete hedges.
use convex_analytics::{floating_coupon::*, quote_risk::*};
use convex_bonds::contracts::*;
use convex_core::{contracts::*, fixings::FixingSeries};
use convex_curves::contracts::*;
use serde_json::Value;
use std::{cell::Cell, collections::HashSet};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/coupon-directional-risk.json")).unwrap()
}
fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}
fn with_prepared(
    row: &Value,
    f: impl FnOnce(&PreparedTermCouponModel<'_>, &CurveSet, &FixingSeries),
) {
    let input = &row["input"];
    let terms: BondTerms = serde_json::from_value(input["bond"]["terms"].clone()).unwrap();
    let bond = terms.build().unwrap();
    let context: BondProjectionContext = serde_json::from_value(input["context"].clone()).unwrap();
    let options: TermCouponOptionInputs = serde_json::from_value(input["options"].clone()).unwrap();
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
    assert_eq!(calls.get(), 3);
    let original = serde_json::to_value(prepared.baseline()).unwrap();
    f(&prepared, &curves, &history);
    assert_eq!(calls.get(), 3);
    assert_eq!(
        serde_json::to_value(prepared.price(&curves).unwrap()).unwrap(),
        original
    );
}

#[test]
fn signed_directional_risk_retains_native_proof_and_both_precision_orders() {
    let fixture = fixture();
    let mut keys = HashSet::new();
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 72);
    for row in fixture["cases"].as_array().unwrap() {
        with_prepared(row, |prepared, curves, _| {
            let key: QuoteRiskKey = serde_json::from_value(row["key"].clone()).unwrap();
            let point: CouponParameterCoordinate =
                serde_json::from_value(row["coordinate"].clone()).unwrap();
            assert_eq!(coupon_parameter_coordinate(&key).unwrap(), point);
            assert_eq!(
                coupon_parameter_key(key.definition().factor.clone(), point).unwrap(),
                key
            );
            keys.insert(key.clone());
            let position: RiskPosition = serde_json::from_value(row["position"].clone()).unwrap();
            let snapshot: SnapshotId = serde_json::from_value(row["snapshot_id"].clone()).unwrap();
            let settings: CouponDirectionalRiskSettings =
                serde_json::from_value(row["settings"].clone()).unwrap();
            let result = term_coupon_directional_parameter_sensitivity(
                prepared,
                curves,
                key.clone(),
                position.clone(),
                snapshot.clone(),
                settings,
            )
            .unwrap();
            assert_eq!(result.key, key);
            assert_eq!(result.position, position);
            assert_eq!(result.snapshot_id, snapshot);
            assert_eq!(result.position_scale, position.scale().unwrap());
            assert_eq!(result.second_unit, "currency_per_model_parameter_squared");
            let value = serde_json::to_value(&result).unwrap();
            assert_eq!(value["settings"], row["settings"]);
            assert_eq!(value["native"]["settings"], row["native_settings"]);
            assert_eq!(result.native.price_evaluations, 6);
            for field in ["first", "half_first", "second", "half_second"] {
                let bound = number(
                    &row[if field.contains("second") {
                        "absolute_scaled_second_bound"
                    } else {
                        "absolute_scaled_first_bound"
                    }],
                );
                assert!(
                    (value[field].as_f64().unwrap() - number(&row["expected"][field])).abs()
                        <= bound,
                    "{}/{field}",
                    row["id"]
                );
                assert_eq!(
                    value[field].as_f64().unwrap(),
                    value["native"][field].as_f64().unwrap() * result.position_scale
                );
            }
            for (actual, expected) in result
                .scaled_prices
                .iter()
                .zip(row["expected"]["scaled_prices"].as_array().unwrap())
            {
                assert!(
                    (*actual - number(expected)).abs() <= number(&row["absolute_scaled_pv_bound"])
                );
            }
            assert_eq!(result.scaled_prices.len(), 6);
            assert_eq!(result.native.prices.len(), 6);
            for (price, scaled) in result.native.prices.iter().zip(&result.scaled_prices) {
                assert_eq!(
                    *scaled,
                    price.valuation.present_value * result.position_scale
                );
            }
            assert!(result.first_resolution >= 0. && result.second_resolution >= 0.);
            let constant = row["id"].as_str().unwrap().contains("constant_collar");
            if position.quantity.decimal().is_zero() {
                assert!(result.scaled_prices.iter().all(|p| *p == 0.));
                assert_eq!(result.first, 0.);
                assert_eq!(result.second, 0.);
                if constant {
                    for order in [1, 2] {
                        let mut invalid = settings;
                        invalid.native.derivative.bump = 1e-8;
                        if order == 1 {
                            invalid.native.derivative.absolute_first_tolerance = 1e-10;
                            invalid.native.derivative.relative_first_tolerance = 0.;
                        } else {
                            invalid.native.derivative.relative_second_tolerance = 0.;
                        }
                        assert!(
                            matches!(term_coupon_directional_parameter_sensitivity(prepared, curves, key.clone(), position.clone(), snapshot.clone(), invalid), Err(QuoteRiskError::Coupon(TermCouponOptionError::DerivativeResolution { order: actual, .. })) if actual == order)
                        );
                    }
                }
            }
            if constant && position.quantity.decimal() == prepared.face_value() {
                for order in [1, 2] {
                    let mut invalid = settings;
                    if order == 1 {
                        invalid.first = RiskTolerance {
                            absolute: 1e-10,
                            relative: 0.,
                        };
                    } else {
                        invalid.second = RiskTolerance {
                            absolute: 1e-6,
                            relative: 0.,
                        };
                    }
                    assert!(
                        matches!(term_coupon_directional_parameter_sensitivity(prepared, curves, key.clone(), position.clone(), snapshot.clone(), invalid), Err(QuoteRiskError::DirectionalResolution { order: actual, .. }) if actual == order)
                    );
                }
            }
        });
    }
    assert_eq!(keys.len(), 5);
}
