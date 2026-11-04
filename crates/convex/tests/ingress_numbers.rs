//! Raw model numbers must not become successful zero before typed validation.
use convex::library::execute_json;
use serde_json::{json, Value};
fn request(number: &str) -> String {
    r#"{"schema_version":1,"inputs":{"calendars":[],"curves":[],"fixings":[],"publications":[],"helper_calendars":[],"calibration":null},"operation":{"kind":"yield_scalar","action":{"kind":"report_percent","yield_value":__NUMBER__}}}"#.replace("__NUMBER__",number)
}
fn execute(input: &str) -> Value {
    serde_json::from_str(&execute_json(input)).unwrap()
}
#[test]
fn exact_numeric_boundary_declarations_preserve_nonzero_model_input() {
    let pack: Value = serde_json::from_str(include_str!("fixtures/number-admission.json")).unwrap();
    let cases = pack["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 15);
    let mut captured = Vec::new();
    let (mut success, mut rejected) = (0, 0);
    for case in cases {
        let input = request(case["number_text"].as_str().unwrap());
        let actual = execute(&input);
        if case["expected"] == "invalid_input" {
            assert_eq!(actual["ok"], "false", "{}: {actual}", case["id"]);
            assert_eq!(actual["error"]["code"], "invalid_input");
            assert!(actual["error"]["message"]
                .as_str()
                .unwrap()
                .contains("underflows binary64"));
            rejected += 1;
        } else {
            assert_eq!(actual["ok"], "true", "{}: {actual}", case["id"]);
            let value = actual["result"]["data"]["value"].as_f64().unwrap();
            assert_eq!(
                format!("{:016x}", value.to_bits()),
                case["expected_percent_bits"].as_str().unwrap()
            );
            success += 1;
        }
        captured.push(json!({"id":case["id"],"request_json":input,"actual_envelope":actual}));
    }
    assert_eq!(success, 9);
    assert_eq!(rejected, 6);
    println!("NUMERIC_INGRESS_REFERENCE={}", json!(captured));
}
#[test]
fn numeric_looking_quoted_and_escaped_text_keeps_the_formal_parser_owner() {
    for text in [
        "1e-400",
        "-1e-400",
        "\"1e-400\"",
        r#"\"1e-400"#,
        r#"\\1e-400"#,
        "é1e-400",
    ] {
        let mut value: Value = serde_json::from_str(&request("0.0")).unwrap();
        value["unexpected_text"] = json!(text);
        let actual = execute(&serde_json::to_string(&value).unwrap());
        assert_eq!(actual["ok"], "false");
        assert_eq!(actual["error"]["code"], "invalid_input");
        assert!(!actual["error"]["message"]
            .as_str()
            .unwrap()
            .contains("underflows binary64"));
    }
}
#[test]
fn original_byte_bounds_and_formal_json_failures_remain_errors() {
    let too_large = " ".repeat(2 * 1024 * 1024 + 1);
    let result = execute(&too_large);
    assert_eq!(result["error"]["code"], "resource_limit");
    for input in [
        "",
        "{",
        "[",
        "{\"n\":1-0}",
        "{\"n\":01}",
        "{\"n\":1e}",
        "{\"n\":+1}",
        "{\"text\":\"unterminated\\",
        "{\"n\":1e309}",
    ] {
        let result = execute(input);
        assert_eq!(result["ok"], "false");
        assert_eq!(
            result["error"]["code"], "invalid_input",
            "{input}: {result}"
        );
    }
}
