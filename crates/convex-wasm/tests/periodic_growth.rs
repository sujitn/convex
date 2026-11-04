//! Actual native compiled WASM export; wasm32/browser qualification is separate.
#[path = "../../../tests/fixtures/library/periodic_growth_contract.rs"]
mod contract;
#[test]
fn wasm_periodic_growth_preserves_native_values_units_and_errors() {
    for case in contract::cases() {
        contract::check(&case, &convex_wasm::library(&contract::request(&case)));
    }
}
