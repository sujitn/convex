#[path = "../../../tests/fixtures/library/custody_lineage_pnl_contract.rs"]
mod controls;
#[test]
fn conserved_successive_custody_and_actual_disposal_pnl() {
    controls::run(convex::library::execute_json);
}
