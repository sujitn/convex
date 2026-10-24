//! Native WASM surface; wasm32/browser execution is not claimed.
#[path = "../../../tests/fixtures/library/physical_partition_pnl_contract.rs"]
mod controls;
#[test]
fn wasm_conserved_physical_custody_and_invoice_pnl() {
    controls::run(convex_wasm::library);
}
