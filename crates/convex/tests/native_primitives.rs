#[path = "../../../tests/fixtures/library/native_primitives_contract.rs"]
mod fixture;

#[test]
fn known_and_forecast_rates_preserve_independent_profiles_and_cash_dates() {
    assert_eq!(fixture::run()["status"], "passed");
}
