//! Native-compiled WASM export; browser/wasm32 execution remains unverified.
#[path = "../../../tests/fixtures/library/cohort_pnl_contract.rs"]
mod controls;
#[test]
fn wasm_cohort_outstanding_cash_physical_reconciliation() {
    controls::run(convex_wasm::library);
}
