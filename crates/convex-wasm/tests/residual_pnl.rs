//! Native WASM surface; wasm32/browser execution is not claimed.
#[path = "../../../tests/fixtures/library/residual_pnl_contract.rs"]
mod controls;
#[test]
fn wasm_full_residual_cash_partial_security_and_causal_pnl() {
    controls::run(convex_wasm::library);
}
