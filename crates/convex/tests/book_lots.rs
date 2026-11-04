//! Independent binary-exact lot arithmetic and unchanged legacy rejection.
use convex::library::{execute_book_json, execute_json};
use serde_json::{json, Value};

fn request() -> Value {
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/library/constrained-hedge-admitted.json"
    );
    let data: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let mut value = data["cases"][0]["request"].clone();
    let problem = &mut value["operation"]["problem"];
    let first = problem["candidates"][0]["risk"].clone();
    let mut second = first.clone();
    second["id"] = json!("LEGAL.LOT.2");
    second["economics"]["id"] = json!("AUDIT.LOT.2.TERMS");
    second["calculation"]["id"] = json!("AUDIT.LOT.2.MODEL");
    problem["expected_holdings"] = json!([first["id"], second["id"]]);
    problem["holdings"] = json!([
        {"risk":first,"current_quantity":"1000","pending_quantity":"-500"},
        {"risk":second,"current_quantity":"2000","pending_quantity":"1000"}]);
    value["operation"]["action"] = json!({"kind":"solve_book","states":[],"scope":null});
    value
}
fn run(value: &Value) -> Value {
    serde_json::from_str(&execute_book_json(&value.to_string())).unwrap()
}

#[test]
fn distinct_lots_retain_original_inputs_and_exact_current_pending_exposure() {
    let request = request();
    let output = run(&request);
    assert_eq!(output["ok"], "true", "{output}");
    let report = &output["result"]["data"];
    assert_eq!(
        report["exposure"]["problem"],
        request["operation"]["problem"]
    );
    // Each original reference1000 has risk1 in factor0. Exact signed sums:
    // current(1000+2000)/1000=3; pending(-500+1000)/1000=0.5.
    assert_eq!(report["exposure"]["current"], json!([3.0, 0.0, 0.0, 0.0]));
    assert_eq!(report["exposure"]["pending"], json!([0.5, 0.0, 0.0, 0.0]));
    assert_eq!(report["exposure"]["before"], json!([3.5, 0.0, 0.0, 0.0]));
    let mut old = request.clone();
    old["operation"]["action"] = json!({"kind":"solve"});
    let old: Value = serde_json::from_str(&execute_json(&old.to_string())).unwrap();
    assert_eq!(old["ok"], "false");
    assert!(old["error"]["message"]
        .as_str()
        .unwrap()
        .contains("duplicate"));
}

#[test]
fn security_holding_limits_cover_all_lots_and_cannot_be_bypassed_by_aliases() {
    let mut request = request();
    let candidate = &mut request["operation"]["problem"]["candidates"][0];
    candidate["trade_lower"] = json!("0");
    candidate["trade_upper"] = json!("0");
    candidate["holding_lower"] = json!("0");
    candidate["holding_upper"] = json!("3000");
    let output = run(&request);
    assert_eq!(output["ok"], "true", "{output}");
    assert_eq!(output["result"]["data"]["status"], "infeasible");
    assert!(output["result"]["data"]["proposal"].is_null());
}

#[test]
fn duplicate_lots_and_mismatched_full_reference_bindings_still_reject() {
    let initial = request();
    for (path, value) in [
        (
            "/operation/problem/holdings/1/risk/id",
            initial["operation"]["problem"]["holdings"][0]["risk"]["id"].clone(),
        ),
        (
            "/operation/problem/holdings/1/risk/economics/version",
            json!(99),
        ),
        (
            "/operation/problem/holdings/1/risk/calculation/version",
            json!(99),
        ),
        (
            "/operation/problem/holdings/1/risk/sensitivities/0/value",
            json!(2.0),
        ),
        (
            "/operation/problem/holdings/1/risk/reference_position/currency",
            json!("EUR"),
        ),
        (
            "/operation/problem/candidates/0/risk/id",
            json!("UNHELD.ALIAS"),
        ),
    ] {
        let mut value_request = initial.clone();
        *value_request.pointer_mut(path).unwrap() = value;
        assert_eq!(run(&value_request)["ok"], "false", "{path}");
    }
    let mut missing = initial;
    missing["operation"]["action"]
        .as_object_mut()
        .unwrap()
        .remove("scope");
    assert_eq!(run(&missing)["ok"], "false");
}

#[test]
fn lot_solver_retains_complete_issuer_scope_and_default_admission() {
    let mut request = request();
    let scope = json!({"reference":{"id":"supplied.scope","version":1},
        "snapshot_id":request["operation"]["problem"]["snapshot_id"],
        "directory":{"issuers":[{"entity_id":"ACME","version":1,"default":null}],
        "instruments":[{"id":"INSTRUMENT.HEDGE.0","version":2,"entity_id":"ACME"},
        {"id":"INSTRUMENT.HEDGE.1","version":2,"entity_id":"ACME"}]}});
    let plain = run(&request);
    request["operation"]["action"]["scope"] = scope.clone();
    let mut scoped = run(&request);
    assert_eq!(scoped["ok"], "true", "{scoped}");
    assert_eq!(scoped["result"]["data"]["issuer_scope"], scope);
    scoped["result"]["data"]
        .as_object_mut()
        .unwrap()
        .remove("issuer_scope");
    assert_eq!(scoped, plain);
    for (path, value) in [
        (
            "/operation/action/scope/snapshot_id",
            json!("WRONG.SNAPSHOT"),
        ),
        (
            "/operation/action/scope/directory/instruments/0/version",
            json!(99),
        ),
        ("/operation/action/scope/directory/instruments", json!([])),
        (
            "/operation/action/scope/directory/issuers/0/default",
            json!({
            "id":"actual.default","effective_date":"2025-01-01","known_at":"2025-01-01T00:00:00Z",
            "reference_id":"actual.source","reference_version":1}),
        ),
    ] {
        let mut invalid = request.clone();
        *invalid.pointer_mut(path).unwrap() = value;
        assert_eq!(run(&invalid)["ok"], "false", "{path}");
    }
}

#[cfg(feature = "schemars")]
#[test]
fn schema_requires_the_explicit_nullable_book_scope() {
    let schema =
        serde_json::to_value(schemars::schema_for!(convex::library::HedgeOperation)).unwrap();
    let book = schema["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|variant| variant["properties"]["kind"]["const"] == "solve_book")
        .unwrap();
    assert!(book["required"]
        .as_array()
        .unwrap()
        .contains(&json!("scope")));
    assert!(book["properties"]["scope"].to_string().contains("null"));
}
