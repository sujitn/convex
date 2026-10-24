#[path = "../../../tests/fixtures/library/dated_yield_contract.rs"]
mod contract;
#[test]
fn checked_dated_cash_values_units_and_failures_match_independent_references() {
    assert_eq!(contract::run()["cases"].as_array().unwrap().len(), 665);
}
