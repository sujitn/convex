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

/// A width is a valid schedule in a unit the method quotes: points or 32nds for a price,
/// basis points for a yield or a spread. More than 16 buckets is a capacity refusal.
#[test]
fn a_width_is_in_a_unit_its_method_quotes() {
    let with_width = |method: serde_json::Value, unit: &str, buckets: serde_json::Value| {
        let mut wire = rule(method, Some("UST-OTR"));
        wire["width"] = json!({"unit": unit, "buckets": buckets});
        serde_json::from_value::<PricingRuleDefinition>(wire).unwrap()
    };
    let one = json!([{"width": "1"}]);
    let price = || json!({"kind": "price"});
    let spread = || json!({"kind": "benchmark_spread", "tenor": "10Y"});
    for (method, unit) in [
        (price(), "price_points"),
        (price(), "thirty_seconds"),
        (json!({"kind": "yield"}), "basis_points"),
        (json!({"kind": "z_spread"}), "basis_points"),
        (spread(), "basis_points"),
    ] {
        let parsed = with_width(method, unit, one.clone());
        parsed.validate().unwrap();
        let wire = serde_json::to_value(&parsed).unwrap();
        assert_eq!(wire["width"]["buckets"], one);
    }
    for (method, unit) in [
        (price(), "basis_points"),
        (json!({"kind": "yield"}), "price_points"),
        (json!({"kind": "z_spread"}), "thirty_seconds"),
        (spread(), "price_points"),
    ] {
        assert!(
            matches!(
                with_width(method.clone(), unit, one.clone()).validate(),
                Err(ReferenceError::InvalidInput(_))
            ),
            "{method} {unit}"
        );
    }
    let invalid = [
        json!([]),
        json!([{"width": "-1"}]),
        json!([{"width": "1001"}]),
        json!([{"width": "1"}, {"max_face": "1000000", "width": "2"}]),
        json!([{"max_face": "5000000", "width": "1"}, {"max_face": "1000000", "width": "2"}]),
    ];
    for buckets in invalid {
        assert!(
            matches!(
                with_width(json!({"kind": "yield"}), "basis_points", buckets.clone()).validate(),
                Err(ReferenceError::InvalidInput(_))
            ),
            "{buckets}"
        );
    }
    let many: Vec<_> = (0..=convex_core::types::MAX_SIZE_BUCKETS)
        .map(|n| json!({"max_face": format!("{}", n + 1), "width": "1"}))
        .collect();
    assert_eq!(
        with_width(json!({"kind": "yield"}), "basis_points", json!(many)).validate(),
        Err(ReferenceError::InvalidInput(
            "a width schedule holds at most 16 size buckets"
        ))
    );
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
    // The refusal is the caller's to fix, and names the bound it enforces.
    let Err(ReferenceError::InvalidInput(reason)) = set(json!(many)).validate() else {
        panic!("a set over its bound is invalid input");
    };
    assert!(
        reason.contains(&format!("at most {MAX_BENCHMARK_SECURITIES} securities")),
        "{reason}"
    );
}
