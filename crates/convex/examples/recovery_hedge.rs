//! Full issuer cash risk, original-owner comparisons and fresh hedge transport controls.
#[path = "../../../tests/fixtures/library/recovery_hedge_contract.rs"]
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
        for owner in controls::owner_requests(&case, &output) {
            let output = convex::library::execute_json(&controls::request(&owner));
            controls::check_owner(&owner, &output);
            let envelope: Value = serde_json::from_str(&output)?;
            outputs.push(json!({"id":owner["id"],"request":owner["request"],"expected_error":null,"expected_envelope":envelope}));
        }
        for scenario in controls::scenarios(&case) {
            let output = convex::library::execute_json(&controls::request(&scenario));
            controls::check_scenario(&scenario, &output);
            let envelope: Value = serde_json::from_str(&output)?;
            outputs.push(json!({"id":scenario["id"],"request":scenario["request"],"expected_error":null,"expected_envelope":envelope}));
        }
    }
    for case in controls::mixed_requests(&outputs) {
        let output = convex::library::execute_json(&controls::request(&case));
        controls::check_mixed(&case, &output);
        let envelope: Value = serde_json::from_str(&output)?;
        outputs.push(json!({"id":case["id"],"request":case["request"],"expected_error":case["expected_error"],"expected_envelope":envelope}));
    }
    println!(
        "{}",
        json!({"certified":false,"qualification":"Fresh outputs prove transport and unchanged full graph owner agreement only. Independent complete-cash controls are separately pinned.","cases":outputs})
    );
    Ok(())
}
