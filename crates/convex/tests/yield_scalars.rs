#[path = "../../../tests/fixtures/library/yield_scalars_contract.rs"]
mod contract;
#[test]
fn checked_scalar_values_units_and_failures_match_independent_references() {
    assert_eq!(contract::run()["cases"].as_array().unwrap().len(), 183);
}
