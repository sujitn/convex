//! Actual native compiled WASM export; wasm32/browser qualification is separate.
#[path = "../../../tests/fixtures/library/curve_method_contract.rs"]
mod contract;
#[test]
fn wasm_curve_methods_preserve_native_curve_methods_and_errors() {
    for case in contract::cases() {
        contract::check(&case, &convex_wasm::library(&contract::request(&case)));
    }
}
