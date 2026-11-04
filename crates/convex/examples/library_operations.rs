//! Standalone exact operation consumer; expectations are native transport only.
#[path = "../../../tests/fixtures/library/operations_contract.rs"]
mod contract;
use serde_json::{json, Value};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "schemars")]
    if std::env::args().any(|a| a == "--emit-schema") {
        println!("{}", convex::library::request_schema());
        return Ok(());
    }
    let mut parity = Vec::new();
    let mut outcomes = Vec::new();
    for case in contract::cases() {
        let input = contract::request(&case);
        let expected = contract::native(&input);
        let actual: Value = serde_json::from_str(&convex::library::execute_json(&input))?;
        if expected != actual
            || actual["ok"]
                != if case["expected_error"].is_null() {
                    "true"
                } else {
                    "false"
                }
            || (!case["expected_error"].is_null()
                && actual["error"]["code"] != case["expected_error"])
        {
            return Err(format!("{}: actual native result {actual}", case["id"]).into());
        }
        outcomes.push(json!({"id":case["id"],"ok":actual["ok"]}));
        parity.push(json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":actual}));
    }
    let cases = if std::env::args().any(|a| a == "--emit-parity") {
        parity
    } else {
        outcomes
    };
    println!(
        "{}",
        json!({"certified":false,"qualification":"Fresh native transport outputs; no independent price oracle or certificate.","cases":cases})
    );
    Ok(())
}
