//! Actual native compiled WASM export; wasm32/browser qualification is separate.
#[path = "../../../tests/fixtures/library/cash_contract.rs"]
mod contract;
#[test]
fn wasm_cash_preserve_native_cash_quotes_government_spreads_and_errors() {
    for case in contract::cases() {
        contract::check(&case, &convex_wasm::library(&contract::request(&case)));
    }
}
