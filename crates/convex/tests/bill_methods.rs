#[path = "../../../tests/fixtures/library/bill_methods_contract.rs"]
mod contract;

#[test]
fn checked_bill_kernels_objects_units_and_failures() {
    assert_eq!(contract::run()["status"], "passed");
}
