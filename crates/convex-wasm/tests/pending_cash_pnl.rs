//! Native WASM surface; wasm32/browser execution is not claimed.
#[path = "../../../tests/fixtures/library/pending_cash_pnl_contract.rs"]
mod controls;
#[test]
fn wasm_pending_coupon_principal_custody_and_receipts_pnl() {
    controls::run(convex_wasm::library);
}
