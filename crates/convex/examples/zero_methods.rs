#[path = "../../../tests/fixtures/library/zero_methods_contract.rs"]
mod contract;

fn main() {
    println!("{}", contract::run());
}
