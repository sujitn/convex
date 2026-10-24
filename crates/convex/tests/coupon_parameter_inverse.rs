#[path = "../../../tests/fixtures/library/coupon_parameter_inverse_contract.rs"]
mod contract;
#[test]
fn full_native_parameter_derivatives_preserve_values_units_and_failures() {
    assert_eq!(contract::run()["cases"].as_array().unwrap().len(), 37);
}
