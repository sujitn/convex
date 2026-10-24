//! Native-compiled WASM export. Actual wasm32/browser qualification stays separate.
#[path = "../../../tests/fixtures/library/graph_hedge_contract.rs"]
mod controls;
#[test]
fn wasm_graph_hedge_and_complete_hedge_columns_preserve_original_controls() {
    let mut outputs = Vec::new();
    for case in controls::cases() {
        let output = convex_wasm::library(&controls::request(&case));
        controls::check(&case, &output);
        outputs.push(serde_json::json!({"id":case["id"],"expected_envelope":serde_json::from_str::<serde_json::Value>(&output).unwrap()}));
        for owner in controls::owner_requests(&case, &output) {
            controls::check_owner(&owner, &convex_wasm::library(&controls::request(&owner)));
        }
    }
    for case in controls::mixed_requests(&outputs) {
        controls::check_mixed(&case, &convex_wasm::library(&controls::request(&case)));
    }
}
