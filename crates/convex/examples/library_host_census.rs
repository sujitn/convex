//! Emit complete current native envelopes for actual host transport checks.
//! Financial reference answers remain in the independently declared controls.
use serde_json::{json, Value};
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("supply one original library request corpus path")?;
    if std::fs::metadata(&path)?.len() > 8 * 1024 * 1024 {
        return Err("original corpus exceeds the capability inventory bound".into());
    }
    let corpus: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let cases = corpus["cases"].as_array().ok_or("missing complete cases")?;
    let mut output = io::BufWriter::new(io::stdout().lock());
    for case in cases {
        let input = serde_json::to_string(&case["request"])?;
        let native = convex::library::decode_request(&input)
            .and_then(|request| convex::library::execute(&request));
        let envelope = match native {
            Ok(result) => json!({"ok":"true","schema_version":1,"result":result}),
            Err(error) => json!({"ok":"false","schema_version":1,"error":error}),
        };
        let shared: Value = serde_json::from_str(&convex::library::execute_json(&input))?;
        if shared != envelope
            || (case["expected_error"].is_null() && envelope["ok"] != "true")
            || (!case["expected_error"].is_null()
                && (envelope["ok"] != "false"
                    || envelope["error"]["code"] != case["expected_error"]))
        {
            return Err(format!("{}: original native/JSON outcome differs", case["id"]).into());
        }
        serde_json::to_writer(
            &mut output,
            &json!({"source_case":case,"id":case["id"],"request":case["request"],"request_json":input,
                "expected_error":case["expected_error"],"expected_envelope":envelope}),
        )?;
        writeln!(&mut output)?;
    }
    output.flush()?;
    Ok(())
}
