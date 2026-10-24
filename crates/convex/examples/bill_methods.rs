//! Standalone checked bill-object, rate-unit and independent-domain controls.
#[path = "../../../tests/fixtures/library/bill_methods_contract.rs"]
mod contract;

fn main() {
    println!("{}", contract::run());
}
