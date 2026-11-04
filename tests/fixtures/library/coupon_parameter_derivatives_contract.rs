//! Independent full-contract parameter derivatives precede transport expectations.
#![allow(dead_code)]
#[path = "contract.rs"]
mod original;
use serde_json::{json, Value};
pub fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("coupon-parameter-derivatives.json")).unwrap()
        ["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn request(case: &Value) -> String {
    original::request(case)
}
pub fn check(case: &Value, output: &str) {
    original::check(case, output);
    if !case["expected_error"].is_null() {
        if let Some(order) = case["expected_resolution_order"].as_u64() {
            let error: Value = serde_json::from_str(output).unwrap();
            assert!(error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("arithmetic resolution"));
            assert!(error["error"]["details"]["native_kind"]
                .as_str()
                .unwrap()
                .contains(&format!("DerivativeResolution {{ order: {order},")));
        }
        return;
    }
    let envelope: Value = serde_json::from_str(output).unwrap();
    let data = &envelope["result"]["data"];
    assert_eq!(
        data["coordinate"],
        case["request"]["operation"]["coordinate"]
    );
    let settings: convex::library::LibraryRequest =
        convex::library::decode_request(&request(case)).unwrap();
    let convex::library::LibraryOperation::TermCouponParameterDerivatives { settings, .. } =
        settings.operation
    else {
        panic!("actual parameter derivative operation");
    };
    assert_eq!(data["settings"], serde_json::to_value(settings).unwrap());
    let original_input: convex::library::TermCouponOptionInput =
        serde_json::from_value(case["request"]["operation"]["input"].clone()).unwrap();
    let options = serde_json::to_value(original_input.options).unwrap();
    let original_parameter = data["original_parameter"].as_f64().unwrap();
    for name in ["base", "plus", "minus", "half_plus", "half_minus"] {
        let actual = data[name]["present_value"].as_f64().unwrap();
        let expected = case["expected"][name]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        assert!(
            (actual - expected).abs() <= case["bounds"]["absolute_pv_bound"].as_f64().unwrap(),
            "{} {name}: {actual} != {expected}",
            case["id"]
        );
        assert_eq!(
            data[name]["terms"],
            case["request"]["operation"]["input"]["bond"]["terms"]
        );
        for field in ["context", "discount_curve"] {
            assert_eq!(data[name][field], data["base"][field]);
        }
        for (a, b) in data[name]["segments"]
            .as_array()
            .unwrap()
            .iter()
            .zip(data["base"]["segments"].as_array().unwrap())
        {
            for field in ["request", "rate", "principal", "year_fraction", "entitled"] {
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
        let actual = data[name].as_f64().unwrap();
        let expected = case["expected"][name]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        assert!(
            (actual - expected).abs() <= case["bounds"][bound].as_f64().unwrap(),
            "{} {name}: {actual} != {expected}",
            case["id"]
        );
    }
    let h = settings.bump;
    assert!(data["first_resolution"].as_f64().unwrap() > 0.0);
    assert!(data["second_resolution"].as_f64().unwrap() > 0.0);
    for (name, delta) in [
        ("base", 0.0),
        ("plus", h),
        ("minus", -h),
        ("half_plus", h / 2.0),
        ("half_minus", -h / 2.0),
    ] {
        let mut expected = options.clone();
        let coordinate = &data["coordinate"];
        let whole = coordinate["parameter"]["kind"] == "normal_factor_loading";
        let rows = if whole {
            &mut expected["whole_coupon"]["factors"]
        } else {
            &mut expected["models"]
        };
        let point = rows
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|p| {
                p["index_id"] == coordinate["index_id"]
                    && p["observation_date"] == coordinate["observation_date"]
                    && p["payment_date"] == coordinate["payment_date"]
            })
            .unwrap();
        assert_eq!(point["source_id"], coordinate["source_id"]);
        assert_eq!(point["version"], coordinate["source_version"]);
        let target = if whole {
            &mut point["loadings"][coordinate["parameter"]["factor"].as_u64().unwrap() as usize]
        } else if coordinate["parameter"]["kind"] == "lognormal_displacement" {
            &mut point["model"]["displacement"]
        } else {
            &mut point["model"]["volatility"]
        };
        assert_eq!(options_pointer_value(case, whole), original_parameter);
        *target = json!(original_parameter + delta);
        assert_eq!(data[name]["options"], expected);
    }
    let pv = |name: &str| data[name]["present_value"].as_f64().unwrap();
    assert_eq!(
        data["first"].as_f64().unwrap(),
        (pv("plus") - pv("minus")) / (2.0 * h)
    );
    assert_eq!(
        data["half_first"].as_f64().unwrap(),
        (pv("half_plus") - pv("half_minus")) / h
    );
    assert_eq!(
        data["second"].as_f64().unwrap(),
        (pv("plus") - 2.0 * pv("base") + pv("minus")) / (h * h)
    );
    assert_eq!(
        data["half_second"].as_f64().unwrap(),
        4.0 * (pv("half_plus") - 2.0 * pv("base") + pv("half_minus")) / (h * h)
    );
    if case["id"] == "coupon_derivatives.constant_collar_zero" {
        for name in ["plus", "minus", "half_plus", "half_minus"] {
            assert_eq!(pv(name), pv("base"));
        }
        for name in ["first", "half_first", "second", "half_second"] {
            assert_eq!(data[name], 0.0);
        }
    }
}

fn options_pointer_value(case: &Value, whole: bool) -> f64 {
    let coordinate = &case["request"]["operation"]["coordinate"];
    let options = &case["request"]["operation"]["input"]["options"];
    let rows = if whole {
        &options["whole_coupon"]["factors"]
    } else {
        &options["models"]
    };
    let point = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|p| {
            p["index_id"] == coordinate["index_id"]
                && p["observation_date"] == coordinate["observation_date"]
                && p["payment_date"] == coordinate["payment_date"]
        })
        .unwrap();
    if whole {
        point["loadings"][coordinate["parameter"]["factor"].as_u64().unwrap() as usize]
            .as_f64()
            .unwrap()
    } else if coordinate["parameter"]["kind"] == "lognormal_displacement" {
        point["model"]["displacement"].as_f64().unwrap()
    } else {
        point["model"]["volatility"].as_f64().unwrap()
    }
}
pub fn run() -> Value {
    let rows: Vec<_> = cases().into_iter().map(|case| {
        let output = convex::library::execute_json(&request(&case));check(&case,&output);
        json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":serde_json::from_str::<Value>(&output).unwrap()})
    }).collect();
    json!({"schema_version":1,"status":"passed","cases":rows,"certified":false})
}
