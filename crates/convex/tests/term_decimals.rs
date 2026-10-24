#[path = "../../../tests/fixtures/library/term_decimals_contract.rs"]
mod contract;

#[test]
fn complete_term_decimals_preserve_metadata_and_reject_lossy_ingress() {
    assert_eq!(contract::run()["status"], "passed");
}
