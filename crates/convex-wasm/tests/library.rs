//! Native compilation tests the actual exported pure WASM adapter function.
//! A browser/wasm32 execution remains a distinct host gate.
#[path = "../../../tests/fixtures/library/contract.rs"]
mod contract;
#[test]
fn wasm_adapter_preserves_raw_json_cash_results_and_errors() {
    for case in contract::cases() {
        contract::check(&case, &convex_wasm::library(&contract::request(&case)));
    }
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&convex_wasm::library_schema()).unwrap(),
        convex::library::request_schema()
    );
}
