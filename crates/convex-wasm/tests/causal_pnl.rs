//! Native-compiled WASM export; browser/wasm32 execution remains unverified.
#[path = "../../../tests/fixtures/library/causal_pnl_contract.rs"]
mod controls;
#[test]
fn wasm_causal_lifecycle_cash_reconciliation() {
    controls::run(convex_wasm::library);
}
