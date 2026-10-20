//! Full contextual ICMA consumer; emitted native outputs are transport only.
#[path = "../../../tests/fixtures/library/icma_contract.rs"]
mod controls;
use serde_json::{json, Value};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "schemars")]
    if std::env::args().any(|a| a == "--emit-schema") {
        println!("{}", convex::library::request_schema());
        return Ok(());
    }
    let mut cases = Vec::new();
    for c in controls::cases() {
        let wire = convex::library::execute_json(&controls::request(&c));
        controls::check(&c, &wire);
        let output: Value = serde_json::from_str(&wire)?;
        cases.push(json!({"id":c["id"],"request":c["request"],"expected_error":c["expected_error"],"expected_envelope":output}));
    }
    println!(
        "{}",
        json!({"certified":false,"qualification":"Fresh native outputs prove transport only; original Decimal60 financial answers are separate.","cases":cases})
    );
    Ok(())
}
