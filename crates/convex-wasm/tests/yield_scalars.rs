//! Actual native compiled WASM export; wasm32/browser qualification is separate.
#[path = "../../../tests/fixtures/library/yield_scalars_contract.rs"]
mod contract;
#[test]
fn wasm_yield_scalars_preserves_native_values_units_and_errors() {
    for case in contract::cases() {
        contract::check(&case, &convex_wasm::library(&contract::request(&case)));
    }
}
