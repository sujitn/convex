//! Full-cash native/JSON reachability against independently declared cash math.
#[path = "../../../tests/fixtures/library/cash_contract.rs"]
mod contract;
use convex::{cash_calibration as c, cash_quotation as q, library::*};
use convex_core::{calendars::Calendar, Date};
use serde_json::{json, Value};

fn case(id: &str) -> Value {
    contract::cases()
        .into_iter()
        .find(|r| r["id"] == id)
        .unwrap()
}
fn output(id: &str) -> Value {
    let result = contract::native(&contract::request(&case(id)));
    assert_eq!(result["ok"], "true", "{id}: {result}");
    result["result"]["data"].clone()
}
fn close(actual: &Value, expected: &Value, tolerance: f64) {
    let a = actual.as_f64().unwrap();
    let b = expected.as_str().unwrap().parse::<f64>().unwrap();
    assert!(
        (a - b).abs() <= tolerance,
        "{a} versus {b}, bound {tolerance}"
    );
}
fn quote_reference(actual: &Value, expected: &Value, price: f64, rate: f64) {
    for key in ["clean_per100", "dirty_per100", "accrued_per100"] {
        close(&actual["conversion"][key], &expected[key], price);
    }
    close(
        &actual["yield_inverse"]["decimal_parameter"],
        &expected["decimal_yield"],
        rate,
    );
    assert!(
        actual["yield_inverse"]["native_residual"]
            .as_f64()
            .unwrap()
            .abs()
            <= actual["yield_inverse"]["settings"]["price_tolerance"]
                .as_f64()
                .unwrap()
    );
}
#[test]
fn full_native_cash_terms_roundtrip_and_all_errors_are_reachable() {
    for case in contract::cases() {
        let input = contract::request(&case);
        let typed = decode_request(&input).unwrap();
        assert_eq!(
            serde_json::to_value(&typed).unwrap(),
            case["request"],
            "{}",
            case["id"]
        );
        contract::check(&case, &execute_json(&input));
        if !case["expected_error"].is_null() {
            assert!(contract::native(&input).get("result").is_none());
        }
    }
}
#[test]
fn independent_decimal_cash_yield_invoice_and_actual_maturity_controls() {
    for reference in contract::references() {
        let id = reference["id"].as_str().unwrap();
        let actual = output(id);
        let expected = &reference["expected"];
        let price = reference["price_absolute"].as_f64().unwrap();
        let rate = reference["yield_absolute"].as_f64().unwrap();
        if id.starts_with("government.") {
            quote_reference(&actual["target"], &expected["target"], price, rate);
            for (a, b) in actual["benchmarks"]
                .as_array()
                .unwrap()
                .iter()
                .zip(expected["benchmarks"].as_array().unwrap())
            {
                quote_reference(&a["quotation"], b, price, rate);
            }
            assert_eq!(actual["selected"]["lower_index"], expected["lower_index"]);
            assert_eq!(actual["selected"]["upper_index"], expected["upper_index"]);
            close(
                &actual["selected"]["upper_weight"],
                &expected["upper_weight"],
                1e-15,
            );
            close(
                &actual["benchmark_decimal_yield"],
                &expected["benchmark_decimal_yield"],
                rate,
            );
            close(
                &actual["basis_points"],
                &expected["basis_points"],
                reference["spread_bp_absolute"].as_f64().unwrap(),
            );
        } else if id.starts_with("cash.invoice.") {
            quote_reference(&actual["quotation"], expected, price, rate);
            close(
                &actual["consideration"]["value"],
                &expected["consideration"],
                reference["money_absolute"].as_f64().unwrap(),
            );
            assert_eq!(actual["consideration"]["currency"], expected["currency"]);
        } else {
            quote_reference(&actual, expected, price, rate);
        }
    }
}
struct SuppliedCalendar;
impl Calendar for SuppliedCalendar {
    fn name(&self) -> &'static str {
        "CASH.SYNTHETIC.v1"
    }
    fn is_business_day(&self, _: Date) -> bool {
        true
    }
}
fn resources(h: &c::CashBondHelper) -> c::CashHelperResources<'_> {
    c::CashHelperResources::Bond {
        ids: &h.terms.calendars,
        calendars: convex::bond_contracts::BondCalendars {
            payment: &SuppliedCalendar,
            reset: &SuppliedCalendar,
            observation: &SuppliedCalendar,
            exercise: &SuppliedCalendar,
        },
    }
}
#[test]
fn standalone_native_functions_match_shared_dispatch_with_full_inputs() {
    for id in [
        "cash.quote.stepped_amortizing",
        "cash.invoice.nonpar",
        "government.linear.leap_delayed",
        "government.exact",
    ] {
        let typed = decode_request(&contract::request(&case(id))).unwrap();
        let direct = match typed.operation {
            LibraryOperation::CashSecurity {
                input,
                reference_date,
                convention,
                settings,
                action,
            } => {
                let res = resources(&input.helper);
                match action {
                    CashSecurityOperation::Quote {} => serde_json::to_value(
                        q::quote_cash_security(
                            &input.helper,
                            &res,
                            reference_date,
                            convention,
                            settings,
                        )
                        .unwrap(),
                    )
                    .unwrap(),
                    CashSecurityOperation::Invoice { position } => serde_json::to_value(
                        q::cash_security_invoice(
                            &input.helper,
                            &res,
                            reference_date,
                            convention,
                            settings,
                            &position,
                        )
                        .unwrap(),
                    )
                    .unwrap(),
                }
            }
            LibraryOperation::GovernmentSpread {
                target,
                benchmarks,
                reference_date,
                settings,
            } => {
                let bound: Vec<_> = benchmarks
                    .iter()
                    .map(|b| resources(&b.quote.helper))
                    .collect();
                let inputs: Vec<_> = benchmarks
                    .iter()
                    .zip(&bound)
                    .map(|(b, resources)| q::GovernmentSecurityInputs {
                        quote: &b.quote,
                        resources,
                    })
                    .collect();
                serde_json::to_value(
                    q::government_spread(
                        &target.helper,
                        &resources(&target.helper),
                        &inputs,
                        reference_date,
                        &settings,
                    )
                    .unwrap(),
                )
                .unwrap()
            }
            _ => panic!("wrong declared operation"),
        };
        assert_eq!(direct, output(id));
    }
}
#[test]
fn signed_original_quote_invoice_and_fractional_government_spread_stay_distinct() {
    let long = output("cash.invoice.long");
    let short = output("cash.invoice.short");
    let original = long["quotation"]["conversion"]["dirty_per100"]
        .as_f64()
        .unwrap();
    assert_eq!(
        long["consideration"]["value"].as_f64().unwrap(),
        original * 5000.0 / 100.0 * 3.0
    );
    assert!(
        (short["consideration"]["value"].as_f64().unwrap()
            + long["consideration"]["value"].as_f64().unwrap() * 2.0 / 3.0)
            .abs()
            < 1e-9
    );
    assert_eq!(output("cash.invoice.zero")["consideration"]["value"], 0.0);
    let spread = output("government.linear");
    assert!((spread["basis_points"].as_f64().unwrap() - 12.75).abs() < 1e-5);
    assert_eq!(spread["benchmarks"][0]["security"]["version"], 21);
    assert_eq!(
        spread["benchmarks"][0]["quotation"]["source"]["quote_version"],
        11
    );
    assert_eq!(
        spread["benchmarks"][0]["quotation"]["source"]["quote"]["unit"],
        "BasisPoints"
    );
    assert_eq!(spread["selected"]["lower_index"], 1);
    assert_ne!(
        output("government.linear.leap_delayed")["selected"]["upper_weight"],
        0.5
    );
    assert!(output("government.linear.requoted_annual")["basis_points"] != spread["basis_points"]);
}
#[test]
fn discarded_terms_cached_yields_and_numeric_quantity_fail_strict_decoding() {
    let mut r = case("government.linear")["request"].clone();
    r["operation"]["benchmarks"][0]["quote"]["cached_yield"] = json!(0.03);
    assert_eq!(
        decode_request(&r.to_string()).unwrap_err().code,
        LibraryErrorCode::InvalidInput
    );
    let mut r = case("cash.invoice.long")["request"].clone();
    r["operation"]["action"]["position"]["quantity"] = json!(15000.0);
    assert_eq!(
        decode_request(&r.to_string()).unwrap_err().code,
        LibraryErrorCode::InvalidInput
    );
    let mut r = case("cash.quote.USD.CashflowYield")["request"].clone();
    r["operation"]["convention"]["street_yield"] = json!(true);
    assert_eq!(
        decode_request(&r.to_string()).unwrap_err().code,
        LibraryErrorCode::InvalidInput
    );
}
