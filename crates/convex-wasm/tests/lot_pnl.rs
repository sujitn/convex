//! Native WASM surface; wasm32/browser execution is not claimed.
#[path = "../../../tests/fixtures/library/lot_pnl_contract.rs"]
mod controls;
#[test]
fn wasm_full_boundary_amendments_conserved_lots_and_causal_pnl() {
    controls::run(convex_wasm::library);
}
