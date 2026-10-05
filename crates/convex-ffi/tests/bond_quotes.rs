//! Real JSON/C ABI quote parity, including exact fixed-bond term consumption.
use std::ffi::{CStr, CString};

use convex_core::types::{Compounding, Date, Frequency, Mark};
use rust_decimal::Decimal;
use serde_json::{json, Value};

unsafe fn rpc(f: unsafe extern "C" fn(*const i8) -> *mut i8, request: Value) -> Value {
    let input = CString::new(request.to_string()).unwrap();
    let output = f(input.as_ptr());
    assert!(!output.is_null());
    let text = CStr::from_ptr(output).to_string_lossy().into_owned();
    convex_ffi::convex_string_free(output);
    serde_json::from_str(&text).unwrap()
}
unsafe fn build(f: unsafe extern "C" fn(*const i8) -> u64, spec: Value) -> u64 {
    let text = CString::new(spec.to_string()).unwrap();
    let handle = f(text.as_ptr());
    assert_ne!(handle, 0, "{spec}");
    handle
}
fn number(v: &Value) -> f64 {
    v.as_str().unwrap().parse().unwrap()
}
fn close(actual: f64, expected: f64, abs: f64) {
    assert!(
        (actual - expected).abs() <= abs + 1e-11 * expected.abs(),
        "{actual} != {expected}"
    );
}

#[test]
fn c_abi_prices_and_risk_match_all_supplied_contract_goldens() {
    let pack: Value = serde_json::from_str(include_str!(
        "../../convex-analytics/tests/fixtures/bond-quotes.json"
    ))
    .unwrap();
    for case in pack["cases"].as_array().unwrap() {
        unsafe {
            let bond = build(convex_ffi::convex_bond_from_json, case.clone());
            let frequency: Frequency =
                serde_json::from_value(case["quote_frequency"].clone()).unwrap();
            let mark = Mark::Yield {
                value: case["yield"].as_str().unwrap().parse::<Decimal>().unwrap(),
                frequency,
            };
            let request = json!({"bond":bond,"settlement":case["settlement"],"mark":mark,"quote_frequency":frequency});
            let price = rpc(convex_ffi::convex_price, request.clone());
            let risk = rpc(convex_ffi::convex_risk, request);
            assert_eq!(price["ok"], "true", "{} {price}", case["id"]);
            assert_eq!(risk["ok"], "true", "{} {risk}", case["id"]);
            for key in ["clean_price", "dirty_price", "accrued", "ytm_decimal"] {
                close(
                    price["result"][key].as_f64().unwrap(),
                    number(&case["expected"][key]),
                    if key == "ytm_decimal" { 1e-10 } else { 1e-9 },
                );
            }
            for key in [
                "modified_duration",
                "macaulay_duration",
                "convexity",
                "dv01",
            ] {
                close(
                    risk["result"][key].as_f64().unwrap(),
                    number(&case["expected"][key]),
                    1e-9,
                );
            }
            let inverse = rpc(
                convex_ffi::convex_price,
                json!({"bond":bond,"settlement":case["settlement"],"mark":format!("{}C",case["expected"]["clean_price"].as_str().unwrap()),"quote_frequency":frequency}),
            );
            assert_eq!(inverse["ok"], "true", "{inverse}");
            close(
                inverse["result"]["ytm_decimal"].as_f64().unwrap(),
                number(&case["yield"]),
                1e-10,
            );
            convex_ffi::convex_release(bond);
        }
    }
}

fn fixed() -> Value {
    json!({"type":"fixed_rate","name":"SUPPLIED-ABI-017","issue":"2024-01-15","maturity":"2029-01-15","coupon_rate":"0.05","frequency":"Annual","day_count":"Thirty360US","currency":"USD","face_value":"100","business_day_convention":"Unadjusted","terms":{"dated_date":"2024-01-15","first_coupon_date":null,"penultimate_coupon_date":null,"calendar":"WEEKEND","end_of_month":false,"settlement_days":0,"redemption_value":"100","ex_coupon":null}})
}
fn curve(rate: f64, owner: &str) -> Value {
    json!({"type":"discrete","name":format!("SUPPLIED-ABI-CURVE-{owner}-{rate}"),"ref_date":"2025-01-15","tenors":[0.01,1.0,5.0,10.0],"values":[rate,rate,rate,rate],"value_kind":"zero_rate","compounding":"Continuous","day_count":"Act365Fixed","interpolation":"linear"})
}

#[test]
fn c_abi_curve_spreads_preserve_frequency_and_actual_benchmarks() {
    use convex_analytics::spreads::{GovernmentBenchmark, Sovereign};
    use convex_bonds::types::Tenor;
    use convex_core::types::Yield;
    unsafe {
        let bond = build(convex_ffi::convex_bond_from_json, fixed());
        let handle = build(convex_ffi::convex_curve_from_json, curve(0.035, "spreads"));
        let benchmark = GovernmentBenchmark::with_cusip_unchecked(
            Sovereign::UST,
            Tenor::Y5,
            "SUPPLIED-YIELD",
            Date::parse("2029-01-15").unwrap(),
            Decimal::new(4, 2),
            Yield::new(Decimal::new(4, 2), Compounding::SemiAnnual),
        );
        let request = json!({"bond":bond,"curve":handle,"settlement":"2025-01-15","mark":"5%@A","spread_type":"GSpread","params":{"quote_frequency":"Annual","government":{"reference_date":"2025-01-15","sovereign":"UST","benchmarks":[benchmark]}}});
        let result = rpc(convex_ffi::convex_spread, request.clone());
        assert_eq!(result["ok"], "true", "{result}");
        // The annual 5% yield converts to semiannual before subtracting 4%.
        let expected = ((2.0 * (1.05_f64.sqrt() - 1.0) - 0.04) * 10000.0).round();
        close(
            result["result"]["spread_bps"].as_f64().unwrap(),
            expected,
            1e-10,
        );
        for government in [
            Value::Null,
            json!({"reference_date":"2025-01-15","sovereign":"UST","benchmarks":[]}),
            json!({"reference_date":"2025-01-15","sovereign":"UK","benchmarks":[benchmark]}),
        ] {
            let mut bad = request.clone();
            bad["params"]["government"] = government;
            assert_eq!(rpc(convex_ffi::convex_spread, bad)["ok"], "false");
        }
        let mut bad = request;
        bad["params"]["govt_curve"] = json!(handle);
        assert_eq!(rpc(convex_ffi::convex_spread, bad)["ok"], "false");
        let no_curve = rpc(
            convex_ffi::convex_price,
            json!({"bond":bond,"settlement":"2025-01-15","mark":"50 Z@SUPPLIED"}),
        );
        assert_eq!(no_curve["ok"], "false");
        let krd = rpc(
            convex_ffi::convex_risk,
            json!({"bond":bond,"curve":handle,"settlement":"2025-01-15","mark":"100C","quote_frequency":"Annual","key_rate_tenors":[1.0,2.0,5.0,10.0]}),
        );
        assert_eq!(krd["ok"], "true", "{krd}");
        assert_eq!(krd["result"]["key_rates"].as_array().unwrap().len(), 4);
        convex_ffi::convex_release(bond);
        convex_ffi::convex_release(handle);
    }
}

#[test]
fn c_abi_frn_never_uses_coupon_spread_as_an_unobserved_fixing() {
    unsafe {
        let bond = build(
            convex_ffi::convex_bond_from_json,
            json!({"type":"floating_rate","name":"FRN-ABSENT-FIXINGS","spread_bps":"75","maturity":"2029-01-15","issue":"2024-01-15","rate_index":"sofr","frequency":"Quarterly","day_count":"Act360"}),
        );
        let handle = build(convex_ffi::convex_curve_from_json, curve(0.035, "frn"));
        for f in [
            convex_ffi::convex_price,
            convex_ffi::convex_risk,
            convex_ffi::convex_cashflows,
        ] {
            let result = rpc(
                f,
                json!({"bond":bond,"curve":handle,"forward_curve":handle,"settlement":"2025-01-15","mark":"100C"}),
            );
            assert_eq!(result["ok"], "false", "{result}");
            assert_eq!(result["error"]["field"], "fixings");
        }
        let result = rpc(
            convex_ffi::convex_spread,
            json!({"bond":bond,"curve":handle,"settlement":"2025-01-15","mark":"100C","spread_type":"DiscountMargin","params":{"forward_curve":handle,"current_index":0.04}}),
        );
        assert_eq!(result["ok"], "false", "{result}");
        assert_eq!(result["error"]["field"], "fixings");
        convex_ffi::convex_release(bond);
        convex_ffi::convex_release(handle);
    }
}

#[test]
fn zero_icma_without_quasi_coupon_references_is_not_a_context_free_day_count() {
    unsafe {
        let bond = build(
            convex_ffi::convex_bond_from_json,
            json!({"type":"zero_coupon","cusip":"ZERO-NO-ICMA-REFERENCE","issue":"2024-01-15","maturity":"2029-01-15","day_count":"ActActIcma","compounding":"SemiAnnual","face_value":"100","currency":"USD"}),
        );
        let result = rpc(
            convex_ffi::convex_price,
            json!({"bond":bond,"settlement":"2025-01-15","mark":"99C"}),
        );
        assert_eq!(result["ok"], "false", "{result}");
        assert!(result["error"]["message"]
            .as_str()
            .unwrap()
            .contains("year fraction"));
        convex_ffi::convex_release(bond);
    }
}

#[test]
fn c_abi_zero_redemption_and_mark_frequency_match_analytic_contract() {
    unsafe {
        let bond = build(
            convex_ffi::convex_bond_from_json,
            json!({"type":"zero_coupon","cusip":"SUPPLIED-ZERO-PREMIUM","issue":"2024-01-15","maturity":"2029-01-15","currency":"EUR","face_value":"1000","redemption_value":"1050","day_count":"Act360","compounding":"SemiAnnual"}),
        );
        let request = json!({"bond":bond,"settlement":"2025-01-15","mark":"4%@A"});
        let price = rpc(convex_ffi::convex_price, request.clone());
        let risk = rpc(convex_ffi::convex_risk, request);
        assert_eq!(price["ok"], "true", "{price}");
        assert_eq!(risk["ok"], "true", "{risk}");
        let time = 1461.0 / 360.0;
        let y = 2.0 * (1.04_f64.sqrt() - 1.0);
        let p = 105.0 * 1.04_f64.powf(-time);
        close(price["result"]["clean_price"].as_f64().unwrap(), p, 1e-10);
        close(price["result"]["ytm_decimal"].as_f64().unwrap(), y, 1e-12);
        close(
            risk["result"]["modified_duration"].as_f64().unwrap(),
            time / (1.0 + y / 2.0),
            1e-10,
        );
        close(
            risk["result"]["convexity"].as_f64().unwrap(),
            time * (time + 0.5) / (1.0 + y / 2.0).powi(2),
            1e-10,
        );
        convex_ffi::convex_release(bond);
    }
}
