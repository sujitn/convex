//! Standalone checked yield projection controls and actual request schema.
#[path = "../../../tests/fixtures/library/reinvestment_contract.rs"]
mod contract;
fn main() {
    if std::env::args().any(|v| v == "--emit-schema") {
        #[cfg(feature = "schemars")]
        println!("{}", convex::library::request_schema());
        #[cfg(not(feature = "schemars"))]
        eprintln!("schema requires the explicit schemars feature");
        return;
    }
    println!("{}", contract::run());
}
