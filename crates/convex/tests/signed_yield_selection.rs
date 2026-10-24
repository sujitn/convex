#[path = "../../../tests/fixtures/library/signed_yield_selection_contract.rs"]
mod contract;
#[test]
fn complete_selected_yield_requests_retain_roots_precision_and_error_scopes() {
    assert_eq!(contract::run()["cases"].as_array().unwrap().len(), 27);
}
