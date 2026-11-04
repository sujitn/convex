//! Aggregate book admission grows without changing the original financial guards.
use convex::library::*;
use convex_core::contracts::json::{parse, parse_book};
use serde_json::{json, Value};

fn request() -> Value {
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/library/graph-hedge-qualified.json"
    );
    let data: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    data["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "graph_hedge.positive.bond.base")
        .unwrap()["request"]
        .clone()
}

#[test]
fn aggregate_nodes_grow_but_lexical_container_and_byte_guards_remain() {
    let raw = json!(vec![vec![1; 512]; 80]).to_string();
    assert!(parse(&raw).is_err());
    assert!(parse_book(&raw).is_ok());
    assert!(parse_book(&json!(vec![vec![1; 512]; 256]).to_string()).is_err());
    for raw in [
        "{\"a\":1,\"a\":2}".to_owned(),
        "1e-999".to_owned(),
        json!("x".repeat(8193)).to_string(),
        json!(vec![0; 4097]).to_string(),
        format!("{}0{}", "[".repeat(65), "]".repeat(65)),
        " ".repeat(2 * 1024 * 1024 + 1),
    ] {
        assert!(parse_book(&raw).is_err());
    }
}

#[test]
fn managed_book_envelopes_match_original_native_finance_and_reject_lost_terms() {
    let value = request();
    let raw = value.to_string();
    assert_eq!(execute_book_json(&raw), execute_json(&raw));
    assert_eq!(
        serde_json::from_str::<Value>(&execute_book_json(&raw)).unwrap()["ok"],
        "true"
    );
    let mut extra = value.clone();
    extra["operation"]["input"]["unknown_economic_term"] = json!(1);
    assert!(decode_book_request(&extra.to_string()).is_err());
    let mut bad = value;
    bad["schema_version"] = json!(999);
    assert_eq!(
        serde_json::from_str::<Value>(&execute_book_json(&bad.to_string())).unwrap()["ok"],
        "false"
    );
}

#[test]
fn large_well_formed_json_is_not_permission_for_an_unrelated_operation() {
    let request = json!({"schema_version":1,"inputs":LibraryInputs::empty(),
        "operation":{"kind":"capabilities"}})
    .to_string();
    assert!(decode_request(&request).is_ok());
    assert!(decode_book_request(&request).is_err());
}
