//! Native WASM surface; wasm32/browser execution is not claimed.
#[path = "../../../tests/fixtures/library/concurrent_claims_pnl_contract.rs"]
mod controls;
#[test]
fn wasm_concurrent_claims_and_returned_paid_recipients_pnl() {
    controls::run(convex_wasm::library);
}
