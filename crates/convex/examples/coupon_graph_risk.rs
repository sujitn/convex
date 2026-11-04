//! Standalone supplied term option control and exact request schema consumer.
#[path = "../../../tests/fixtures/library/coupon_graph_risk_contract.rs"]
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
