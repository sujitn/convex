#[path = "../../../tests/fixtures/library/zero_terms_contract.rs"]
mod contract;

fn main() {
    println!("{}", contract::run());
}
