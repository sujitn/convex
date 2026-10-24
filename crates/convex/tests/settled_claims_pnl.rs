#[path = "../../../tests/fixtures/library/settled_claims_pnl_contract.rs"]
mod controls;
#[test]
fn settled_claims_and_returned_paid_recipients_pnl() {
    controls::run(convex::library::execute_json);
}
