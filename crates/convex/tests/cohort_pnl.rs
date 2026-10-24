#[path = "../../../tests/fixtures/library/cohort_pnl_contract.rs"]
mod controls;
#[test]
fn full_cohort_outstanding_cash_physical_reconciliation() {
    controls::run(convex::library::execute_json);
}
