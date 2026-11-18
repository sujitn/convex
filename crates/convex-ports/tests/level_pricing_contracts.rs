//! Wire contracts of pricing rules and benchmark sets: their JSON shape, unknown
//! fields refused, tenors, and the bounds each refuses with its type.
use convex_ports::level_pricing::*;
use convex_ports::reference_repository::ReferenceError;
use serde_json::json;

fn rule(method: serde_json::Value, set: Option<&str>) -> serde_json::Value {
    let mut rule = json!({
        "instrument_id": "CORP-1",
        "method": method,
        "effective": {"from": "2025-01-01T00:00:00Z", "until": null},
        "provenance": {"source": "desk", "evidence_reference": "contract"},
    });
    if let Some(set) = set {
        rule["benchmark_set_id"] = set.into();
    }
    rule
}

#[test]
fn a_rule_round_trips_and_omits_an_absent_set() {
    let wire = rule(json!({"kind": "z_spread"}), None);
    let parsed: PricingRuleDefinition = serde_json::from_value(wire.clone()).unwrap();
    parsed.validate().unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), wire);
    assert_eq!(parsed.method.name(), "z_spread");
}

#[test]
fn unknown_fields_and_methods_are_refused() {
    for wire in [
        rule(json!({"kind": "yield", "curve": "USD-SOFR"}), None),
        rule(json!({"kind": "oas"}), None),
        {
            let mut wire = rule(json!({"kind": "price"}), None);
            wire["level"] = "99.5".into();
            wire
        },
    ] {
        assert!(serde_json::from_value::<PricingRuleDefinition>(wire).is_err());
    }
}

#[test]
fn a_benchmark_spread_names_its_set_and_a_tenor() {
    let spread = |tenor: &str| json!({"kind": "benchmark_spread", "tenor": tenor});
    let parse = |wire| serde_json::from_value::<PricingRuleDefinition>(wire).unwrap();
    parse(rule(spread("10Y"), Some("UST-OTR")))
        .validate()
        .unwrap();
    assert!(parse(rule(spread("10Y"), None)).validate().is_err());
    for tenor in [
        "", "10", "10y", "010Y", "0M", "51Y", "1000M", "+5Y", "1é", "Y",
    ] {
        assert!(
            parse(rule(spread(tenor), Some("UST-OTR")))
                .validate()
                .is_err(),
            "{tenor}"
        );
    }
}

#[test]
fn tenors_name_months() {
    assert_eq!(tenor_months("6M").unwrap(), 6);
    assert_eq!(tenor_months("10Y").unwrap(), 120);
    assert_eq!(tenor_months("50Y").unwrap(), MAX_BENCHMARK_TENOR_MONTHS);
    assert!(tenor_months("601M").is_err());
}

#[test]
fn a_usd_set_holds_one_to_32_securities_with_unique_tenors_and_instruments() {
    let set = |securities: serde_json::Value| -> BenchmarkSetDefinition {
        serde_json::from_value(json!({
            "benchmark_set_id": "UST-OTR",
            "currency": "USD",
            "securities": securities,
            "effective": {"from": "2025-01-01T00:00:00Z", "until": null},
            "provenance": {"source": "desk", "evidence_reference": "contract"},
        }))
        .unwrap()
    };
    let security =
        |tenor: &str, id: &str| json!({"tenor": tenor, "instrument_id": id, "version": 1});
    let valid = set(json!([
        security("2Y", "UST-2Y"),
        security("10Y", "UST-10Y")
    ]));
    valid.validate().unwrap();
    // Spreads are quoted against Treasuries only: a set in another currency is refused.
    let mut euro = valid.clone();
    euro.currency = convex_core::Currency::EUR;
    assert!(euro.validate().is_err());
    assert!(set(json!([])).validate().is_err());
    assert!(set(json!([security("1Y", "A"), security("12M", "B")]))
        .validate()
        .is_err());
    assert!(set(json!([security("1Y", "A"), security("2Y", "A")]))
        .validate()
        .is_err());
    let many: Vec<_> = (1..=MAX_BENCHMARK_SECURITIES + 1)
        .map(|n| security(&format!("{n}M"), &format!("UST-{n}")))
        .collect();
    assert_eq!(
        set(json!(many)).validate(),
        Err(ReferenceError::CapacityExceeded)
    );
}
