#[path = "../../../tests/fixtures/library/term_decimals_contract.rs"]
mod contract;

fn main() {
    println!("{}", contract::run());
}
