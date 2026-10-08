//! Actual complete native/shared construction, methods, residuals and typed failures.
#[path = "../../../tests/fixtures/library/curve_family_contract.rs"]
mod contract;
use serde_json::{json, Value};
#[test]
fn every_full_curve_family_request_and_failure_roundtrips() {
    for c in contract::cases() {
        let input = contract::request(&c);
        let typed = convex::library::decode_request(&input).unwrap();
        assert_eq!(
            serde_json::to_value(&typed).unwrap(),
            c["request"],
            "{}",
            c["id"]
        );
        contract::check(&c, &convex::library::execute_json(&input));
    }
}
#[test]
fn full_sources_tail_disclosure_observation_rank_and_no_grid_alias_survive_dispatch() {
    let cases = contract::cases();
    let output = |id: &str| -> Value {
        let c = cases.iter().find(|c| c["id"] == id).unwrap();
        serde_json::from_str(&convex::library::execute_json(&contract::request(c))).unwrap()
    };
    let result = output("curve.segment.inner_tail");
    assert_eq!(result["result"]["data"]["extrapolated"], true);
    assert_eq!(
        output("curve.delegate.Clamp.0")["result"]["data"]["extrapolated"],
        true
    );
    let mut origin = cases
        .iter()
        .find(|c| c["id"] == "curve.delegate.Clamp.0")
        .unwrap()["request"]
        .clone();
    origin["operation"]["query"]["kind"] = json!("DiscountFactor");
    let identity: Value =
        serde_json::from_str(&convex::library::execute_json(&origin.to_string())).unwrap();
    assert_eq!(identity["result"]["data"]["value"], 1.0);
    assert_eq!(identity["result"]["data"]["extrapolated"], false);
    let result = output("curve.segment.describe");
    let curves = result["result"]["data"]["curves"].as_array().unwrap();
    let segment = curves
        .iter()
        .find(|c| c["recipe"]["key"]["id"] == "segments")
        .unwrap();
    assert_eq!(segment["interpolation_grid"]["status"], "unsupported");
    assert_eq!(segment["structure"]["joins"].as_array().unwrap().len(), 1);
    assert_eq!(
        segment["structure"]["source_domains"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let result = output("curve.model.fit.sv.positive");
    let d = &result["result"]["data"];
    assert_eq!(d["solver"]["rank"], 6);
    assert_eq!(
        d["curve"]["structure"]["observations"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    assert_ne!(
        d["curve"]["recipe"]["source"]["Parametric"]["parameters"],
        d["input_recipe"]["source"]["Parametric"]["parameters"]
    );
    let failed = output("curve.failure.fit_unidentified");
    assert!(failed.get("result").is_none());
    assert_eq!(failed["error"]["code"], "non_unique");
    assert!(
        failed["error"]["details"]["solver"]["rank"]
            .as_u64()
            .unwrap()
            < 4
    );
    let result = output("curve.segment.max128");
    let curves = result["result"]["data"]["curves"].as_array().unwrap();
    let c = curves
        .iter()
        .find(|c| c["recipe"]["key"]["id"] == "segments")
        .unwrap();
    assert_eq!(c["structure"]["joins"].as_array().unwrap().len(), 127);
}
#[test]
fn sampled_and_missing_parametric_segment_policy_fields_fail_strict_ingress() {
    let c = contract::cases()
        .into_iter()
        .find(|c| c["id"] == "curve.model.observations.ns.positive")
        .unwrap();
    for field in ["cached_zeros", "sampled_grid", "default_tau", "fitted"] {
        let mut request = c["request"].clone();
        request["inputs"]["curves"][0]["source"]["Parametric"][field] = json!(true);
        assert!(convex::library::decode_request(&request.to_string()).is_err());
    }
    let mut request = c["request"].clone();
    request["inputs"]["curves"][0]["source"]["Parametric"]
        .as_object_mut()
        .unwrap()
        .remove("horizon");
    assert!(convex::library::decode_request(&request.to_string()).is_err());
    let mut request = c["request"].clone();
    request["inputs"]["curves"][0]["source"]["Parametric"]["parameters"]
        .as_object_mut()
        .unwrap()
        .remove("tau");
    assert!(convex::library::decode_request(&request.to_string()).is_err());
}
