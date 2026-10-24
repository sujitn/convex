//! Native WASM surface; wasm32/browser execution is not claimed.
#[path = "../../../tests/fixtures/library/claim_pnl_contract.rs"]
mod controls;
#[test]
fn wasm_full_original_cash_transfers_and_causal_pnl() {
    controls::run(convex_wasm::library);
}
