#[path = "../../../tests/fixtures/library/lot_pnl_contract.rs"]
mod controls;
#[test]
fn full_boundary_amendments_conserved_lots_and_causal_pnl() {
    controls::run(convex::library::execute_json);
}
