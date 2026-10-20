//! Full Treasury bill consumer; native envelopes are transport expectations only.
#[path = "../../../tests/fixtures/library/treasury_bill_contract.rs"]
mod contract;
use serde_json::{json, Value};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "schemars")]
    if std::env::args().any(|a| a == "--emit-schema") {
        println!("{}", convex::library::request_schema());
        return Ok(());
    }
    let mut results = Vec::new();
    for case in contract::cases() {
        let input = contract::request(&case);
        let actual: Value = serde_json::from_str(&convex::library::execute_json(&input))?;
        let native = contract::native(&input);
        let error = &case["expected_error"];
        if actual != native
            || actual["ok"] != if error.is_null() { "true" } else { "false" }
            || (!error.is_null() && actual["error"]["code"] != *error)
        {
            return Err(format!("{}: {actual}", case["id"]).into());
        }
        results.push(if std::env::args().any(|a| a == "--emit-parity") {
            json!({"id":case["id"],"request":case["request"],"expected_error":error,"expected_envelope":actual})
        } else { json!({"id":case["id"],"ok":actual["ok"]}) });
    }
    println!("{}", json!({"certified":false,"cases":results}));
    Ok(())
}
