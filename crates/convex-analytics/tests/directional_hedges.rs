//! Independent supplied-record conformance and full native directional proof.
use convex_analytics::{floating_coupon::*, quote_risk::*};
use convex_bonds::contracts::*;
use convex_core::{contracts::*, fixings::FixingSeries};
use convex_curves::contracts::*;
use serde_json::Value;
use std::cell::Cell;
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/directional-hedges.json")).unwrap()
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
fn records_and_native_pricing_retain_both_derivative_orders_and_original_sides() {
    let f = fixture();
    assert_eq!(f["supplied_records"].as_array().unwrap().len(), 72);
    for row in f["supplied_records"].as_array().unwrap() {
        let supplied: CouponDirectionalSensitivity =
            serde_json::from_value(row["record"].clone()).unwrap();
        supplied.validate().unwrap();
        for (value, field) in [
            (supplied.first, "first"),
            (supplied.half_first, "half_first"),
            (supplied.second, "second"),
            (supplied.half_second, "half_second"),
        ] {
            let bound = number(
                &row[if field.contains("second") {
                    "absolute_scaled_second_bound"
                } else {
                    "absolute_scaled_first_bound"
                }],
            );
            assert!(
                (value - number(&row["expected"][field])).abs() <= bound,
                "{} {field}",
                row["id"]
            );
        }
        with_prepared(row, |prepared, curves, _| {
            let result = term_coupon_directional_parameter_sensitivity(
                prepared,
                curves,
                supplied.key.clone(),
                supplied.position.clone(),
                supplied.snapshot_id.clone(),
                supplied.settings,
            )
            .unwrap();
            let record = result.hedge_record().unwrap();
            record.validate().unwrap();
            assert_eq!(record.key, supplied.key);
            assert_eq!(record.settings, supplied.settings);
            assert_eq!(record.native_parameters, supplied.native_parameters);
            for (actual, expected) in record.native_prices.iter().zip(&supplied.native_prices) {
                assert!((actual - expected).abs() <= 1e-8);
            }
            for (actual, field) in [
                (record.first, "first"),
                (record.half_first, "half_first"),
                (record.second, "second"),
                (record.half_second, "half_second"),
            ] {
                let bound = number(
                    &row[if field.contains("second") {
                        "absolute_scaled_second_bound"
                    } else {
                        "absolute_scaled_first_bound"
                    }],
                );
                assert!(
                    (actual - number(&row["expected"][field])).abs() <= bound,
                    "{} {field}",
                    row["id"]
                );
            }
            assert_eq!(record.base_pv().unwrap(), result.scaled_prices[0]);
            assert_eq!(result.native.price_evaluations, 6);
            for sample in &result.native.prices {
                assert_eq!(
                    serde_json::to_value(&sample.valuation.terms).unwrap(),
                    row["input"]["bond"]["terms"]
                );
            }
        });
    }
}
#[test]
fn zero_holdings_cannot_bypass_native_precision_or_six_sample_conformance() {
    let f = fixture();
    let row = f["supplied_records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["record"]["position"]["quantity"] == "0")
        .unwrap();
    let base: CouponDirectionalSensitivity = serde_json::from_value(row["record"].clone()).unwrap();
    base.validate().unwrap();
    let mut wrong = base.clone();
    wrong.settings.native.derivative.absolute_second_tolerance = 1e-15;
    wrong.settings.native.derivative.relative_second_tolerance = 0.;
    assert!(wrong.validate().is_err());
    let mut wrong = base.clone();
    wrong.native_parameters[1] = wrong.native_parameters[0];
    assert!(wrong.validate().is_err());
    let mut wrong = base.clone();
    wrong.native_prices[1] += 1.;
    assert!(wrong.validate().is_err());
    let mut wrong = base;
    wrong.first = 1.;
    assert!(wrong.validate().is_err());
}
