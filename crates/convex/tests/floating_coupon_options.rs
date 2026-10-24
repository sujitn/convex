#[path = "../../../tests/fixtures/library/term_coupon_options_contract.rs"]
mod contract;
#[test]
fn complete_term_coupon_option_dispatch_preserves_independent_finance_and_failures() {
    let r = contract::run();
    assert_eq!(r["status"], "passed");
    assert_eq!(r["cases"].as_array().unwrap().len(), 21);
}
