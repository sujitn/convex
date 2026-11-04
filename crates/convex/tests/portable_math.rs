#[path = "../../../tests/fixtures/library/portable_math_contract.rs"]
mod contract;

#[test]
fn portable_kernels_match_independent_exact_binary_controls() {
    assert!(contract::verify() > 350);
}
