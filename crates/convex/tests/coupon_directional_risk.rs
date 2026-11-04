#[path = "../../../tests/fixtures/library/coupon_directional_risk_contract.rs"]
mod contract;
#[test]
fn directional_parameters_preserve_full_prices_and_failures() {
    assert_eq!(contract::run()["cases"].as_array().unwrap().len(), 110);
}
