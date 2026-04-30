//! End-to-end FFI smoke tests.
//!
//! Goes through the same code paths an Excel call would: build a JSON
//! `BondSpec`, call `convex_bond_from_json`, build a JSON `PricingRequest`
//! with a textual mark, call `convex_price`, parse the envelope.

use std::ffi::{CStr, CString};

use serde_json::{json, Value};

unsafe fn rpc(f: unsafe extern "C" fn(*const i8) -> *mut i8, req: &str) -> Value {
    let req_c = CString::new(req).unwrap();
    let resp_ptr = f(req_c.as_ptr());
    assert!(!resp_ptr.is_null(), "FFI returned null pointer");
    let resp = CStr::from_ptr(resp_ptr).to_string_lossy().into_owned();
    convex_ffi::convex_string_free(resp_ptr);
    serde_json::from_str(&resp).unwrap_or_else(|e| panic!("invalid JSON {resp:?}: {e}"))
}

unsafe fn build_handle(spec: Value) -> u64 {
    let s = CString::new(spec.to_string()).unwrap();
    let handle = convex_ffi::convex_bond_from_json(s.as_ptr());
    if handle == convex_ffi::INVALID_HANDLE {
        let err_ptr = convex_ffi::convex_last_error();
        let err = if err_ptr.is_null() {
            "<no error message>".to_string()
        } else {
            CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
        };
        panic!("bond_from_json failed: {err}");
    }
    handle
}

fn fixed_rate_5pct() -> Value {
    json!({
        "type": "fixed_rate",
        "cusip": "TEST10Y5",
        "coupon_rate": 0.05,
        "frequency": "SemiAnnual",
        "maturity": "2035-01-15",
        "issue": "2025-01-15",
        "day_count": "Thirty360US",
        "currency": "USD",
        "face_value": 100
    })
}

#[test]
fn build_fixed_rate_bond_with_free_form_name() {
    // 8-char id like "FIXED-5Y" routes to BondIdentifier::name (not CUSIP, not ISIN).
    // Regression: the bond's own `identifiers` invariant has to be satisfied
    // even when the user passed a name only — this used to error with
    // "Missing required field: identifiers".
    unsafe {
        let spec = json!({
            "type": "fixed_rate",
            "name": "FIXED-5Y",
            "coupon_rate": 0.045,
            "frequency": "SemiAnnual",
            "maturity": "2030-01-15",
            "issue":    "2025-01-15",
            "day_count": "Thirty360US",
            "currency": "USD",
            "face_value": 100,
        });
        let h = build_handle(spec);
        assert!(h >= 100, "handle = {h}");
    }
}

#[test]
fn build_fixed_rate_bond_returns_handle() {
    unsafe {
        let h = build_handle(fixed_rate_5pct());
        assert!(h >= 100, "handle should be ≥100 (got {h})");
    }
}

#[test]
fn price_with_clean_text_mark() {
    unsafe {
        let bond = build_handle(fixed_rate_5pct());
        let req = json!({
            "bond": bond,
            "settlement": "2025-04-15",
            "mark": "99.5C",
            "quote_frequency": "SemiAnnual"
        });
        let resp = rpc(convex_ffi::convex_price, &req.to_string());
        assert_eq!(resp["ok"], "true", "response: {resp}");
        let r = &resp["result"];
        let clean = r["clean_price"].as_f64().unwrap();
        let dirty = r["dirty_price"].as_f64().unwrap();
        let accrued = r["accrued"].as_f64().unwrap();
        let ytm = r["ytm_decimal"].as_f64().unwrap();
        assert!((clean - 99.5).abs() < 1e-9, "clean = {clean}");
        assert!((dirty - clean - accrued).abs() < 1e-9);
        assert!(ytm > 0.04 && ytm < 0.06, "ytm = {ytm}");
    }
}

#[test]
fn price_with_yield_text_mark_inverts() {
    unsafe {
        let bond = build_handle(fixed_rate_5pct());
        // Step 1: price from clean = 99.5
        let r1 = rpc(
            convex_ffi::convex_price,
            &json!({
                "bond": bond,
                "settlement": "2025-04-15",
                "mark": "99.5C",
                "quote_frequency": "SemiAnnual"
            })
            .to_string(),
        );
        let ytm1 = r1["result"]["ytm_decimal"].as_f64().unwrap();

        // Step 2: feed that yield back as a yield mark
        let yield_pct = format!("{:.6}%", ytm1 * 100.0);
        let r2 = rpc(
            convex_ffi::convex_price,
            &json!({
                "bond": bond,
                "settlement": "2025-04-15",
                "mark": yield_pct,
                "quote_frequency": "SemiAnnual"
            })
            .to_string(),
        );
        let clean2 = r2["result"]["clean_price"].as_f64().unwrap();
        assert!((clean2 - 99.5).abs() < 1e-4, "clean2 = {clean2}");
    }
}

#[test]
fn risk_metrics_are_present() {
    unsafe {
        let bond = build_handle(fixed_rate_5pct());
        let resp = rpc(
            convex_ffi::convex_risk,
            &json!({
                "bond": bond,
                "settlement": "2025-04-15",
                "mark": "99.5C",
                "quote_frequency": "SemiAnnual"
            })
            .to_string(),
        );
        assert_eq!(resp["ok"], "true", "response: {resp}");
        let r = &resp["result"];
        assert!(r["modified_duration"].as_f64().unwrap() > 0.0);
        assert!(r["macaulay_duration"].as_f64().unwrap() > 0.0);
        assert!(r["dv01"].as_f64().unwrap() > 0.0);
    }
}

#[test]
fn cashflows_are_returned() {
    unsafe {
        let bond = build_handle(fixed_rate_5pct());
        let resp = rpc(
            convex_ffi::convex_cashflows,
            &json!({"bond": bond, "settlement": "2025-04-15"}).to_string(),
        );
        assert_eq!(resp["ok"], "true");
        let flows = resp["result"]["flows"].as_array().unwrap();
        assert!(
            flows.len() >= 19,
            "expected ≥19 semi-annual flows, got {}",
            flows.len()
        );
    }
}

#[test]
fn invalid_handle_returns_error_envelope() {
    unsafe {
        let resp = rpc(
            convex_ffi::convex_price,
            &json!({
                "bond": 99999u64,
                "settlement": "2025-04-15",
                "mark": "99.5C"
            })
            .to_string(),
        );
        assert_eq!(resp["ok"], "false");
        assert_eq!(resp["error"]["code"], "invalid_handle");
    }
}

#[test]
fn schema_introspection() {
    unsafe {
        let name = CString::new("PricingRequest").unwrap();
        let ptr = convex_ffi::convex_schema(name.as_ptr());
        assert!(!ptr.is_null());
        let resp = CStr::from_ptr(ptr).to_string_lossy().into_owned();
        convex_ffi::convex_string_free(ptr);
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["ok"], "true");
        // `result` is the schema object (it parses as JSON before reaching us).
        assert_eq!(v["result"]["title"], "PricingRequest");
    }
}

unsafe fn build_curve(spec: Value) -> u64 {
    let s = CString::new(spec.to_string()).unwrap();
    let h = convex_ffi::convex_curve_from_json(s.as_ptr());
    if h == convex_ffi::INVALID_HANDLE {
        let err_ptr = convex_ffi::convex_last_error();
        let err = if err_ptr.is_null() {
            "<no error>".to_string()
        } else {
            CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
        };
        panic!("curve_from_json failed: {err}");
    }
    h
}

fn flat_curve(rate: f64) -> Value {
    json!({
        "type": "discrete",
        "ref_date": "2025-01-15",
        "tenors": [0.5, 1.0, 2.0, 5.0, 10.0, 30.0],
        "values": [rate, rate, rate, rate, rate, rate],
        "value_kind": "zero_rate",
        "interpolation": "linear",
        "day_count": "Act365Fixed",
        "compounding": "Continuous"
    })
}

#[test]
fn z_spread_returns_bps_and_dv01() {
    unsafe {
        let bond = build_handle(fixed_rate_5pct());
        let curve = build_curve(flat_curve(0.04));
        let resp = rpc(
            convex_ffi::convex_spread,
            &json!({
                "bond": bond,
                "curve": curve,
                "settlement": "2025-04-15",
                "mark": "99.5C",
                "spread_type": "ZSpread",
            })
            .to_string(),
        );
        assert_eq!(resp["ok"], "true", "response: {resp}");
        let r = &resp["result"];
        let bps = r["spread_bps"].as_f64().unwrap();
        assert!(bps.abs() < 500.0, "z bps = {bps}");
        assert!(r["spread_dv01"].as_f64().is_some());
    }
}

#[test]
fn i_spread_works_against_swap_curve() {
    unsafe {
        let bond = build_handle(fixed_rate_5pct());
        let curve = build_curve(flat_curve(0.04));
        let resp = rpc(
            convex_ffi::convex_spread,
            &json!({
                "bond": bond,
                "curve": curve,
                "settlement": "2025-04-15",
                "mark": "99.5C",
                "spread_type": "ISpread",
            })
            .to_string(),
        );
        assert_eq!(resp["ok"], "true", "response: {resp}");
        assert!(resp["result"]["spread_bps"].as_f64().is_some());
    }
}

#[test]
fn g_spread_requires_separate_gov_curve() {
    unsafe {
        let bond = build_handle(fixed_rate_5pct());
        let swap = build_curve(flat_curve(0.04));
        // Without params.govt_curve we should refuse rather than fabricate.
        let no_govt = rpc(
            convex_ffi::convex_spread,
            &json!({
                "bond": bond, "curve": swap, "settlement": "2025-04-15",
                "mark": "99.5C", "spread_type": "GSpread",
            })
            .to_string(),
        );
        assert_eq!(no_govt["ok"], "false");
        assert_eq!(no_govt["error"]["field"], "params.govt_curve");

        // With a separate gov curve at 3.5%, G-spread on a bond priced
        // against the 4% swap curve should be roughly +50bps.
        let gov = build_curve(flat_curve(0.035));
        let resp = rpc(
            convex_ffi::convex_spread,
            &json!({
                "bond": bond, "curve": swap, "settlement": "2025-04-15",
                "mark": "99.5C", "spread_type": "GSpread",
                "params": {"govt_curve": gov},
            })
            .to_string(),
        );
        assert_eq!(resp["ok"], "true", "{resp}");
        let bps = resp["result"]["spread_bps"].as_f64().unwrap();
        assert!(bps > 0.0 && bps.is_finite(), "g-spread bps = {bps}");
    }
}

#[test]
fn asw_par_returns_a_finite_spread() {
    unsafe {
        let bond = build_handle(fixed_rate_5pct());
        let curve = build_curve(flat_curve(0.04));
        let resp = rpc(
            convex_ffi::convex_spread,
            &json!({
                "bond": bond,
                "curve": curve,
                "settlement": "2025-04-15",
                "mark": "99.5C",
                "spread_type": "AssetSwapPar",
            })
            .to_string(),
        );
        assert_eq!(resp["ok"], "true", "response: {resp}");
        assert!(resp["result"]["spread_bps"].as_f64().unwrap().is_finite());
    }
}

fn zero_coupon_5y() -> Value {
    json!({
        "type": "zero_coupon",
        "cusip": "TESTZ0005",
        "maturity": "2030-01-15",
        "issue": "2025-01-15",
        "compounding": "SemiAnnual",
        "day_count": "ActActIcma"
    })
}

#[test]
fn zero_price_yield_round_trip() {
    unsafe {
        let bond = build_handle(zero_coupon_5y());
        let r1 = rpc(
            convex_ffi::convex_price,
            &json!({"bond": bond, "settlement": "2025-04-15", "mark": "85"}).to_string(),
        );
        assert_eq!(r1["ok"], "true", "{r1}");
        let ytm = r1["result"]["ytm_decimal"].as_f64().unwrap();
        assert!(ytm > 0.0 && ytm < 0.10);

        let yield_pct = format!("{:.6}%", ytm * 100.0);
        let r2 = rpc(
            convex_ffi::convex_price,
            &json!({"bond": bond, "settlement": "2025-04-15", "mark": yield_pct}).to_string(),
        );
        let p = r2["result"]["clean_price"].as_f64().unwrap();
        assert!((p - 85.0).abs() < 1e-3);
    }
}

#[test]
fn zero_risk_closed_form() {
    unsafe {
        let bond = build_handle(zero_coupon_5y());
        let r = rpc(
            convex_ffi::convex_risk,
            &json!({"bond": bond, "settlement": "2025-04-15", "mark": "85"}).to_string(),
        );
        assert_eq!(r["ok"], "true", "{r}");
        let mac = r["result"]["macaulay_duration"].as_f64().unwrap();
        // From 2025-04-15 to 2030-01-15 ≈ 4.75 years.
        assert!((mac - 4.75).abs() < 0.05, "macaulay = {mac}");
    }
}

fn frn_5y() -> Value {
    json!({
        "type": "floating_rate",
        "cusip": "TESTFR005",
        "spread_bps": 75,
        "maturity": "2030-01-15",
        "issue": "2025-01-15",
        "rate_index": "sofr",
        "frequency": "Quarterly",
        "day_count": "Act360"
    })
}

#[test]
fn frn_price_with_clean_mark() {
    unsafe {
        let bond = build_handle(frn_5y());
        let r = rpc(
            convex_ffi::convex_price,
            &json!({"bond": bond, "settlement": "2025-04-15", "mark": "100"}).to_string(),
        );
        assert_eq!(r["ok"], "true", "{r}");
        let dirty = r["result"]["dirty_price"].as_f64().unwrap();
        let clean = r["result"]["clean_price"].as_f64().unwrap();
        let accrued = r["result"]["accrued"].as_f64().unwrap();
        assert!((dirty - clean - accrued).abs() < 1e-9);
    }
}

#[test]
fn frn_yield_mark_rejected() {
    unsafe {
        let bond = build_handle(frn_5y());
        let r = rpc(
            convex_ffi::convex_price,
            &json!({"bond": bond, "settlement": "2025-04-15", "mark": "5%"}).to_string(),
        );
        assert_eq!(r["ok"], "false");
    }
}

fn sinking_fund_10y() -> Value {
    json!({
        "type": "sinking_fund",
        "cusip": "TESTSF010",
        "coupon_rate": 0.05,
        "frequency": "SemiAnnual",
        "maturity": "2035-01-15",
        "issue": "2025-01-15",
        "day_count": "Thirty360US",
        "schedule": [
            {"date": "2031-01-15", "amount": 20.0, "price": 100.0},
            {"date": "2032-01-15", "amount": 20.0, "price": 100.0},
            {"date": "2033-01-15", "amount": 20.0, "price": 100.0},
            {"date": "2034-01-15", "amount": 20.0, "price": 100.0}
        ]
    })
}

#[test]
fn sinking_fund_builds_and_prices() {
    unsafe {
        let bond = build_handle(sinking_fund_10y());
        let r = rpc(
            convex_ffi::convex_price,
            &json!({"bond": bond, "settlement": "2025-04-15", "mark": "99.5C"}).to_string(),
        );
        assert_eq!(r["ok"], "true", "{r}");
        assert!(r["result"]["ytm_decimal"].as_f64().unwrap() > 0.0);
    }
}

#[test]
fn krd_returns_one_entry_per_tenor() {
    unsafe {
        let bond = build_handle(fixed_rate_5pct());
        let curve = build_curve(flat_curve(0.04));
        let resp = rpc(
            convex_ffi::convex_risk,
            &json!({
                "bond": bond,
                "curve": curve,
                "settlement": "2025-04-15",
                "mark": "99.5C",
                "key_rate_tenors": [2.0, 5.0, 10.0]
            })
            .to_string(),
        );
        assert_eq!(resp["ok"], "true", "{resp}");
        let kr = resp["result"]["key_rates"].as_array().unwrap();
        assert_eq!(kr.len(), 3);
        for (i, expect_tenor) in [2.0_f64, 5.0, 10.0].iter().enumerate() {
            let t = kr[i]["tenor"].as_f64().unwrap();
            assert!((t - expect_tenor).abs() < 1e-9, "tenor[{i}] = {t}");
            let d = kr[i]["duration"].as_f64().unwrap();
            assert!(d.is_finite(), "krd[{i}] not finite");
        }
        // Sum of KRDs should approximately equal modified duration.
        let sum_krd: f64 = kr.iter().map(|e| e["duration"].as_f64().unwrap()).sum();
        let mod_dur = resp["result"]["modified_duration"].as_f64().unwrap();
        // Loose bound — KRDs only cover 2/5/10 of the term structure here.
        assert!(
            sum_krd > 0.0 && sum_krd < mod_dur * 2.0,
            "sum_krd={sum_krd}, mod_dur={mod_dur}"
        );
    }
}

#[test]
fn cashflow_kinds_are_stable_strings() {
    unsafe {
        let bond = build_handle(fixed_rate_5pct());
        let resp = rpc(
            convex_ffi::convex_cashflows,
            &json!({"bond": bond, "settlement": "2025-04-15"}).to_string(),
        );
        let flows = resp["result"]["flows"].as_array().unwrap();
        for cf in flows {
            let kind = cf["kind"].as_str().unwrap();
            assert!(
                matches!(
                    kind,
                    "coupon" | "redemption" | "coupon-and-redemption" | "fee"
                ),
                "unexpected kind {kind}"
            );
        }
    }
}

#[test]
fn make_whole_round_trip() {
    // MW callable bond mirroring Ford 6.798% '28: T+35bps MW spread, par
    // call 1 month before maturity, semi-annual 30/360 US.
    unsafe {
        let spec = json!({
            "type": "callable",
            "name": "MWBOND01",
            "coupon_rate": 0.06798,
            "frequency": "SemiAnnual",
            "maturity": "2028-11-07",
            "issue":    "2018-11-07",
            "day_count": "Thirty360US",
            "currency": "USD",
            "face_value": 100,
            "call_style": "make_whole",
            "make_whole_spread_bps": 35.0,
            "call_schedule": [
                {"date": "2028-10-07", "price": 100.0}
            ]
        });
        let bond = build_handle(spec);

        // ATM scenario: UST = 5% ≈ coupon → MW close to par.
        let req_atm = json!({
            "bond": bond,
            "call_date": "2026-04-15",
            "treasury_rate": 0.05
        });
        let resp = rpc(convex_ffi::convex_make_whole, &req_atm.to_string());
        assert_eq!(resp["ok"], "true", "response: {resp}");
        let r = &resp["result"];
        let price_atm = r["price"].as_f64().unwrap();
        assert!((r["spread_bps"].as_f64().unwrap() - 35.0).abs() < 1e-9);
        assert!((r["discount_rate"].as_f64().unwrap() - 0.0535).abs() < 1e-9);
        // Floored at the call entry's price; shouldn't drop below par.
        assert!(price_atm >= 100.0, "ATM price {price_atm} below floor");
        assert!(
            price_atm < 110.0,
            "ATM price {price_atm} unexpectedly far above par"
        );

        // ITM scenario: UST = 3% (well below coupon) → MW well above par.
        let req_itm = json!({
            "bond": bond,
            "call_date": "2026-04-15",
            "treasury_rate": 0.03
        });
        let resp = rpc(convex_ffi::convex_make_whole, &req_itm.to_string());
        assert_eq!(resp["ok"], "true");
        let price_itm = resp["result"]["price"].as_f64().unwrap();
        assert!(
            price_itm > price_atm + 1.0,
            "ITM ({price_itm}) should exceed ATM ({price_atm}) by >1pt"
        );
    }
}

#[test]
fn make_whole_rejects_non_make_whole_bond() {
    // A callable with no MW spread should error cleanly, not silently return 100.
    unsafe {
        let spec = json!({
            "type": "callable",
            "name": "PARCALL01",
            "coupon_rate": 0.05,
            "frequency": "SemiAnnual",
            "maturity": "2030-01-15",
            "issue":    "2025-01-15",
            "day_count": "Thirty360US",
            "currency": "USD",
            "face_value": 100,
            "call_style": "american",
            "call_schedule": [
                {"date": "2027-01-15", "price": 102.0}
            ]
        });
        let bond = build_handle(spec);
        let req = json!({
            "bond": bond,
            "call_date": "2027-01-15",
            "treasury_rate": 0.04
        });
        let resp = rpc(convex_ffi::convex_make_whole, &req.to_string());
        assert_eq!(resp["ok"], "false", "expected error envelope: {resp}");
        assert_eq!(resp["error"]["code"], "invalid_input");
    }
}

#[test]
fn make_whole_schema_lookup() {
    unsafe {
        for name in ["MakeWholeRequest", "MakeWholeResponse"] {
            let c = CString::new(name).unwrap();
            let ptr = convex_ffi::convex_schema(c.as_ptr());
            assert!(!ptr.is_null());
            let resp = CStr::from_ptr(ptr).to_string_lossy().into_owned();
            convex_ffi::convex_string_free(ptr);
            let v: Value = serde_json::from_str(&resp).unwrap();
            assert_eq!(v["ok"], "true", "{name}: {v}");
            assert_eq!(v["result"]["title"], name);
        }
    }
}

#[test]
fn mark_parse_round_trip() {
    unsafe {
        let text = CString::new("+125bps@USD.SOFR").unwrap();
        let ptr = convex_ffi::convex_mark_parse(text.as_ptr());
        assert!(!ptr.is_null());
        let resp = CStr::from_ptr(ptr).to_string_lossy().into_owned();
        convex_ffi::convex_string_free(ptr);
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["ok"], "true");
        assert_eq!(v["result"]["mark"], "spread");
        assert_eq!(v["result"]["benchmark"], "USD.SOFR");
    }
}
