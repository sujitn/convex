//! Actual complete method/side/domain contracts across native and shared JSON.
#[path = "../../../tests/fixtures/library/curve_method_contract.rs"]
mod contract;
use serde_json::{json, Value};

#[test]
fn all_explicit_method_requests_preserve_full_terms_and_expected_failures() {
    for case in contract::cases() {
        let input = contract::request(&case);
        let typed = convex::library::decode_request(&input).unwrap();
        assert_eq!(
            serde_json::to_value(&typed).unwrap(),
            case["request"],
            "{}",
            case["id"]
        );
        contract::check(&case, &convex::library::execute_json(&input));
    }
}

#[test]
fn full_node_grid_boundaries_tail_disclosure_and_discount_origin_survive() {
    let cases = contract::cases();
    let output = |id: &str| -> Value {
        let case = cases.iter().find(|c| c["id"] == id).unwrap();
        serde_json::from_str(&convex::library::execute_json(&contract::request(case))).unwrap()
    };
    let all = output("method.limit1024.FlatForward");
    let curve = &all["result"]["data"]["curves"][0];
    assert_eq!(
        curve["recipe"]["source"]["FlatForward"]["nodes"]
            .as_array()
            .unwrap()
            .len(),
        1024
    );
    assert_eq!(
        curve["interpolation_grid"]["tenors"]
            .as_array()
            .unwrap()
            .len(),
        1024
    );
    assert_eq!(
        output("method.no_origin.tail.Raw.0")["result"]["data"]["extrapolated"],
        true
    );
    assert_eq!(
        output("method.no_origin.forward.Right.0")["result"]["data"]["extrapolated"],
        true
    );
    let identity = output("method.no_origin.tail.DiscountFactor.0");
    assert_eq!(identity["result"]["data"]["value"], 1.0);
    assert_eq!(identity["result"]["data"]["extrapolated"], false);
    assert_eq!(
        output("method.no_origin.forward.Left.1")["result"]["data"]["extrapolated"],
        true
    );
    assert_eq!(
        output("method.exact_tail_boundary.Right")["result"]["data"]["extrapolated"],
        true
    );
    assert_eq!(
        output("method.exact_tail_boundary.Left")["result"]["data"]["extrapolated"],
        false
    );
    let failed = output("method.failure.forward_origin");
    assert!(failed.get("result").is_none());
    let right = output("method.forward.Right.FlatForward.positive.2")["result"]["data"]["value"]
        .as_f64()
        .unwrap();
    let left = output("method.forward.Left.FlatForward.positive.2")["result"]["data"]["value"]
        .as_f64()
        .unwrap();
    assert!((right - left).abs() > 0.003);
}

#[test]
fn missing_sides_nodes_and_approximate_flags_are_rejected_by_strict_schema() {
    let cases = contract::cases();
    let source = &cases
        .iter()
        .find(|c| c["id"] == "method.forward.Left.FlatForward.positive.2")
        .unwrap()["request"];
    let mut missing = source.clone();
    missing["operation"]["query"]
        .as_object_mut()
        .unwrap()
        .remove("side");
    assert!(convex::library::decode_request(&missing.to_string()).is_err());
    for field in [
        "sampled_grid",
        "fallback",
        "default_forward",
        "linear_proxy",
    ] {
        let mut bad = source.clone();
        bad["inputs"]["curves"][0]["source"]["FlatForward"][field] = json!(true);
        assert!(convex::library::decode_request(&bad.to_string()).is_err());
    }
    let mut missing = source.clone();
    missing["inputs"]["curves"][0]["source"]["FlatForward"]
        .as_object_mut()
        .unwrap()
        .remove("nodes");
    assert!(convex::library::decode_request(&missing.to_string()).is_err());
}
