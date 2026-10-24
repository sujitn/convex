//! Full original quotes to direct ICMA risk to complete same-factor hedge proposals.
#[path = "../../../tests/fixtures/library/icma_risk_contract.rs"]
mod controls;
use serde_json::{json, Value};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "schemars")]
    if std::env::args().any(|a| a == "--emit-schema") {
        println!("{}", convex::library::request_schema());
        return Ok(());
    }
    let mut outputs = Vec::new();
    for case in controls::cases() {
        let output = convex::library::execute_json(&controls::request(&case));
        controls::check(&case, &output);
        let envelope: Value = serde_json::from_str(&output)?;
        outputs.push(json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":envelope}));
        if case["expected_error"].is_null()
            && case["request"]["operation"]["action"]["kind"] == "hedge_vector"
        {
            for hedge in controls::hedge_requests(&case, &output) {
                let wire = convex::library::execute_json(&controls::request(&hedge));
                controls::check_hedge(&hedge, &wire);
                let envelope: Value = serde_json::from_str(&wire)?;
                outputs.push(json!({"id":hedge["id"],"request":hedge["request"],"expected_error":hedge["expected_error"],"expected_envelope":envelope}));
            }
        }
    }
    println!(
        "{}",
        json!({"certified":false,"qualification":"Fresh envelopes prove transport only; full independent Decimal60 quotations/hedge controls are separate.","cases":outputs})
    );
    Ok(())
}
