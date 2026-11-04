//! Native WASM surface; wasm32/browser execution is not claimed.
#[path = "../../../tests/fixtures/library/child_cash_pnl_contract.rs"]
mod controls;
#[test]
fn wasm_earned_child_cash_and_independent_transfers_pnl() {
    controls::run(convex_wasm::library);
}
