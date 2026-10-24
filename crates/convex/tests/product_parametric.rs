#[path = "../../../tests/fixtures/library/product_parametric.rs"]
mod fixture;
use convex::curve_contracts::*;
use convex::library::*;
use convex_core::Date;
use serde_json::{json, Value};

#[test]
fn full_product_ns_and_svensson_fits_match_independent_dated_controls() {
    let tolerances = fixture::controls()["tolerances"].clone();
    for case in fixture::cases() {
        let built = fixture::pure(&case).unwrap_or_else(|e| panic!("{}: {e:?}", case["id"]));
        let recipe = fixture::recipe(&case);
        let key: CurveKey = serde_json::from_value(recipe["curve"]["key"].clone()).unwrap();
        let curve = built.curves().get(&key).unwrap();
        assert!(curve.interpolation_grid().is_err());
        let original: CurveRecipe = serde_json::from_value(recipe["curve"].clone()).unwrap();
        assert_eq!(curve.recipe(), &original);
        let solved = serde_json::to_value(
            curve
                .structured_diagnostics()
                .unwrap()
                .parameters
                .as_ref()
                .unwrap(),
        )
        .unwrap();
        for (name, value) in case["model"].as_object().unwrap() {
            if name == "kind" {
                assert_eq!(&solved[name], value);
                continue;
            }
            let bound = if name.starts_with("tau") {
                "decay_absolute"
            } else {
                "beta_absolute"
            };
            assert!(
                (solved[name].as_f64().unwrap() - value.as_f64().unwrap()).abs()
                    <= tolerances[bound].as_f64().unwrap(),
                "{}:{name} {solved}",
                case["id"]
            );
        }
        let fit = &built.diagnostics()["calibrations"][0];
        let count = recipe["helpers"].as_array().unwrap().len();
        assert_eq!(fit["repriced_helpers"].as_array().unwrap().len(), count);
        assert_eq!(fit["solver"]["kind"], "parametric");
        assert_eq!(
            fit["solver"]["diagnostics"]["rank"].as_u64().unwrap() as usize,
            count
        );
        assert_eq!(
            fit["solver"]["diagnostics"]["native_residuals"]
                .as_array()
                .unwrap()
                .len(),
            count
        );
        for control in case["expected"].as_array().unwrap() {
            let point = CurvePoint::Date(Date::parse(control["date"].as_str().unwrap()).unwrap());
            let actual = curve
                .query(&CurveQuery::DiscountFactor { point })
                .unwrap()
                .value;
            assert!(
                (actual
                    - control["discount_factor"]
                        .as_str()
                        .unwrap()
                        .parse::<f64>()
                        .unwrap())
                .abs()
                    <= tolerances["discount_absolute"].as_f64().unwrap()
            );
        }
        let wire = fixture::request(&case, json!({"kind":"build"}));
        let native = execute(&decode_request(&wire).unwrap()).unwrap();
        assert_eq!(
            native["data"]["calibrations"],
            built.diagnostics()["calibrations"]
        );
        fixture::check_transport(&case, &execute_json(&wire));
    }
}
#[test]
fn invalid_bounds_terms_units_horizons_methods_and_work_fail_without_mutation() {
    let original = fixture::cases().remove(0);
    let saved = fixture::pure(&original).unwrap();
    let path = "/inputs/calibration/rates/0/recipe";
    let mut edits = Vec::new();
    for (suffix, replacement) in [
        ("/curve/calibration/Parametric/lower", json!([-0.2])),
        ("/curve/calibration/Parametric/initial/tau", json!(0)),
        ("/curve/calibration/Parametric/quote_scales", json!([1])),
        ("/curve/calibration/Parametric/parameter_scales", json!([1])),
        (
            "/curve/calibration/Parametric/parameter_scales",
            json!([1, 1, 1, 0]),
        ),
        ("/curve/calibration/Parametric/weights", json!([1, 1, 1, 0])),
        ("/curve/interpolation", json!("Linear")),
        ("/curve/extrapolation", json!("FlatForward")),
        (
            "/bootstrap",
            json!({"lower_zero":-0.1,"upper_zero":0.1,"max_iterations":100}),
        ),
        ("/helpers/0/quote/unit", json!("PricePer100")),
        ("/helpers/0/terms/calendar/version", json!(8)),
        ("/pillars", json!(["2027-04-07"])),
    ] {
        let mut bad = original.clone();
        *bad.pointer_mut(&(path.to_owned() + suffix)).unwrap() = replacement;
        edits.push(bad);
    }
    let mut exhausted = original.clone();
    exhausted["inputs"]["calibration"]["rates"][0]["recipe"]["curve"]["calibration"]
        ["Parametric"]["max_iterations"] = json!(1);
    let error = fixture::pure(&exhausted).unwrap_err();
    assert_eq!(error.code, LibraryErrorCode::NoConvergence);
    assert_eq!(error.details["solver"]["iterations"], 1);
    for bad in edits {
        assert!(fixture::pure(&bad).is_err(), "unexpected acceptance: {bad}");
    }
    assert_eq!(
        saved.diagnostics(),
        fixture::pure(&original).unwrap().diagnostics()
    );
    let mut unknown = original.clone();
    unknown["inputs"]["calibration"]["rates"][0]["recipe"]["curve"]["calibration"]["Parametric"]
        ["zeros"] = json!([0.03]);
    assert!(decode_calibration_inputs(&unknown["inputs"]).is_err());
}
#[test]
fn undamped_identification_and_zero_weight_native_residuals_are_mandatory() {
    let original = fixture::cases().remove(0);
    let mut bad = original.clone();
    let r = &mut bad["inputs"]["calibration"]["rates"][0]["recipe"];
    let first = r["helpers"][0].clone();
    for i in 1..4 {
        let id = r["helpers"][i]["id"].clone();
        r["helpers"][i] = first.clone();
        r["helpers"][i]["id"] = id;
    }
    r["overlap_policy"] = json!("RetainAllGlobal");
    let error = fixture::pure(&bad).unwrap_err();
    assert_eq!(error.code, LibraryErrorCode::NonUnique);
    assert!(error.details["solver"]["rank"].as_u64().unwrap() < 4);
    let mut inconsistent = original.clone();
    let rate = &mut inconsistent["inputs"]["calibration"]["rates"][0];
    let mut extra = rate["recipe"]["helpers"][0].clone();
    extra["id"] = json!("zero-weight-original-quote");
    extra["quote"]["value"] = json!(extra["quote"]["value"].as_f64().unwrap() + 0.001);
    rate["recipe"]["helpers"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    rate["recipe"]["curve"]["source"]["Helpers"]
        .as_array_mut()
        .unwrap()
        .push(json!("zero-weight-original-quote"));
    let s = &mut rate["recipe"]["curve"]["calibration"]["Parametric"];
    s["quote_scales"].as_array_mut().unwrap().push(json!(1));
    s["weights"].as_array_mut().unwrap().push(json!(0));
    rate["recipe"]["overlap_policy"] = json!("RetainAllGlobal");
    rate["quote_versions"]
        .as_array_mut()
        .unwrap()
        .push(json!(15));
    rate["futures_calendar_stamps"]
        .as_array_mut()
        .unwrap()
        .push(Value::Null);
    assert_eq!(
        fixture::pure(&inconsistent).unwrap_err().code,
        LibraryErrorCode::NoConvergence
    );
}
#[test]
fn original_product_quote_risk_rebuilds_exact_parametric_models() {
    let case = fixture::cases().remove(0);
    let wire = fixture::request(&case, fixture::risk_action(&case));
    let result = execute(&decode_request(&wire).unwrap()).unwrap();
    let sensitivity: convex::quote_risk::QuoteSensitivity =
        serde_json::from_value(result["data"].clone()).unwrap();
    sensitivity.validate().unwrap();
    assert_ne!(sensitivity.plus_pv, sensitivity.minus_pv);
    let mut short = fixture::risk_action(&case);
    short["position"]["quantity"] = json!("-5000");
    let short = execute(&decode_request(&fixture::request(&case, short)).unwrap()).unwrap();
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
            short["data"][name].as_f64().unwrap(),
            -result["data"][name].as_f64().unwrap()
        );
    }
    let before = fixture::pure(&case).unwrap();
    let mut action = fixture::risk_action(&case);
    action["kind"] = json!("scenario");
    action["delta"] = json!(0.0001);
    for key in ["product", "position", "snapshot_id", "settings"] {
        action.as_object_mut().unwrap().remove(key);
    }
    let scenario = before
        .evaluate(&serde_json::from_value(action).unwrap())
        .unwrap();
    assert_eq!(scenario["calibrations"][0]["solver"]["kind"], "parametric");
    assert_ne!(
        scenario["calibrations"][0]["solver"]["parameters"],
        before.diagnostics()["calibrations"][0]["solver"]["parameters"]
    );
    assert_eq!(
        before.diagnostics(),
        fixture::pure(&case).unwrap().diagnostics()
    );
}

#[test]
fn retained_poor_local_start_hits_original_work_limit_and_preserves_last_good() {
    let raw: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/curves/product-parametric-conditioned.json"
    ))
    .unwrap();
    let failed = raw["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "svensson.government.positive")
        .unwrap();
    let admitted = fixture::cases()
        .into_iter()
        .find(|case| case["id"] == failed["id"])
        .unwrap();
    let last_good = fixture::pure(&admitted).unwrap();
    let error = fixture::pure(failed).unwrap_err();
    assert_eq!(error.code, LibraryErrorCode::ResourceLimit);
    assert_eq!(error.details["solver"]["rank"], 6);
    assert!(error.details["solver"]["evaluations"].as_u64().unwrap() > 2000);
    assert_eq!(
        last_good.diagnostics(),
        fixture::pure(&admitted).unwrap().diagnostics()
    );
}
