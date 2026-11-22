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

/// Each method applies to the bonds it quotes: every method but a discount margin or an
/// OAS to a fixed bullet, a price to any bond with no set, a yield to worst to a callable
/// valued on its workouts, a discount margin only to a floating rate note and an OAS only
/// to a callable the HW1F model values, on its workouts or not, each with no set. An
/// American, make-whole or puttable callable is quoted by price only.
#[test]
fn each_method_applies_to_the_bonds_it_quotes() {
    use convex_ports::reference_data::{BondReferenceData, BondType, CallScheduleEntry};
    let bullet: BondReferenceData =
        serde_json::from_str(include_str!("fixtures/reference_bond.json")).unwrap();
    let mut note = bullet.clone();
    note.bond_type = BondType::FloatingRate;
    note.coupon_rate = None;
    let mut callable = bullet.clone();
    callable.bond_type = BondType::FixedCallable;
    callable.is_callable = true;
    callable.call_schedule = vec![CallScheduleEntry {
        call_date: convex_core::Date::parse("2028-01-15").unwrap(),
        call_price: callable.face_value,
        is_make_whole: false,
    }];
    callable.contractual_terms.as_mut().unwrap().call_terms =
        Some(convex_ports::reference_master::CallTerms {
            style: convex_ports::reference_master::CallStyle::Bermudan,
            notice_days: 30,
        });
    let mut american = callable.clone();
    american.contractual_terms.as_mut().unwrap().call_terms =
        Some(convex_ports::reference_master::CallTerms {
            style: convex_ports::reference_master::CallStyle::American,
            notice_days: 30,
        });
    let mut make_whole = callable.clone();
    make_whole.call_schedule[0].is_make_whole = true;
    let mut puttable = callable.clone();
    puttable.is_putable = true;
    // More calls than the workout model takes, which the HW1F model still values.
    let mut called_often = callable.clone();
    called_often.call_schedule = (0..=MAX_WORKOUT_CALLS as i64)
        .map(|day| CallScheduleEntry {
            call_date: callable.call_schedule[0].call_date.add_days(day),
            ..callable.call_schedule[0].clone()
        })
        .collect();
    let mut zero = bullet.clone();
    zero.bond_type = BondType::ZeroCoupon;
    zero.coupon_rate = None;
    let methods = [
        PricingMethod::Price {},
        PricingMethod::Yield {},
        PricingMethod::ZSpread {},
        PricingMethod::BenchmarkSpread {
            tenor: "10Y".into(),
        },
        PricingMethod::DiscountMargin {},
        PricingMethod::Oas {},
    ];
    let by_price_only = [true, false, false, false, false, false];
    // Per method, in the order above: whether it applies with no set, then with one.
    let rows = [
        (
            &bullet,
            [true, true, true, true, false, false],
            [true, true, true, true, false, false],
        ),
        (&note, [true, false, false, false, true, false], [false; 6]),
        (
            &callable,
            [true, true, false, false, false, true],
            [false; 6],
        ),
        (
            &called_often,
            [true, false, false, false, false, true],
            [false; 6],
        ),
    ];
    let priced_only =
        [&american, &make_whole, &puttable, &zero].map(|bond| (bond, by_price_only, [false; 6]));
    for (bond, without_set, with_set) in rows.into_iter().chain(priced_only) {
        for (index, method) in methods.iter().enumerate() {
            assert_eq!(
                method.applies_to(false, bond),
                without_set[index],
                "{:?} {}",
                bond.bond_type,
                method.name()
            );
            assert_eq!(
                method.applies_to(true, bond),
                with_set[index],
                "{:?} {} with a set",
                bond.bond_type,
                method.name()
            );
        }
    }
    let margin = PricingMethod::DiscountMargin {};
    assert_eq!(margin.name(), "discount_margin");
    assert_eq!(
        margin.level_field(),
        Some(convex_ports::quote_event::QuoteField::MidSpread)
    );
    let wire = rule(json!({"kind": "discount_margin"}), None);
    let parsed: PricingRuleDefinition = serde_json::from_value(wire.clone()).unwrap();
    parsed.validate().unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), wire);
}

/// An OAS is a spread level read from `mid_spread`, as a discount margin is: a quote is
/// read by it whenever it declares it, even beside a price.
#[test]
fn an_oas_is_a_spread_level_read_beside_a_price() {
    use convex_ports::quote_event::QuoteField;
    let oas = PricingMethod::Oas {};
    assert_eq!(oas.name(), "oas");
    let (price, spread) = (QuoteField::MidPrice, QuoteField::MidSpread);
    assert_eq!(oas.level_field(), Some(spread));
    assert_eq!(PricingMethod::Price {}.level_field(), None);
    for method in [oas.clone(), PricingMethod::DiscountMargin {}] {
        assert!(method.is_level_declared(&[spread]));
        assert!(method.is_level_declared(&[price, spread]));
        assert!(!method.is_level_declared(&[price]));
    }
    assert!(!PricingMethod::Price {}.is_level_declared(&[price]));
    let wire = rule(json!({"kind": "oas"}), None);
    let parsed: PricingRuleDefinition = serde_json::from_value(wire.clone()).unwrap();
    parsed.validate().unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), wire);
}

#[test]
fn unknown_fields_and_methods_are_refused() {
    for wire in [
        rule(json!({"kind": "yield", "curve": "USD-SOFR"}), None),
        rule(json!({"kind": "i_spread"}), None),
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
