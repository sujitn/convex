//! Native WASM surface; wasm32/browser execution is not claimed.
#[path = "../../../tests/fixtures/library/custody_lineage_pnl_contract.rs"]
mod controls;
#[test]
fn wasm_conserved_successive_custody_and_actual_disposal_pnl() {
    controls::run(convex_wasm::library);
}
