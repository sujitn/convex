#[path = "../../../tests/fixtures/library/product_parametric.rs"]
mod fixture;
#[test]
fn native_compiled_wasm_product_calibration_results_and_failures_match_native() {
    for case in fixture::transport_cases() {
        fixture::check_transport(
            &case,
            &convex_wasm::library(&fixture::request(
                &case,
                serde_json::json!({"kind":"build"}),
            )),
        );
    }
}
