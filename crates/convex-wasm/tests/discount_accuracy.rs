//! Actual native compiled WASM export; wasm32/browser qualification is separate.
#[path = "../../../tests/fixtures/library/discount_accuracy_contract.rs"]
mod contract;
#[test]
fn wasm_discount_accuracy_preserves_native_values_units_and_errors() {
    for case in contract::cases() {
        contract::check(&case, &convex_wasm::library(&contract::request(&case)));
    }
}
