//! Full original boundary amendment and conserved original lot controls.
#[path = "../../../tests/fixtures/library/lot_pnl_contract.rs"]
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
        println!(
            "{}",
            serde_json::json!({"status":"passed","cases":cases.len(),
            "financial_books":positive,"typed_failures":cases.len()-positive,
            "full_product_revaluations":cases.iter().filter_map(|c|
                c["expected_envelope"]["result"]["data"]["report"]["product_evaluations"].as_u64()).sum::<u64>(),
            "certified":false,"documentation":"docs/COHORT-BOOK-PNL.md"})
        );
    }
}
