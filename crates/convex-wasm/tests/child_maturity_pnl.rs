//! Native WASM surface; wasm32/browser execution is not claimed.
#[path = "../../../tests/fixtures/library/child_maturity_pnl_contract.rs"]
mod controls;
#[test]
fn wasm_earned_child_principal_and_coupon_through_maturity_pnl() {
    controls::run(convex_wasm::library);
}
