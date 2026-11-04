//! Native-compiled WASM export; browser/wasm32 execution remains unverified.
#[path = "../../../tests/fixtures/library/delivery_hedge_contract.rs"]
mod controls;
#[test]
fn wasm_delivery_basket_columns_and_book() {
    controls::run(convex_wasm::library);
}
