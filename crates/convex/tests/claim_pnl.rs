#[path = "../../../tests/fixtures/library/claim_pnl_contract.rs"]
mod controls;
#[test]
fn full_original_cash_transfers_and_causal_pnl() {
    controls::run(convex::library::execute_json);
}
