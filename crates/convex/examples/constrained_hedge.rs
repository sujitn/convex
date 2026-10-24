//! Complete supplied-factor consumer; fresh outputs are transport expectations only.
#[path = "../../../tests/fixtures/library/constrained_hedge.rs"]
mod controls;
use serde_json::{json, Value};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "schemars")]
    if std::env::args().any(|a| a == "--emit-schema") {
        println!("{}", convex::library::request_schema());
        return Ok(());
    }
    let mut outcomes = Vec::new();
    let mut parity = Vec::new();
    for case in controls::cases() {
        let wire = controls::request(&case);
        let output = convex::library::execute_json(&wire);
        controls::check(&case, &output);
        let actual: Value = serde_json::from_str(&output)?;
        outcomes.push(
            json!({"id":case["id"],"ok":actual["ok"],"status":actual["result"]["data"]["status"]}),
        );
        parity.push(json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":actual}));
    }
    println!(
        "{}",
        json!({"certified":false,"qualification":"Fresh native supplied-factor transport results; independent Decimal financial controls are separate.",
        "cases":if std::env::args().any(|a|a=="--emit-parity") {parity}else{outcomes}})
    );
    Ok(())
}
