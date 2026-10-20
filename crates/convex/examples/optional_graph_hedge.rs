//! Full native optional risk certificates and freshly calculated hedge columns.
#[path = "../../../tests/fixtures/library/optional_graph_hedge_contract.rs"]
mod controls;
fn main() {
    #[cfg(feature = "schemars")]
    if std::env::args().any(|a| a == "--emit-schema") {
        println!("{}", convex::library::request_schema());
        return;
    }
    let outputs = controls::run(convex::library::execute_json);
    println!(
        "{}",
        serde_json::json!({"certified":false,"qualification":"Fresh envelopes prove transport and original native-owner agreement. Independent complete financial controls are separately declared.","cases":outputs})
    );
}
