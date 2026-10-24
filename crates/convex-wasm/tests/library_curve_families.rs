//! Actual native compiled WASM export; wasm32/browser qualification is separate.
#[path = "../../../tests/fixtures/library/curve_family_contract.rs"]
mod contract;
#[test]
fn wasm_curve_families_preserve_native_curve_families_and_errors() {
    for case in contract::cases() {
        contract::check(&case, &convex_wasm::library(&contract::request(&case)));
    }
}
