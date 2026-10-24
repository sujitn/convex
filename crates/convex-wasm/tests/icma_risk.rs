//! Native-compiled WASM export. Actual wasm32/browser qualification stays separate.
#[path = "../../../tests/fixtures/library/icma_risk_contract.rs"]
mod controls;
#[test]
fn wasm_icma_risk_and_complete_hedge_columns_preserve_original_controls() {
    for case in controls::cases() {
        let output = convex_wasm::library(&controls::request(&case));
        controls::check(&case, &output);
        if case["expected_error"].is_null()
            && case["request"]["operation"]["action"]["kind"] == "hedge_vector"
        {
            for hedge in controls::hedge_requests(&case, &output) {
                controls::check_hedge(&hedge, &convex_wasm::library(&controls::request(&hedge)));
            }
        }
    }
}
