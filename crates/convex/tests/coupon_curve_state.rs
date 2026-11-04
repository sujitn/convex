#[path = "../../../tests/fixtures/library/coupon_curve_state_contract.rs"]
mod contract;
#[test]
fn full_model_scenarios_preserve_independent_values_and_information() {
    assert_eq!(contract::run()["cases"].as_array().unwrap().len(), 34);
}
