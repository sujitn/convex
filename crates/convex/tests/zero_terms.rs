#[path = "../../../tests/fixtures/library/zero_terms_contract.rs"]
mod contract;

#[test]
fn zero_terms_retain_fields_and_reject_unknown_substitutions() {
    assert_eq!(contract::run()["status"], "passed");
}
