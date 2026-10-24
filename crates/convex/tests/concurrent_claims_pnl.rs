#[path = "../../../tests/fixtures/library/concurrent_claims_pnl_contract.rs"]
mod controls;
#[test]
fn concurrent_claims_and_returned_paid_recipients_pnl() {
    controls::run(convex::library::execute_json);
}
