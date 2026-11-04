//! Actual native compiled WASM export; wasm32/browser qualification is separate.
#[path = "../../../tests/fixtures/library/icma_workout_contract.rs"]
mod contract;
#[test]
fn wasm_icma_workout_preserves_all_native_results_and_failures() {
    for case in contract::cases() {
        contract::check(&case, &convex_wasm::library(&contract::request(&case)));
    }
}
