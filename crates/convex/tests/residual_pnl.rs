#[path = "../../../tests/fixtures/library/residual_pnl_contract.rs"]
mod controls;
#[test]
fn full_residual_cash_partial_security_and_causal_pnl() {
    controls::run(convex::library::execute_json);
}
