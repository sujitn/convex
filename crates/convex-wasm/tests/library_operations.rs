//! Actual native compiled WASM export; wasm32/browser qualification is separate.
#[path = "../../../tests/fixtures/library/operations_contract.rs"]
mod contract;
#[test]
fn wasm_operations_preserve_native_conversions_bucket_results_and_errors() {
    for case in contract::cases() {
        contract::check(&case, &convex_wasm::library(&contract::request(&case)));
    }
}
