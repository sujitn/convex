//! Actual C quote admission, RPC field ownership and generated schema coverage.

use convex_ffi::*;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::sync::Mutex;

static OWN_CALLS: Mutex<()> = Mutex::new(());

type Rpc = unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_char;

fn call(function: Rpc, text: &str) -> Value {
    let input = CString::new(text).unwrap();
    // SAFETY: the input owns its immutable terminated bytes for the entire call.
    let output = unsafe { function(input.as_ptr()) };
    assert!(!output.is_null());
    // SAFETY: the native function returns an owned terminated output allocation.
    let bytes = unsafe { CStr::from_ptr(output) }.to_bytes().to_vec();
    // SAFETY: release this output exactly once after copying it.
    unsafe { convex_string_free(output) };
    serde_json::from_slice(&bytes).unwrap()
}

struct OwnedHandle(u64);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        convex_release(self.0);
    }
}

fn construct(request: &Value, curve: bool) -> OwnedHandle {
    let input = CString::new(request.to_string()).unwrap();
    // SAFETY: input owns valid terminated bytes for the complete constructor call.
    let handle = unsafe {
        if curve {
            convex_curve_from_json(input.as_ptr())
        } else {
            convex_bond_from_json(input.as_ptr())
        }
    };
    assert_ne!(handle, 0);
    OwnedHandle(handle)
}

#[test]
fn original_complete_quotes_and_rpc_inputs_conserve_precision_and_field_ownership() {
    let _guard = OWN_CALLS.lock().unwrap();
    let fixture: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/quote-admission.json")).unwrap();
    for row in fixture["mark_cases"].as_array().unwrap() {
        let envelope = call(convex_mark_parse, row["text"].as_str().unwrap());
        if row["expected"] == "rejected" {
            assert_eq!(envelope["ok"], "false", "{row}: {envelope}");
            assert_eq!(envelope["error"]["code"], "invalid_input");
        } else {
            assert_eq!(envelope["ok"], "true", "{row}: {envelope}");
            let result = &envelope["result"];
            let value = if result["mark"] == "spread" {
                &result["value"]["value_bps"]
            } else {
                &result["value"]
            };
            let text = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            assert_eq!(
                Decimal::from_str_exact(&text).unwrap(),
                Decimal::from_str_exact(row["exact_decimal"].as_str().unwrap()).unwrap()
            );
        }
        println!(
            "{}",
            json!({"group":"mark","id":row["id"],"actual":envelope})
        );
    }
    let bond = construct(&fixture["bond_constructor"], false);
    let curve = construct(&fixture["curve_constructor"], true);
    for row in fixture["RPC_cases"].as_array().unwrap() {
        let operation = row["operation"].as_str().unwrap();
        let mut request = if operation == "curve_query" {
            json!({"curve":curve.0,"query":row["query"],"tenor":row["tenor"]})
        } else {
            json!({"bond":bond.0,"settlement":"2025-04-15","mark":row["mark"]})
        };
        if let Some(extra) = row.get("extra").and_then(Value::as_object) {
            request.as_object_mut().unwrap().extend(extra.clone());
        }
        let function: Rpc = match operation {
            "price" => convex_price,
            "risk" => convex_risk,
            "curve_query" => convex_curve_query,
            _ => panic!("unknown declared operation"),
        };
        let envelope = call(function, &request.to_string());
        assert_eq!(
            envelope["ok"] == "true",
            row["expected"] == "accepted",
            "{row}: {envelope}"
        );
        println!(
            "{}",
            json!({"group":"RPC","id":row["id"],"actual":envelope})
        );
    }
    for row in fixture["curve_cases"].as_array().unwrap() {
        let mut request = fixture["curve_constructor"].clone();
        let rate: f64 = row["rate"].as_str().unwrap().parse().unwrap();
        request["values"] = json!([rate, rate]);
        let input = CString::new(request.to_string()).unwrap();
        // SAFETY: the terminated allocation remains owned through this call.
        let handle = unsafe { convex_curve_from_json(input.as_ptr()) };
        if handle != 0 {
            let handle = OwnedHandle(handle);
            let envelope = call(
                convex_curve_query,
                &json!({"curve":handle.0,"query":"df","tenor":1}).to_string(),
            );
            assert_eq!(envelope["ok"], "false", "{row}: {envelope}");
            println!(
                "{}",
                json!({"group":"curve","id":row["id"],"actual":envelope})
            );
        }
    }
}

#[test]
fn every_returned_convenience_schema_is_generated_and_resolves_its_local_references() {
    let _guard = OWN_CALLS.lock().unwrap();
    fn visit(node: &Value, root: &Value) {
        match node {
            Value::Object(fields) => {
                if let Some(reference) = fields.get("$ref").and_then(Value::as_str) {
                    assert!(reference.starts_with("#/"), "{reference}");
                    assert!(root.pointer(&reference[1..]).is_some(), "{reference}");
                }
                for value in fields.values() {
                    visit(value, root);
                }
            }
            Value::Array(values) => {
                for value in values {
                    visit(value, root);
                }
            }
            _ => {}
        }
    }
    for name in [
        "BondSpec",
        "CurveSpec",
        "Mark",
        "PricingRequest",
        "PricingResponse",
        "RiskRequest",
        "RiskResponse",
        "SpreadRequest",
        "SpreadResponse",
        "CashflowRequest",
        "CashflowResponse",
        "CurveQueryRequest",
        "CurveQueryResponse",
        "MakeWholeRequest",
        "MakeWholeResponse",
        "YasRequest",
        "YasResponse",
        "ScenarioRequest",
        "ScenarioResponse",
        "RiskProfileRequest",
        "RiskProfile",
        "HedgeRequest",
        "HedgeProposal",
        "CompareRequest",
        "CompareResponse",
    ] {
        let envelope = call(convex_schema, name);
        assert_eq!(envelope["ok"], "true", "{name}: {envelope}");
        let schema = &envelope["result"];
        assert_eq!(schema["title"], name);
        assert!(schema["$schema"].as_str().is_some());
        visit(schema, schema);
        println!("{}", json!({"schema_name":name,"schema":schema}));
    }
}

#[test]
fn native_tagged_constructors_reject_unknown_and_duplicate_fields() {
    use convex_analytics::dto::{BondSpec, CurveSpec};
    let coupon = json!({"coupon_rate":"0.05","maturity":"2035-01-15","issue":"2025-01-15"});
    for kind in [
        "fixed_rate",
        "callable",
        "floating_rate",
        "zero_coupon",
        "sinking_fund",
    ] {
        let mut request = if kind == "floating_rate" {
            json!({"spread_bps":125,"maturity":"2035-01-15","issue":"2025-01-15"})
        } else if kind == "zero_coupon" {
            json!({"maturity":"2035-01-15","issue":"2025-01-15"})
        } else {
            coupon.clone()
        };
        request["type"] = json!(kind);
        if kind == "callable" {
            request["call_schedule"] = json!([]);
        }
        if kind == "sinking_fund" {
            request["schedule"] = json!([]);
        }
        let healthy: BondSpec = serde_json::from_value(request.clone()).unwrap();
        println!(
            "{}",
            json!({"schema_case":"BondSpec","input":request,"serialized":healthy})
        );
        request["principal_steps"] = json!([]);
        assert!(
            serde_json::from_value::<BondSpec>(request.clone()).is_err(),
            "{request}"
        );
        println!("{}", json!({"schema_rejection":"BondSpec","input":request}));
    }
    for request in [
        json!({"type":"discrete","ref_date":"2025-01-15","tenors":[1,10],"values":[0.04,0.04]}),
        json!({"type":"bootstrap","ref_date":"2025-01-15","instruments":[{"kind":"deposit","tenor":1,"rate":0.04}]}),
    ] {
        let healthy: CurveSpec = serde_json::from_value(request.clone()).unwrap();
        println!(
            "{}",
            json!({"schema_case":"CurveSpec","input":request,"serialized":healthy})
        );
        let mut bad = request;
        bad["recovery"] = json!(0.4);
        assert!(serde_json::from_value::<CurveSpec>(bad.clone()).is_err());
        println!("{}", json!({"schema_rejection":"CurveSpec","input":bad}));
    }
    for request in [
        r#"{"type":"fixed_rate","coupon_rate":0.05,"coupon_rate":0.06,"maturity":"2035-01-15","issue":"2025-01-15"}"#,
        r#"{"type":"callable","coupon_rate":0.05,"maturity":"2035-01-15","issue":"2025-01-15","call_schedule":[{"date":"2030-01-15","price":100,"price":101}]}"#,
    ] {
        assert!(serde_json::from_str::<BondSpec>(request).is_err());
    }
    for request in [
        r#"{"type":"discrete","ref_date":"2025-01-15","tenors":[1,10],"values":[0.04,0.04],"values":[0.05,0.05]}"#,
        r#"{"type":"bootstrap","ref_date":"2025-01-15","instruments":[{"kind":"deposit","tenor":1,"rate":0.04,"rate":0.05}]}"#,
    ] {
        assert!(serde_json::from_str::<CurveSpec>(request).is_err());
    }
}

#[test]
fn registered_names_match_handles_at_every_declared_rpc_reference_path() {
    let _guard = OWN_CALLS.lock().unwrap();
    let fixture: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/quote-admission.json")).unwrap();
    let bond = construct(&fixture["bond_constructor"], false);
    let curve = construct(&fixture["curve_constructor"], true);
    let mut callable_request = fixture["bond_constructor"].clone();
    callable_request["type"] = json!("callable");
    callable_request["name"] = json!("following-supplied-callable-quote-book");
    callable_request["call_schedule"] = json!([{"date":"2030-01-15","price":100}]);
    callable_request["call_style"] = json!("make_whole");
    callable_request["make_whole_spread_bps"] = json!(25);
    let callable = construct(&callable_request, false);
    let position = json!({"currency":"USD","settlement":"2025-04-15","notional_face":"1000000","clean_price_per_100":100,"dirty_price_per_100":101.25,"accrued_per_100":1.25,"market_value":"1012500","ytm_decimal":0.05,"modified_duration_years":7,"macaulay_duration_years":7.175,"convexity":50,"dv01":700});
    let basic = json!({"bond":bond.0,"curve":curve.0,"settlement":"2025-04-15","mark":"5%"});
    let mut pricing = basic.clone();
    pricing["forward_curve"] = json!(curve.0);
    let mut spread = basic.clone();
    spread["spread_type"] = json!("ZSpread");
    spread["params"] = json!({"forward_curve":curve.0});
    let mut profile = basic.clone();
    profile["notional_face"] = json!("1000000");
    let mut yas = basic.clone();
    yas["swap_curve"] = json!(curve.0);
    let mut scenarios = basic;
    scenarios["scenarios"] = json!([]);
    let requests: [(&str, Rpc, Value, &[&str]); 10] = [
        (
            "PricingRequest",
            convex_price,
            pricing.clone(),
            &["/bond", "/curve", "/forward_curve"],
        ),
        (
            "RiskRequest",
            convex_risk,
            pricing,
            &["/bond", "/curve", "/forward_curve"],
        ),
        (
            "SpreadRequest",
            convex_spread,
            spread,
            &["/bond", "/curve", "/params/forward_curve"],
        ),
        (
            "CashflowRequest",
            convex_cashflows,
            json!({"bond":bond.0,"settlement":"2025-04-15"}),
            &["/bond"],
        ),
        (
            "CurveQueryRequest",
            convex_curve_query,
            json!({"curve":curve.0,"query":"df","tenor":1}),
            &["/curve"],
        ),
        (
            "MakeWholeRequest",
            convex_make_whole,
            json!({"bond":callable.0,"call_date":"2030-01-15","treasury_rate":0.04}),
            &["/bond"],
        ),
        (
            "RiskProfileRequest",
            convex_risk_profile,
            profile,
            &["/bond", "/curve"],
        ),
        (
            "HedgeRequest",
            convex_hedge,
            json!({"strategy":"cash_bond_pair","position":position,"curve":curve.0,"settlement":"2025-04-15"}),
            &["/curve"],
        ),
        (
            "YasRequest",
            convex_yas,
            yas,
            &["/bond", "/curve", "/swap_curve"],
        ),
        (
            "ScenarioRequest",
            convex_scenario,
            scenarios,
            &["/bond", "/curve"],
        ),
    ];
    let mut checked = 0;
    for (schema, function, request, paths) in requests {
        let numeric = call(function, &request.to_string());
        for path in paths {
            let mut named = request.clone();
            let name = if *path == "/bond" {
                if request["bond"] == callable.0 {
                    callable_request["name"].clone()
                } else {
                    fixture["bond_constructor"]["name"].clone()
                }
            } else {
                fixture["curve_constructor"]["name"].clone()
            };
            *named.pointer_mut(path).unwrap() = name;
            let actual = call(function, &named.to_string());
            assert_eq!(actual, numeric, "{schema}: {path}");
            assert_ne!(
                actual["error"]["code"], "invalid_handle",
                "{schema}: {path}"
            );
            println!(
                "{}",
                json!({"schema_case":schema,"input":named,"reference_path":path,"actual":actual})
            );
            *named.pointer_mut(path).unwrap() = json!("unregistered-quote-wire-control");
            let rejected = call(function, &named.to_string());
            assert_eq!(
                rejected["error"]["code"], "invalid_handle",
                "{schema}: {path}: {rejected}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 20);
}

#[test]
fn original_supplied_description_amounts_retain_every_decimal_digit() {
    let _guard = OWN_CALLS.lock().unwrap();
    let reference: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/quote-description.json"
    ))
    .unwrap();
    for row in reference["cases"].as_array().unwrap() {
        let handle = construct(&row["constructor"], false);
        // SAFETY: this test owns a live handle. Copy and release the returned
        // allocation exactly once while retaining the handle until after use.
        let envelope: Value = unsafe {
            let pointer = convex_describe(handle.0);
            assert!(!pointer.is_null());
            let bytes = CStr::from_ptr(pointer).to_bytes().to_vec();
            convex_string_free(pointer);
            serde_json::from_slice(&bytes).unwrap()
        };
        assert_eq!(envelope["ok"], "true");
        let value = &envelope["result"][row["field"].as_str().unwrap()];
        let text = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        assert_eq!(
            Decimal::from_str_exact(&text).unwrap(),
            Decimal::from_str_exact(row["exact_decimal"].as_str().unwrap()).unwrap(),
            "{}",
            row["id"]
        );
        println!(
            "{}",
            json!({"description_id":row["id"],"actual":envelope,"exact_decimal":row["exact_decimal"]})
        );
    }
}
