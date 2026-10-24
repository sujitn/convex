//! Actual native compiled WASM export; wasm32/browser qualification is separate.
#[path = "../../../tests/fixtures/library/treasury_bill_contract.rs"]
mod contract;
#[test]
fn wasm_treasury_bill_preserves_native_quotes_risk_and_errors() {
    for case in contract::cases() {
        contract::check(&case, &convex_wasm::library(&contract::request(&case)));
    }
}
