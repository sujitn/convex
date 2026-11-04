//! Full curve-method consumer; emitted native envelopes are transport expectations only.
#[path = "../../../tests/fixtures/library/curve_method_contract.rs"]
mod contract;
use serde_json::{json, Value};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "schemars")]
    if std::env::args().any(|a| a == "--emit-schema") {
        println!("{}", convex::library::request_schema());
        return Ok(());
    }
    let mut outcomes = Vec::new();
    let mut parity = Vec::new();
    for case in contract::cases() {
        let input = contract::request(&case);
        let expected = contract::native(&input);
        let actual: Value = serde_json::from_str(&convex::library::execute_json(&input))?;
        if actual != expected
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
        json!({"certified":false,"qualification":"Fresh native curve-method transport results; predeclared independent analytic curve-method zero/discount/forward controls remain separate.","cases":cases})
    );
    Ok(())
}
