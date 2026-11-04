//! The actual MCP wrapper preserves opaque request text and rejects ignored fields.
use convex_mcp::server::LibraryToolParams;
use serde_json::json;

#[test]
fn library_wrapper_preserves_the_exact_request_string() {
    let text = "  {\"opaque_decimal\":\"0.000000000000000000000001\"} \n";
    let parsed: LibraryToolParams = serde_json::from_value(json!({"request_json":text})).unwrap();
    assert_eq!(parsed.request_json, text);
}

#[test]
fn library_wrapper_rejects_unknown_missing_and_wrong_type_fields() {
    for value in [
        json!({"request_json":"{}","unexpected_argument":123}),
        json!({}),
        json!({"request_json":null}),
        json!({"request_json":123}),
        json!({"request_json":{}}),
        json!({"request_json":[]}),
    ] {
        assert!(serde_json::from_value::<LibraryToolParams>(value).is_err());
    }
}

#[test]
fn generated_wrapper_schema_matches_actual_strict_deserialization() {
    let schema = serde_json::to_value(schemars::schema_for!(LibraryToolParams)).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["required"], json!(["request_json"]));
    assert_eq!(schema["properties"]["request_json"]["type"], "string");
    assert_eq!(schema["properties"].as_object().unwrap().len(), 1);
}
