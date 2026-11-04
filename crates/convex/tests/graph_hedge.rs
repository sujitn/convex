//! Actual native graph pricing and complete common-factor hedge columns.
#[path = "../../../tests/fixtures/library/graph_hedge_contract.rs"]
mod controls;

#[test]
fn full_graph_columns_match_independent_cash_and_actual_original_risk() {
    let mut outputs = Vec::new();
    for case in controls::cases() {
        let output = convex::library::execute_json(&controls::request(&case));
        controls::check(&case, &output);
        outputs.push(serde_json::json!({"id":case["id"],"expected_envelope":serde_json::from_str::<serde_json::Value>(&output).unwrap()}));
        for owner in controls::owner_requests(&case, &output) {
            controls::check_owner(
                &owner,
                &convex::library::execute_json(&controls::request(&owner)),
            );
        }
    }
    for case in controls::mixed_requests(&outputs) {
        controls::check_mixed(
            &case,
            &convex::library::execute_json(&controls::request(&case)),
        );
    }
}
