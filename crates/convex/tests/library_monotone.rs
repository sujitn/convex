//! Full supplied contract and strict lossless native/shared ingress.
#[path = "../../../tests/fixtures/library/monotone_contract.rs"]
mod contract;
use serde_json::{json, Value};

#[test]
fn full_monotone_terms_roundtrip_and_exact_failures_survive() {
    for case in contract::cases() {
        let input = contract::request(&case);
        let typed = convex::library::decode_request(&input).unwrap();
        assert_eq!(
            serde_json::to_value(typed).unwrap(),
            case["request"],
            "{}",
            case["id"]
        );
        contract::check(&case, &convex::library::execute_json(&input));
    }
}

#[test]
fn actual_model_origin_and_original_positive_grid_are_disclosed() {
    let cases = contract::cases();
    let output = |id: &str| -> Value {
        serde_json::from_str(&convex::library::execute_json(&contract::request(
            cases.iter().find(|c| c["id"] == id).unwrap(),
        )))
        .unwrap()
    };
    let result = output("monotone.describe.steep");
    let curve = &result["result"]["data"]["curves"][0];
    assert_eq!(
        curve["monotone"]["model"],
        "hagan_west_2006_nonnegative_exact_axis_quadratic"
    );
    assert_eq!(curve["domain"]["minimum_tenor"], 0.0);
    assert_eq!(curve["interpolation_grid"]["tenors"][0], 1.0);
    assert_eq!(
        curve["monotone"]["node_forwards"].as_array().unwrap().len(),
        5
    );
    assert_eq!(
        curve["monotone"]["discrete_forwards"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(curve["recipe"]["key"]["version"], "source.v7");
    assert_eq!(curve["diagnostics"]["iterations"], 0);
    assert!(output("monotone.failure.negative_interval")
        .get("result")
        .is_none());
}

#[test]
fn omitted_terms_extra_model_policies_and_forward_sides_reject_strictly() {
    let cases = contract::cases();
    let source = &cases
        .iter()
        .find(|c| c["id"] == "monotone.describe.steep")
        .unwrap()["request"];
    for field in [
        "interpolation",
        "extrapolation",
        "time_basis",
        "representation",
    ] {
        let mut bad = source.clone();
        bad["inputs"]["curves"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(convex::library::decode_request(&bad.to_string()).is_err());
    }
    let mut bad = source.clone();
    bad["inputs"]["curves"][0]["source"]["MonotoneConvex"]
        .as_object_mut()
        .unwrap()
        .remove("nodes");
    assert!(convex::library::decode_request(&bad.to_string()).is_err());
    for field in [
        "signed_forwards",
        "sampled_derivative",
        "amelioration",
        "provider",
        "origin_rate",
    ] {
        let mut bad = source.clone();
        bad["inputs"]["curves"][0]["source"]["MonotoneConvex"][field] = json!(true);
        assert!(convex::library::decode_request(&bad.to_string()).is_err());
    }
    let mut q = source.clone();
    q["operation"] = json!({"kind":"curve_query","curve":q["inputs"]["curves"][0]["key"],"query":{"kind":"MonotoneConvexForward","point":{"Tenor":1.0}}});
    assert!(convex::library::decode_request(&q.to_string()).is_err());
}
