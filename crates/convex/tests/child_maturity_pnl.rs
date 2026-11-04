#[path = "../../../tests/fixtures/library/child_maturity_pnl_contract.rs"]
mod controls;
#[test]
fn earned_child_principal_and_coupon_through_maturity_pnl() {
    controls::run(convex::library::execute_json);
}
