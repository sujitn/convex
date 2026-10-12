#[path = "../../../tests/fixtures/library/physical_partition_pnl_contract.rs"]
mod controls;
#[test]
fn conserved_physical_custody_and_invoice_pnl() {
    controls::run(convex::library::execute_json);
}
