#[path = "../../../tests/fixtures/library/optional_graph_hedge_contract.rs"]
mod controls;

#[test]
fn complete_guarded_optional_columns_and_hedge_book() {
    controls::run(convex::library::execute_json);
}
