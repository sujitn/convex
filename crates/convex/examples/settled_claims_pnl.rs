//! Complete original principal/coupon settlement and native closure.
#[path = "../../../tests/fixtures/library/settled_claims_pnl_contract.rs"]
mod controls;
fn main() {
    let arguments = std::env::args().collect::<Vec<_>>();
    if let Some(at) = arguments.iter().position(|a| a == "--check-envelopes") {
        let raw = std::fs::read_to_string(&arguments[at + 1]).unwrap();
        let responses: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let cases = controls::cases();
        let answers = responses["cases"].as_array().unwrap();
        assert_eq!(answers.len(), cases.len());
        for (case, answer) in cases.iter().zip(answers) {
            assert_eq!(case["id"], answer["id"]);
            controls::check(case, &answer["envelope"].to_string());
        }
        println!(
            "{}",
            serde_json::json!({"status":"passed","full_independent_financial_checks":cases.len(),"original_checkers_and_bounds":true,"exact_transport_parity":false,"certified":false})
        );
        return;
    }
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
