//! Full native model scenarios versus independently declared contract values.
use convex_analytics::floating_coupon::*;
use convex_bonds::contracts::*;
use convex_core::{contracts::UtcTimestamp, fixings::FixingSeries};
use convex_curves::contracts::*;
use serde_json::Value;
use std::cell::Cell;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/coupon-curve-state.json")).unwrap()
}

#[test]
fn all_original_curve_families_preserve_full_coupon_model_scenarios() {
    let f = fixture();
    for row in f["cases"].as_array().unwrap() {
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
            |_, date| {
                calls.set(calls.get() + 1);
                Ok(UtcTimestamp::parse(&format!("{date}T16:00:00Z")).unwrap())
            },
        )
        .unwrap();
        assert_eq!(calls.get(), 3);
        let original = serde_json::to_value(prepared.baseline()).unwrap();
        let rebuilt = build_curve_set(&recipes, &CurveSet::new()).unwrap().curves;
        prepared.admit_baseline(&rebuilt).unwrap();
        for key in curves.keys() {
            assert!(curves
                .get(key)
                .unwrap()
                .same_valuation_state(rebuilt.get(key).unwrap())
                .unwrap());
        }
        let mut altered = recipes.clone();
        match &mut altered[0].source {
            CurveSource::Nodes(nodes)
            | CurveSource::PiecewiseConstant { nodes }
            | CurveSource::FlatForward { nodes }
            | CurveSource::MonotoneConvex { nodes } => {
                for node in nodes {
                    node.quote.value += 0.001;
                }
            }
            CurveSource::Parametric { parameters, .. } => match parameters {
                ParametricModel::NelsonSiegel { beta0, .. }
                | ParametricModel::Svensson { beta0, .. } => *beta0 += 0.001,
            },
            _ => panic!("explicit original leaf"),
        }
        let changed = build_curve_set(&altered, &CurveSet::new()).unwrap();
        assert!(prepared.admit_baseline(&changed.curves).is_err());

        let coordinate: CouponParameterCoordinate =
            serde_json::from_value(row["coordinate"].clone()).unwrap();
        let delta = row["delta"].as_f64().unwrap();
        let result = prepared
            .model_scenario(&curves, &coordinate, delta)
            .unwrap();
        for (field, actual) in [
            ("base_pv", result.base.present_value),
            ("scenario_pv", result.scenario.present_value),
        ] {
            assert!(
                (actual - row["expected"][field].as_f64().unwrap()).abs()
                    <= f["absolute_pv_bound"].as_f64().unwrap(),
                "{} {field}: {actual}",
                row["id"]
            );
        }
        assert_eq!(
            result.original_parameter,
            row["expected"]["original_parameter"].as_f64().unwrap()
        );
        assert!(
            (result.scenario_parameter - row["expected"]["scenario_parameter"].as_f64().unwrap())
                .abs()
                <= 1e-15
        );
        assert_eq!(result.coordinate, coordinate);
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["base"], original);
        for key in ["terms", "context", "discount_curve"] {
            assert_eq!(value["base"][key], value["scenario"][key]);
        }
        for (base, shocked) in value["base"]["segments"]
            .as_array()
            .unwrap()
            .iter()
            .zip(value["scenario"]["segments"].as_array().unwrap())
        {
            for key in [
                "start",
                "end",
                "payment_date",
                "request",
                "rate",
                "principal",
                "year_fraction",
                "entitled",
            ] {
                assert_eq!(base[key], shocked[key], "{} {key}", row["id"]);
            }
        }
        if delta == 0.0 {
            assert_eq!(value["base"], value["scenario"]);
        }
        assert_eq!(
            serde_json::to_value(prepared.price(&curves).unwrap()).unwrap(),
            original
        );
        assert_eq!(serde_json::to_value(&options).unwrap(), original["options"]);
        assert_eq!(calls.get(), 3);
        for delta in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(prepared
                .model_scenario(&curves, &coordinate, delta)
                .is_err());
        }
        let mut wrong = coordinate.clone();
        wrong.source_id = convex_core::contracts::SourceId::new("different-source").unwrap();
        assert!(prepared.model_scenario(&curves, &wrong, 0.01).is_err());
        wrong = coordinate.clone();
        wrong.source_version = convex_core::contracts::Version::new(2).unwrap();
        assert!(prepared.model_scenario(&curves, &wrong, 0.01).is_err());
        wrong = coordinate.clone();
        wrong.observation_date = convex_core::Date::parse("2024-01-01").unwrap();
        assert!(matches!(
            prepared.model_scenario(&curves, &wrong, 0.01),
            Err(TermCouponOptionError::MissingModel { .. })
        ));
        wrong = coordinate.clone();
        wrong.parameter = match coordinate.parameter {
            CouponModelParameter::NormalFactorLoading { .. } => {
                CouponModelParameter::NormalFactorLoading { factor: usize::MAX }
            }
            CouponModelParameter::NormalVolatility => CouponModelParameter::LognormalVolatility,
            _ => CouponModelParameter::NormalVolatility,
        };
        assert!(prepared.model_scenario(&curves, &wrong, 0.01).is_err());
        if matches!(
            coordinate.parameter,
            CouponModelParameter::NormalVolatility | CouponModelParameter::LognormalVolatility
        ) {
            assert!(prepared
                .model_scenario(&curves, &coordinate, -result.original_parameter - 0.01)
                .is_err());
        }
        assert_eq!(serde_json::to_value(prepared.baseline()).unwrap(), original);
    }
}
