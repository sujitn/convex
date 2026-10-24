//! Actual native compiled WASM export; wasm32/browser qualification is separate.
#[path = "../../../tests/fixtures/library/rate_helpers_contract.rs"]
mod contract;
#[test]
fn wasm_rate_helpers_preserve_native_rate_helpers_and_errors() {
    for case in contract::cases() {
        contract::check(&case, &convex_wasm::library(&contract::request(&case)));
    }
}
