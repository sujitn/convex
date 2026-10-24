//! Actual-security inverse reachability and independent Decimal60 cash controls.
#[path = "../../../tests/fixtures/library/government_inverse.rs"]
mod controls;
use convex::{cash_calibration as c, cash_quotation as q, library::*};
use convex_bonds::contracts::BondCalendars;
use convex_core::{calendars::Calendar, Date};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::{collections::BTreeMap, str::FromStr};

#[test]
fn complete_reverse_requests_and_failures_survive_shared_json() {
    for case in controls::cases() {
        let wire = controls::request(&case);
        if case["expected_error"].is_null() {
            let typed = decode_request(&wire).unwrap();
            assert_eq!(
                serde_json::to_value(typed).unwrap(),
                case["request"],
                "{}",
                case["id"]
            );
        }
        controls::check(&case, &execute_json(&wire));
    }
}

#[test]
fn independent_prices_spreads_cash_and_signed_invoices_match() {
    let cases = controls::cases();
    for reference in controls::references() {
        let case = cases.iter().find(|c| c["id"] == reference["id"]).unwrap();
        let envelope = controls::native(&controls::request(case));
        assert_eq!(envelope["ok"], "true", "{}: {envelope}", case["id"]);
        let actual = &envelope["result"]["data"];
        let expected = &reference["expected"];
        for key in ["dirty_per100", "clean_per100", "accrued_per100"] {
            let bound = controls::number(
                &reference[if key == "accrued_per100" {
                    "accrued_absolute"
                } else {
                    "price_absolute"
                }],
            );
            controls::close(&actual["valuation"][key], &expected[key], bound);
        }
        for key in [
            "benchmark_decimal_yield",
            "target_decimal_yield",
            "decimal_spread",
        ] {
            controls::close(
                &actual[key],
                &expected[key],
                controls::number(&reference["rate_absolute"]),
            );
        }
        controls::close(
            &actual["basis_points"],
            &expected["basis_points"],
            controls::number(&reference["basis_points_absolute"]),
        );
        controls::close(
            &actual["consideration"]["value"],
            &expected["consideration"],
            controls::number(&reference["money_absolute"]),
        );
        for key in ["lower_index", "upper_index", "target_remaining_days"] {
            assert_eq!(actual["selected"][key], expected[key]);
        }
        for key in ["lower_weight", "upper_weight"] {
            controls::close(&actual["selected"][key], &expected[key], 1e-15);
        }
        assert_eq!(
            actual["target"],
            case["request"]["operation"]["target"]["target"]
        );
        assert_eq!(actual["quote"], case["request"]["operation"]["quote"]);
        assert_eq!(actual["settings"], case["request"]["operation"]["settings"]);
        assert_eq!(actual["position"], case["request"]["operation"]["position"]);
        for (i, benchmark) in actual["benchmarks"].as_array().unwrap().iter().enumerate() {
            let source = &case["request"]["operation"]["benchmarks"][i]["quote"];
            assert_eq!(benchmark["security"], source["security"]);
            assert_eq!(benchmark["quotation"]["source"], source["helper"]);
            controls::close(
                &benchmark["quotation"]["yield_inverse"]["decimal_parameter"],
                &expected["benchmark_yields"][i],
                controls::number(&reference["rate_absolute"]),
            );
        }
        let inverse = &actual["yield_inverse"];
        assert!(
            controls::number(&inverse["native_residual"]).abs()
                <= controls::number(&inverse["settings"]["price_tolerance"])
        );
        assert!(
            controls::number(&actual["decimal_spread_residual"]).abs()
                <= controls::number(&actual["settings"]["decimal_spread_tolerance"])
        );
        assert!(inverse["price_evaluations"].as_u64().unwrap() >= 3);
        // Keep decimal contractual cash and separate same-date principal
        // allocations; binary floating point cannot verify the declared bound.
        let cash = &actual["valuation"]["cash"];
        let coupons = cash["coupons"].as_array().unwrap();
        let principal = cash["principal"].as_array().unwrap();
        let decimal = |value: &Value| Decimal::from_str(value.as_str().unwrap()).unwrap();
        let mut components = BTreeMap::<String, Vec<Decimal>>::new();
        for entry in coupons.iter().filter(|c| c["entitled"] == true) {
            components
                .entry(entry["payment_date"].as_str().unwrap().into())
                .or_default()
                .push(decimal(&entry["amount"]));
        }
        for entry in principal {
            components
                .entry(entry["payment_date"].as_str().unwrap().into())
                .or_default()
                .push(decimal(&entry["amount"]));
        }
        let mut expected_components = BTreeMap::<String, Vec<Decimal>>::new();
        for entry in expected["cash"].as_array().unwrap() {
            expected_components
                .entry(entry["date"].as_str().unwrap().into())
                .or_default()
                .push(decimal(&entry["amount"]));
        }
        assert_eq!(
            components.keys().collect::<Vec<_>>(),
            expected_components.keys().collect::<Vec<_>>(),
            "{}: {cash}",
            case["id"]
        );
        let bound = Decimal::from_scientific(&reference["cash_absolute"].to_string()).unwrap();
        for (date, mut values) in components {
            let expected = expected_components.get_mut(&date).unwrap();
            values.sort();
            expected.sort();
            assert_eq!(values.len(), expected.len(), "{}: {date}", case["id"]);
            for (actual, expected) in values.iter().zip(expected.iter()) {
                assert!(
                    (actual - expected).abs() <= bound,
                    "{}: {date}: {actual} versus {expected}, bound {bound}",
                    case["id"]
                );
            }
            assert!(
                (values.iter().copied().sum::<Decimal>()
                    - expected.iter().copied().sum::<Decimal>())
                .abs()
                    <= bound,
                "{}: {date}",
                case["id"]
            );
        }
    }
}

#[test]
fn direct_native_inverse_reuses_checked_contract_and_every_benchmark() {
    struct SuppliedCalendar;
    impl Calendar for SuppliedCalendar {
        fn name(&self) -> &'static str {
            "CASH.SYNTHETIC.v1"
        }
        fn is_business_day(&self, _: Date) -> bool {
            true
        }
    }
    let calendar = SuppliedCalendar;
    let calendars = || BondCalendars {
        payment: &calendar,
        reset: &calendar,
        observation: &calendar,
        exercise: &calendar,
    };
    for case in controls::cases()
        .into_iter()
        .filter(|c| c["expected_error"].is_null())
    {
        let request = decode_request(&controls::request(&case)).unwrap();
        let LibraryOperation::GovernmentSpreadPrice {
            target,
            benchmarks,
            reference_date,
            settings,
            quote,
            position,
        } = request.operation
        else {
            panic!("wrong route")
        };
        let resources = c::CashHelperResources::Bond {
            ids: &target.target.terms.calendars,
            calendars: calendars(),
        };
        let bound: Vec<_> = benchmarks
            .iter()
            .map(|b| c::CashHelperResources::Bond {
                ids: &b.quote.helper.terms.calendars,
                calendars: calendars(),
            })
            .collect();
        let securities: Vec<_> = benchmarks
            .iter()
            .zip(&bound)
            .map(|(b, resources)| q::GovernmentSecurityInputs {
                quote: &b.quote,
                resources,
            })
            .collect();
        let actual = q::price_government_spread(
            &target.target,
            &resources,
            &securities,
            reference_date,
            &settings,
            &quote,
            &position,
        )
        .unwrap_or_else(|e| panic!("{}: {e}", case["id"]));
        let pillar = actual
            .valuation
            .cash
            .cashflows()
            .unwrap()
            .iter()
            .map(|c| c.date)
            .max()
            .unwrap();
        // This is an explicitly model-generated roundtrip observation, not an
        // independent financial oracle or a new observed market price.
        let roundtrip = c::CashBondHelper {
            id: target.target.security.id.as_str().into(),
            quote_version: quote.quote_version,
            pillar,
            terms: target.target.terms.clone(),
            trade_date: target.target.trade_date,
            settlement: target.target.settlement,
            on_payment: target.target.on_payment,
            credit_identity: None,
            quote_kind: c::CashQuoteKind::DirtyPrice,
            quote: convex::curve_contracts::MarketQuote {
                unit: convex::curve_contracts::QuoteUnit::PricePer100,
                value: actual.valuation.dirty_per100,
            },
            tolerance: settings.government.inverse.price_tolerance,
        };
        let forward = q::government_spread(
            &roundtrip,
            &resources,
            &securities,
            reference_date,
            &settings.government,
        )
        .unwrap();
        assert!(
            (forward.decimal_spread - actual.decimal_spread).abs()
                <= settings.decimal_spread_tolerance
        );
        let expected = controls::native(&controls::request(&case));
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            expected["result"]["data"]
        );
    }
}

#[test]
fn explicit_spread_postcondition_cannot_be_replaced_by_price_acceptance() {
    let case = controls::cases()
        .into_iter()
        .find(|c| c["id"] == "gov_inverse.linear.SemiAnnual")
        .unwrap();
    let mut r = case["request"].clone();
    r["operation"]["settings"]["decimal_spread_tolerance"] = json!(1e-30);
    let answer: Value = serde_json::from_str(&execute_json(&r.to_string())).unwrap();
    assert_eq!(answer["ok"], "false", "{answer}");
    assert_eq!(answer["error"]["code"], "no_convergence");
    assert_eq!(
        answer["error"]["details"]["residual_unit"],
        "annual_decimal_rate"
    );
    assert!(answer.get("result").is_none());
}
