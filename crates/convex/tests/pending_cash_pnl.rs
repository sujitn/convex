#[path = "../../../tests/fixtures/library/pending_cash_pnl_contract.rs"]
mod controls;
#[test]
fn pending_coupon_principal_custody_and_receipts_pnl() {
    controls::run(convex::library::execute_json);
}
