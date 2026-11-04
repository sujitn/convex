#[path = "../../../tests/fixtures/library/zero_methods_contract.rs"]
mod contract;

#[test]
fn checked_zero_cash_yield_dates_compounding_and_failures() {
    assert_eq!(contract::run()["status"], "passed");
}
