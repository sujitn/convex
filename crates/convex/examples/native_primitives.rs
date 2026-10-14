#[path = "../../../tests/fixtures/library/native_primitives_contract.rs"]
mod fixture;

fn main() {
    println!("{}", fixture::run());
}
