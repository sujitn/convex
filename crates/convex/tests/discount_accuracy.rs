#[path = "../../../tests/fixtures/library/discount_accuracy_contract.rs"]
mod contract;
#[test]
fn checked_discount_accuracy_values_units_and_failures_match_independent_references() {
    assert_eq!(contract::run()["cases"].as_array().unwrap().len(), 548);
}
