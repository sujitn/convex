#[path = "../../../tests/fixtures/library/periodic_growth_contract.rs"]
mod contract;
#[test]
fn checked_projection_values_units_and_failures_match_independent_references() {
    assert_eq!(contract::run()["cases"].as_array().unwrap().len(), 433);
}
