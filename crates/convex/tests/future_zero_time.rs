#[path = "../../../tests/fixtures/library/future_zero_time_contract.rs"]
mod contract;

#[test]
fn future_zero_time_cash_matches_independent_references_and_root_classes() {
    assert_eq!(contract::run()["cases"].as_array().unwrap().len(), 368);
}
