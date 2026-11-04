#[path = "../../../tests/fixtures/library/mixed_scenario_contract.rs"]
mod controls;
#[test]
fn full_mixed_book_revaluation_and_model_attribution() {
    controls::run(convex::library::execute_json);
}
