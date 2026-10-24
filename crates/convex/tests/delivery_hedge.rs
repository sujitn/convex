#[path = "../../../tests/fixtures/library/delivery_hedge_contract.rs"]
mod controls;
#[test]
fn full_delivery_basket_ctd_risk_and_hedges() {
    controls::run(convex::library::execute_json);
}
