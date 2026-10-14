#[path = "../../../tests/fixtures/library/portable_math_contract.rs"]
mod contract;

fn main() {
    println!(
        "{} independent portable kernel controls passed",
        contract::verify()
    );
}
