//! Native-compiled WASM export; browser/wasm32 execution remains unverified.
#[path = "../../../tests/fixtures/library/mixed_scenario_contract.rs"]
mod controls;
#[test]
fn wasm_mixed_book_revaluation_and_model_attribution() {
    controls::run(convex_wasm::library);
}
