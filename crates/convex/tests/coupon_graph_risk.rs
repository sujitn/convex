#[path = "../../../tests/fixtures/library/coupon_graph_risk_contract.rs"]
mod contract;
#[test]
fn complete_coupon_graph_risk_dispatch_preserves_independent_finance_and_failures() {
    let r = contract::run();
    assert_eq!(r["status"], "passed");
    assert_eq!(r["cases"].as_array().unwrap().len(), 27);
}
