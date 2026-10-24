#[path = "../../../tests/fixtures/library/child_cash_pnl_contract.rs"]
mod controls;
#[test]
fn earned_child_cash_and_independent_transfers_pnl() {
    controls::run(convex::library::execute_json);
}
