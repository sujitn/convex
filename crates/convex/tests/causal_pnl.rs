#[path = "../../../tests/fixtures/library/causal_pnl_contract.rs"]
mod controls;
#[test]
fn full_causal_lifecycle_cash_reconciliation() {
    controls::run(convex::library::execute_json);
}
