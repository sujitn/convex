#[path = "../../../tests/fixtures/library/product_ufr.rs"]
mod fixture;
#[path = "../../../tests/fixtures/library/product_parametric.rs"]
mod previous;
use convex::curve_contracts::*;
use convex::library::*;
use serde_json::{json, Value};

fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}
fn case_recipe_mut(case: &mut Value) -> &mut Value {
    if case["inputs"]["calibration"]["rates"]
        .as_array()
        .unwrap()
        .is_empty()
    {
        &mut case["inputs"]["calibration"]["cash"][0]["recipe"]
    } else {
        &mut case["inputs"]["calibration"]["rates"][0]["recipe"]
    }
}

#[test]
fn full_product_ufr_fits_match_independent_quotes_parameters_and_analytic_joins() {
    let tolerance = fixture::controls()["tolerances"].clone();
    for case in fixture::cases() {
        let result = fixture::pure(&case).unwrap_or_else(|e| panic!("{}:{e:?}", case["id"]));
        let original: CurveRecipe =
            serde_json::from_value(fixture::recipe(&case)["curve"].clone()).unwrap();
        let curve = result.curves().get(&original.key).unwrap();
        assert_eq!(curve.recipe(), &original);
        assert!(curve.interpolation_grid().is_err());
        let tail = curve.ufr_diagnostics().unwrap();
        assert_eq!(
            curve.domain().extrapolation,
            convex_curves::ExtrapolationMethod::UfrConvergence {
                ufr: tail.ultimate_forward_rate,
                alpha: tail.convergence_speed
            }
        );
        assert!(
            (tail.ultimate_forward_rate - case["parameters"]["ufr"].as_f64().unwrap()).abs()
                <= tolerance["ufr_absolute"].as_f64().unwrap()
        );
        assert!(
            (tail.convergence_speed - case["parameters"]["alpha"].as_f64().unwrap()).abs()
                <= tolerance["alpha_absolute"].as_f64().unwrap()
        );
        let fit = &result.diagnostics()["calibrations"][0];
        assert_eq!(fit["solver"]["kind"], "ufr");
        assert_eq!(fit["solver"]["diagnostics"]["rank"], 2);
        assert_eq!(fit["repriced_helpers"].as_array().unwrap().len(), 2);
        assert_eq!(
            fit["solver"]["diagnostics"]["native_residuals"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        for quote in &curve.diagnostics().quotes {
            assert_eq!(quote.input.unit, quote.model.unit);
            assert!(quote.residual.abs() <= quote.tolerance);
        }
        for control in case["controls"].as_array().unwrap() {
            let t = number(&control["tenor"]);
            let point = CurvePoint::Tenor(t);
            for (query, expected, bound) in [
                (CurveQuery::Raw { point }, "zero", "zero_absolute"),
                (
                    CurveQuery::DiscountFactor { point },
                    "discount_factor",
                    "discount_absolute",
                ),
                (
                    CurveQuery::UfrJoinedForward {
                        point,
                        side: ForwardSide::Left,
                    },
                    "forward_left",
                    "forward_absolute",
                ),
                (
                    CurveQuery::UfrJoinedForward {
                        point,
                        side: ForwardSide::Right,
                    },
                    "forward_right",
                    "forward_absolute",
                ),
            ] {
                let actual = curve.query(&query).unwrap();
                assert!(
                    (actual.value - number(&control[expected])).abs()
                        <= tolerance[bound].as_f64().unwrap(),
                    "{}:{t}:{expected} {actual:?}",
                    case["id"]
                );
            }
        }
        let point = tail.last_liquid_point;
        let left = curve
            .query(&CurveQuery::UfrJoinedForward {
                point,
                side: ForwardSide::Left,
            })
            .unwrap();
        let right = curve
            .query(&CurveQuery::UfrJoinedForward {
                point,
                side: ForwardSide::Right,
            })
            .unwrap();
        assert_eq!(left.value, right.value);
        assert!(!left.extrapolated && right.extrapolated);
        let base = result.curves().get(&tail.base).unwrap();
        assert_eq!(
            curve
                .query(&CurveQuery::DiscountFactor { point })
                .unwrap()
                .value,
            base.query(&CurveQuery::DiscountFactor { point })
                .unwrap()
                .value
        );
        if case["id"].as_str().unwrap().starts_with("flat_forward") {
            assert!((right.value - number(&case["original_base_right_at_llp"])).abs() > 0.001);
        }
        let wire = fixture::request(&case, json!({"kind":"build"}));
        fixture::check_transport(&case, &execute_json(&wire));
        assert_eq!(
            execute(&decode_request(&wire).unwrap()).unwrap()["data"]["calibrations"],
            result.diagnostics()["calibrations"]
        );
    }
}

#[test]
fn invalid_full_product_ufr_inputs_fail_atomically_with_original_work_and_domain_guards() {
    let original = fixture::cases().remove(0);
    let good = fixture::pure(&original).unwrap();
    let prefix = "/inputs/calibration/rates/0/recipe/curve";
    for (suffix, value) in [
        ("/calibration/Ufr/initial/alpha", json!(0.0)),
        ("/calibration/Ufr/lower/alpha", json!(0.0)),
        ("/calibration/Ufr/upper/ufr", json!(11.0)),
        ("/calibration/Ufr/lower/ufr", json!(0.05)),
        ("/calibration/Ufr/parameter_scales", json!([0.0, 0.1])),
        ("/calibration/Ufr/quote_scales", json!([100.0])),
        ("/calibration/Ufr/weights", json!([1.0, 0.0])),
        ("/calibration/Ufr/max_iterations", json!(257)),
        ("/calibration/Ufr/objective_tolerance", json!(0.0)),
        ("/calibration/Ufr/last_liquid_point", json!({"Tenor":0.0})),
        ("/calibration/Ufr/last_liquid_point", json!({"Tenor":20.0})),
        ("/calibration/Ufr/base/version", json!("absent")),
        ("/extrapolation/UfrConvergence/alpha", json!(0.2)),
        ("/dependencies", json!([])),
        ("/interpolation", json!("Linear")),
        ("/time_basis", json!("Act360")),
    ] {
        let mut bad = original.clone();
        *bad.pointer_mut(&(prefix.to_owned() + suffix)).unwrap() = value;
        assert!(
            fixture::pure(&bad).is_err(),
            "unexpected acceptance:{suffix}"
        );
    }
    let mut bad = original.clone();
    case_recipe_mut(&mut bad)["curve"]["calibration"]["Ufr"]["max_iterations"] = json!(1);
    let error = fixture::pure(&bad).unwrap_err();
    assert_eq!(error.code, LibraryErrorCode::NoConvergence);
    assert_eq!(error.details["solver"]["iterations"], 1);
    let mut bad = original.clone();
    bad["inputs"]["curves"][0]["source"] = json!({"PiecewiseConstant":{"nodes":bad["inputs"]["curves"][0]["source"]["FlatForward"]["nodes"]}});
    bad["inputs"]["curves"][0]["interpolation"] = json!("PiecewiseConstant");
    case_recipe_mut(&mut bad)["curve"]["interpolation"] = json!("PiecewiseConstant");
    assert_eq!(
        fixture::pure(&bad).unwrap_err().code,
        LibraryErrorCode::Unsupported
    );
    let key: CurveKey =
        serde_json::from_value(fixture::recipe(&original)["curve"]["key"].clone()).unwrap();
    let curve = good.curves().get(&key).unwrap();
    assert!(curve
        .query(&CurveQuery::UfrForward {
            point: CurvePoint::Tenor(1.0)
        })
        .is_err());
    assert!(curve
        .query(&CurveQuery::UfrJoinedForward {
            point: CurvePoint::Tenor(0.0),
            side: ForwardSide::Left
        })
        .is_err());
    assert!(curve
        .query(&CurveQuery::UfrJoinedForward {
            point: CurvePoint::Tenor(151.0),
            side: ForwardSide::Right
        })
        .is_err());
    assert_eq!(
        good.diagnostics(),
        fixture::pure(&original).unwrap().diagnostics()
    );
}

#[test]
fn underidentified_ufr_speed_and_zero_weight_original_quotes_cannot_be_accepted() {
    let original = fixture::cases().remove(0);
    let mut bad = original.clone();
    let r = case_recipe_mut(&mut bad);
    let id = r["helpers"][1]["id"].clone();
    r["helpers"][1] = r["helpers"][0].clone();
    r["helpers"][1]["id"] = id;
    r["overlap_policy"] = json!("RetainAllGlobal");
    let error = fixture::pure(&bad).unwrap_err();
    assert_eq!(error.code, LibraryErrorCode::NonUnique);
    assert_eq!(error.details["solver"]["rank"], 1);
    let good = fixture::pure(&original).unwrap();
    let key: CurveKey =
        serde_json::from_value(fixture::recipe(&original)["curve"]["key"].clone()).unwrap();
    let forward = good
        .curves()
        .get(&key)
        .unwrap()
        .ufr_diagnostics()
        .unwrap()
        .boundary_forward;
    let mut bad = original.clone();
    let r = case_recipe_mut(&mut bad);
    r["curve"]["calibration"]["Ufr"]["initial"]["ufr"] = json!(forward);
    r["curve"]["extrapolation"]["UfrConvergence"]["ufr"] = json!(forward);
    assert_eq!(
        fixture::pure(&bad).unwrap_err().code,
        LibraryErrorCode::NonUnique
    );
    let mut bad = original.clone();
    let rate = &mut bad["inputs"]["calibration"]["rates"][0];
    let mut extra = rate["recipe"]["helpers"][0].clone();
    extra["id"] = json!("retained-zero-weight-quote");
    extra["quote"]["value"] = json!(extra["quote"]["value"].as_f64().unwrap() + 0.1);
    rate["recipe"]["helpers"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    rate["recipe"]["curve"]["source"]["Helpers"]
        .as_array_mut()
        .unwrap()
        .push(json!("retained-zero-weight-quote"));
    rate["recipe"]["curve"]["calibration"]["Ufr"]["quote_scales"]
        .as_array_mut()
        .unwrap()
        .push(json!(100.0));
    rate["recipe"]["curve"]["calibration"]["Ufr"]["weights"]
        .as_array_mut()
        .unwrap()
        .push(json!(0.0));
    rate["recipe"]["overlap_policy"] = json!("RetainAllGlobal");
    rate["quote_versions"]
        .as_array_mut()
        .unwrap()
        .push(json!(13));
    rate["futures_calendar_stamps"]
        .as_array_mut()
        .unwrap()
        .push(Value::Null);
    assert_eq!(
        fixture::pure(&bad).unwrap_err().code,
        LibraryErrorCode::NoConvergence
    );
}

#[test]
fn original_quote_risk_rebuilds_tail_and_preserves_signed_quantity_and_fixed_base() {
    let case = fixture::cases().remove(0);
    let original = fixture::pure(&case).unwrap();
    let action = fixture::risk_action(&case);
    let result =
        execute(&decode_request(&fixture::request(&case, action.clone())).unwrap()).unwrap();
    let risk: convex::quote_risk::QuoteSensitivity =
        serde_json::from_value(result["data"].clone()).unwrap();
    risk.validate().unwrap();
    assert_ne!(risk.plus_pv, risk.minus_pv);
    let mut short = action;
    short["position"]["quantity"] = json!("-5000");
    let signed = execute(&decode_request(&fixture::request(&case, short)).unwrap()).unwrap();
    for name in [
        "base_pv",
        "plus_pv",
        "minus_pv",
        "half_plus_pv",
        "half_minus_pv",
        "value",
        "half_value",
    ] {
        assert_eq!(
            signed["data"][name].as_f64().unwrap(),
            -result["data"][name].as_f64().unwrap()
        );
    }
    assert_eq!(
        original.diagnostics(),
        fixture::pure(&case).unwrap().diagnostics()
    );
}

#[test]
fn ufr_joins_actual_prior_product_parametric_owner_and_rebuilds_both_quote_dependencies() {
    let mut case = fixture::cases()
        .into_iter()
        .find(|c| c["id"] == "nelson_siegel.rate.positive")
        .unwrap();
    let mut base = previous::cases()
        .into_iter()
        .find(|c| c["id"] == "nelson_siegel.rate.positive")
        .unwrap();
    let base_recipe = &mut base["inputs"]["calibration"]["rates"][0]["recipe"];
    base_recipe["curve"]["key"]["id"] = json!("fictional.calibrated.NS.base");
    for helper in base_recipe["helpers"].as_array_mut().unwrap() {
        helper["terms"]["curve"]["id"] = json!("fictional.calibrated.NS.base");
    }
    let key = base["inputs"]["calibration"]["rates"][0]["recipe"]["curve"]["key"].clone();
    let r = case_recipe_mut(&mut case);
    r["curve"]["calibration"]["Ufr"]["base"] = key.clone();
    r["curve"]["dependencies"] = json!([key]);
    case["inputs"]["curves"] = json!([]);
    case["inputs"]["calibration"]["rates"]
        .as_array_mut()
        .unwrap()
        .push(base["inputs"]["calibration"]["rates"][0].clone());
    case["inputs"]["calendars"]
        .as_array_mut()
        .unwrap()
        .push(base["inputs"]["calendars"][0].clone());
    let result = fixture::pure(&case).unwrap();
    assert_eq!(
        result.diagnostics()["calibrations"][0]["solver"]["kind"],
        "parametric"
    );
    assert_eq!(
        result.diagnostics()["calibrations"][1]["solver"]["kind"],
        "ufr"
    );
    let mut action = fixture::risk_action(&case);
    let base_rate = &base["inputs"]["calibration"]["rates"][0];
    action["key"]["coordinate"]["curve"] = key.clone();
    action["key"]["coordinate"]["helper_id"] = base_rate["recipe"]["helpers"][0]["id"].clone();
    action["key"]["coordinate"]["quote_version"] = base_rate["quote_versions"][0].clone();
    action["key"]["factor"]["target"]["curve_id"] = key["id"].clone();
    action["key"]["native_unit"] = base_rate["recipe"]["helpers"][0]["quote"]["unit"].clone();
    let coordinate: convex::quote_risk::QuoteRiskKey =
        serde_json::from_value(action["key"].clone()).unwrap();
    // The original larger dislocation is outside this bounded tail family's
    // admissible two-quote solution. Failure must preserve the complete baseline.
    assert_eq!(
        result
            .evaluate(&CalibrationOperation::Scenario {
                key: coordinate.clone(),
                delta: 0.0001
            })
            .unwrap_err()
            .code,
        LibraryErrorCode::NoConvergence
    );
    let rebuilt = result
        .evaluate(&CalibrationOperation::Scenario {
            key: coordinate,
            delta: 0.000001,
        })
        .unwrap();
    assert_eq!(rebuilt["calibrations"][1]["solver"]["kind"], "ufr");
    assert_ne!(
        rebuilt["calibrations"][0]["solver"]["parameters"],
        result.diagnostics()["calibrations"][0]["solver"]["parameters"]
    );
    assert_ne!(
        rebuilt["calibrations"][1]["solver"]["parameters"],
        result.diagnostics()["calibrations"][1]["solver"]["parameters"]
    );
    assert_eq!(
        result.diagnostics(),
        fixture::pure(&case).unwrap().diagnostics()
    );
}
