#[path = "../../../tests/fixtures/library/whole_coupon_normal_contract.rs"]
mod contract;
#[test]
fn complete_whole_coupon_normal_dispatch_preserves_independent_finance_and_failures() {
    let r = contract::run();
    assert_eq!(r["status"], "passed");
    assert_eq!(r["cases"].as_array().unwrap().len(), 21);
}
