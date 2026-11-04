//! Full original UFR terms, exact native dispatch and strict schema admission.
#[path = "../../../tests/fixtures/library/ufr_contract.rs"]
mod contract;
use serde_json::{json, Value};

#[test]
fn supplied_ufr_contracts_roundtrip_and_preserve_actual_failures() {
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
fn analytic_join_provenance_and_no_grid_substitute_survive_json() {
    let cases = contract::cases();
    let output = |id: &str| -> Value {
        serde_json::from_str(&convex::library::execute_json(&contract::request(
            cases.iter().find(|c| c["id"] == id).unwrap(),
        )))
        .unwrap()
    };
    let described = output("ufr.describe.knot_cutoff");
    let curve = described["result"]["data"]["curves"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["recipe"]["key"]["id"] == "ufr.knot_cutoff")
        .unwrap();
    assert_eq!(curve["ufr"]["base"]["version"], "source.v7");
    assert_eq!(curve["ufr"]["boundary_tenor"], 10.0);
    assert_eq!(curve["ufr"]["boundary_forward"], 0.02);
    assert_eq!(curve["interpolation_grid"]["status"], "unsupported");
    assert_eq!(curve["diagnostics"]["iterations"], 0);
    assert_eq!(
        output("ufr.UfrForward.knot_cutoff.10")["result"]["data"]["extrapolated"],
        true
    );
    assert_eq!(
        output("ufr.Raw.knot_cutoff.10")["result"]["data"]["extrapolated"],
        false
    );
    assert_eq!(
        output("ufr.positive_origin_identity")["result"]["data"]["extrapolated"],
        false
    );
    assert!(output("ufr.failure.missing_base").get("result").is_none());
}

#[test]
fn missing_llp_parameters_versions_and_extra_policy_terms_fail_strict_decode() {
    let cases = contract::cases();
    let original = &cases
        .iter()
        .find(|c| c["id"] == "ufr.describe.positive")
        .unwrap()["request"];
    for field in ["base", "last_liquid_point"] {
        let mut bad = original.clone();
        bad["inputs"]["curves"][0]["source"]["UfrConvergence"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(convex::library::decode_request(&bad.to_string()).is_err());
    }
    for field in ["ufr", "alpha"] {
        let mut bad = original.clone();
        bad["inputs"]["curves"][0]["extrapolation"]["UfrConvergence"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(convex::library::decode_request(&bad.to_string()).is_err());
    }
    for field in [
        "boundary_forward",
        "derivative_fallback",
        "smith_wilson",
        "provider",
        "sampled_grid",
    ] {
        let mut bad = original.clone();
        bad["inputs"]["curves"][0]["source"]["UfrConvergence"][field] = json!(0.03);
        assert!(convex::library::decode_request(&bad.to_string()).is_err());
    }
}
