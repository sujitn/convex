//! Full original nonlinear mixed-book scenarios and independent P&L controls.
#[path = "../../../tests/fixtures/library/mixed_scenario_contract.rs"]
mod controls;
fn main() {
    #[cfg(feature = "schemars")]
    if std::env::args().any(|a| a == "--emit-schema") {
        println!("{}", convex::library::request_schema());
        return;
    }
    let cases = controls::run(convex::library::execute_json);
    if std::env::args().any(|a| a == "--emit-parity") {
        println!("{}", serde_json::json!({"certified":false,"cases":cases}));
    } else {
        let positive = cases
            .iter()
            .filter(|c| c["expected_error"].is_null())
            .count();
        let valuations: usize = cases
            .iter()
            .filter_map(|c| {
                c["expected_envelope"]["result"]["data"]["report"]["product_evaluations"].as_u64()
            })
            .map(|n| n as usize)
            .sum();
        println!(
            "{}",
            serde_json::json!({"status":"passed","certified":false,
            "complete_cases":cases.len(),"financial_books":positive,"typed_failures":cases.len()-positive,
            "full_product_revaluations":valuations,
            "meaning":"hypothetical same-information-set model marking and ordered attribution; futures quote levels and cash PV remain separate",
            "documentation":"docs/MIXED-BOOK-SCENARIOS.md"})
        );
    }
}
