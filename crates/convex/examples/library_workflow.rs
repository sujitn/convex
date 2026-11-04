//! Standalone complete supplied-input/client contract consumer; no service.
#[path = "support/library_fixture.rs"]
mod fixture;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "schemars")]
    if std::env::args().any(|a| a == "--emit-schema") {
        println!("{}", convex::library::request_schema());
        return Ok(());
    }
    let cases = fixture::cases()?;
    if std::env::args().any(|a| a == "--emit-requests") {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({"schema_version":1,"certified":false,
            "qualification":"Synthetic engineering parity requests; candidate outputs are not independent numerical oracles; original native controls and independent references are retained.","cases":cases}))?
        );
        return Ok(());
    }
    let mut results = vec![];
    let mut parity = vec![];
    for case in cases {
        let json = serde_json::to_string(&case.request)?;
        let native = convex::library::execute(&case.request);
        let actual: serde_json::Value =
            serde_json::from_str(&convex::library::execute_json(&json))?;
        match (native, case.expected_error) {
            (Ok(value), None) => {
                if actual["ok"] != "true" || actual["result"] != value {
                    return Err(format!("{}: JSON/native mismatch {actual}", case.id).into());
                }
            }
            (Err(error), Some(code)) if error.code == code => {
                if actual["error"] != serde_json::to_value(error)? {
                    return Err("error mismatch".into());
                }
            }
            (a, b) => return Err(format!("{}: expected{b:?}; actual{a:?}", case.id).into()),
        }
        results.push(serde_json::json!({"id":case.id,"ok":actual["ok"]}));
        parity.push(serde_json::json!({"id":case.id,"request":case.request,"expected_error":case.expected_error,"expected_envelope":actual}));
    }
    let output = if std::env::args().any(|a| a == "--emit-parity") {
        serde_json::json!({"certified":false,"qualification":"Fresh native outputs for adapter transport parity only; no financial oracle or independent certificate.","cases":parity})
    } else {
        serde_json::json!({"certified":false,"cases":results})
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
