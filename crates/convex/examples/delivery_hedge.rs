//! Run complete original delivery basket/risk and independently controlled books.
#[path = "../../../tests/fixtures/library/delivery_hedge_contract.rs"]
mod controls;
fn main() {
    #[cfg(feature = "schemars")]
    if std::env::args().any(|a| a == "--emit-schema") {
        println!("{}", convex::library::request_schema());
        return;
    }
    println!(
        "{}",
        serde_json::json!({"certified":false,
        "qualification":"Full independent financial controls precede execution. Fresh envelopes prove transport only.",
        "cases":controls::run(convex::library::execute_json)})
    );
}
