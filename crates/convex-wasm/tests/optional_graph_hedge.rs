//! Native-compiled WASM export; browser/wasm32 execution remains unverified.
#[path = "../../../tests/fixtures/library/optional_graph_hedge_contract.rs"]
mod controls;
#[test]
fn wasm_guarded_optional_columns_and_book() {
    controls::run(convex_wasm::library);
}
