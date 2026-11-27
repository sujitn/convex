//! A perpetual is stored with perpetual terms and no maturity date; both keys are omitted
//! when absent, so a dated bond's bytes are unchanged. Admission is structural: a fixed
//! positive coupon under 30/360 US or 30E/360, calls at whole coupon periods from a first
//! call on the coupon grid, and nothing else embedded. Triggers, coupon cancellation and
//! floating perpetuals have no terms here and are refused.
use convex_ports::level_pricing::{
    is_quoted_two_way, is_router_valued, PricingMethod, PricingRuleDefinition,
};
use convex_ports::reference_data::{BondReferenceData, BondType};
use convex_ports::reference_repository::{validate_bond_reference, ReferenceError};
use serde_json::{json, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/reference_bond.json")).unwrap()
}

/// The fixture bond as a 6.5% semiannual 30/360 US perpetual from January 2025, callable
/// at par every coupon date from January 2030 on 30 days' notice.
fn perpetual_json() -> Value {
    let mut bond = fixture();
    bond.as_object_mut().unwrap().remove("maturity_date");
    bond["bond_type"] = json!("Perpetual");
    bond["is_callable"] = json!(true);
    bond["issue_date"] = json!("2025-01-15");
    bond["coupon_rate"] = json!("0.065");
    bond["day_count"] = json!("30/360 US");
    let terms = &mut bond["contractual_terms"];
    terms["dated_date"] = json!("2025-01-15");
    terms["settings"]["day_count"] = json!("Thirty360US");
    terms["settings"]["end_of_month"] = json!(false);
    terms["perpetual"] = json!({
        "first_call_date": "2030-01-15",
        "call_interval_months": 6,
        "call_price": "100",
        "notice_days": 30,
    });
    bond
}

fn perpetual() -> BondReferenceData {
    serde_json::from_value(perpetual_json()).unwrap()
}

fn refused(bond: &BondReferenceData) -> ReferenceError {
    validate_bond_reference(bond).err().unwrap()
}

#[test]
fn a_dated_bond_keeps_its_bytes_and_a_perpetual_stores_no_maturity() {
    // A dated bond writes back exactly the stored fixture: its maturity, and no perpetual
    // terms.
    let dated: BondReferenceData = serde_json::from_value(fixture()).unwrap();
    assert_eq!(serde_json::to_value(&dated).unwrap(), fixture());
    let bond = perpetual();
    assert_eq!(bond.maturity_date, None);
    assert_eq!(
        bond.schedule_end(),
        Some(convex_core::Date::parse("2030-01-15").unwrap())
    );
    validate_bond_reference(&bond).unwrap();
    let written = serde_json::to_value(&bond).unwrap();
    assert!(written.get("maturity_date").is_none());
    assert_eq!(written, perpetual_json());
    assert!(!is_quoted_two_way(&bond) && !is_router_valued(&bond));
}

#[test]
fn a_bond_has_a_maturity_date_or_perpetual_terms_exactly_once() {
    let date = convex_core::Date::parse("2040-01-15").unwrap();
    let mut both = perpetual();
    both.maturity_date = Some(date);
    let mut neither = perpetual();
    neither.contractual_terms.as_mut().unwrap().perpetual = None;
    let mut dated_with_terms = perpetual();
    dated_with_terms.bond_type = BondType::FixedCallable;
    dated_with_terms.maturity_date = Some(date);
    // A floating perpetual is a floating rate note with no maturity.
    let mut floating: BondReferenceData = serde_json::from_value(fixture()).unwrap();
    floating.bond_type = BondType::FloatingRate;
    floating.maturity_date = None;
    for bond in [both, neither, dated_with_terms, floating] {
        assert!(
            matches!(refused(&bond), ReferenceError::InvalidInput(_)),
            "{:?}",
            bond.bond_type
        );
    }
}

#[test]
fn a_perpetual_carries_only_a_fixed_coupon_and_its_recurring_calls() {
    let invalid: [fn(&mut BondReferenceData); 15] = [
        |b| b.is_callable = false,
        |b| {
            b.call_schedule = vec![convex_ports::reference_data::CallScheduleEntry {
                call_date: convex_core::Date::parse("2030-01-15").unwrap(),
                call_price: rust_decimal::Decimal::ONE_HUNDRED,
                is_make_whole: false,
            }]
        },
        |b| {
            b.contractual_terms.as_mut().unwrap().call_terms =
                Some(convex_ports::reference_master::CallTerms {
                    style: convex_ports::reference_master::CallStyle::Bermudan,
                    notice_days: 30,
                })
        },
        |b| b.is_putable = true,
        |b| {
            b.floating_terms = Some(convex_ports::reference_data::FloatingRateTerms {
                index: convex_core::ids::FloatingRateIndex::Sofr,
                spread: rust_decimal::Decimal::from(45),
                reset_frequency: 2,
                cap: None,
                floor: None,
            })
        },
        |b| b.coupon_rate = Some(rust_decimal::Decimal::ZERO),
        |b| {
            b.contractual_terms.as_mut().unwrap().redemption_value =
                convex_core::contracts::ExactDecimal::parse("101").unwrap()
        },
        |b| perpetual_terms(b).call_interval_months = 0,
        |b| perpetual_terms(b).call_interval_months = 3,
        |b| perpetual_terms(b).call_interval_months = 1806,
        |b| {
            perpetual_terms(b).call_price =
                convex_core::contracts::ExactDecimal::parse("0").unwrap()
        },
        |b| {
            perpetual_terms(b).call_price =
                convex_core::contracts::ExactDecimal::parse("1000.5").unwrap()
        },
        |b| perpetual_terms(b).notice_days = 367,
        // A first call on the 29th and later rolls to a shorter month's last day.
        |b| {
            b.contractual_terms.as_mut().unwrap().dated_date =
                convex_core::contracts::BusinessDate::parse("2025-01-29").unwrap();
            b.issue_date = convex_core::Date::parse("2025-01-29").unwrap();
            perpetual_terms(b).first_call_date =
                convex_core::contracts::BusinessDate::parse("2030-01-29").unwrap()
        },
        // A first call off the coupon grid implies a stub.
        |b| {
            perpetual_terms(b).first_call_date =
                convex_core::contracts::BusinessDate::parse("2030-03-15").unwrap()
        },
    ];
    for (i, change) in invalid.into_iter().enumerate() {
        let mut bond = perpetual();
        change(&mut bond);
        let error = refused(&bond);
        assert!(
            matches!(error, ReferenceError::InvalidInput(_)),
            "{i}: {error:?}"
        );
    }
    let mut actual = perpetual();
    actual.day_count = "ACT/ACT".into();
    actual
        .contractual_terms
        .as_mut()
        .unwrap()
        .settings
        .day_count = convex_core::daycounts::DayCountConvention::ActActIsda;
    assert!(matches!(refused(&actual), ReferenceError::InvalidInput(_)));
    // A back stub would end the stated schedule before the first call; no bond takes one.
    let mut back_stub = perpetual();
    back_stub.contractual_terms.as_mut().unwrap().stub =
        convex_ports::reference_master::BondStub::ShortLast;
    assert!(matches!(
        refused(&back_stub),
        ReferenceError::Unsupported(_)
    ));
    // Month end with end_of_month is a regular roll to these checks.
    let mut month_end = perpetual();
    month_end.issue_date = convex_core::Date::parse("2025-07-31").unwrap();
    let terms = month_end.contractual_terms.as_mut().unwrap();
    terms.dated_date = convex_core::contracts::BusinessDate::parse("2025-07-31").unwrap();
    terms.settings.end_of_month = true;
    perpetual_terms(&mut month_end).first_call_date =
        convex_core::contracts::BusinessDate::parse("2030-07-31").unwrap();
    validate_bond_reference(&month_end).unwrap();
}

fn perpetual_terms(
    bond: &mut BondReferenceData,
) -> &mut convex_ports::reference_master::PerpetualTerms {
    bond.contractual_terms
        .as_mut()
        .unwrap()
        .perpetual
        .as_mut()
        .unwrap()
}

#[test]
fn write_down_triggers_and_coupon_cancellation_have_no_terms() {
    for field in ["write_down_trigger", "coupon_cancellation", "reset"] {
        let mut bond = perpetual_json();
        bond["contractual_terms"]["perpetual"][field] = json!({});
        assert!(
            serde_json::from_value::<BondReferenceData>(bond).is_err(),
            "{field}"
        );
    }
}

#[test]
fn a_perpetual_is_quoted_by_price_or_yield_and_its_g_spread_read_at_its_workout() {
    let bond = perpetual();
    let bullet: BondReferenceData = serde_json::from_value(fixture()).unwrap();
    for (method, applies) in [
        (json!({"kind": "price"}), true),
        (json!({"kind": "yield"}), true),
        (json!({"kind": "z_spread"}), false),
        (json!({"kind": "benchmark_spread", "tenor": "10Y"}), false),
        (json!({"kind": "discount_margin"}), false),
        (json!({"kind": "oas"}), false),
    ] {
        let method: PricingMethod = serde_json::from_value(method).unwrap();
        assert_eq!(method.applies_to(false, &bond), applies, "{method:?}");
        assert_eq!(method.applies_to(true, &bond), applies, "{method:?}");
    }
    let rule = |basis: Option<&str>| -> PricingRuleDefinition {
        let mut rule = json!({
            "instrument_id": "BOND-001",
            "method": {"kind": "yield"},
            "benchmark_set_id": "ust-on-the-run",
            "effective": {"from": "2025-01-01T00:00:00Z", "until": null},
            "provenance": {"source": "desk", "evidence_reference": "perpetual-test"},
        });
        if let Some(basis) = basis {
            rule["tenor_basis"] = json!(basis);
        }
        serde_json::from_value(rule).unwrap()
    };
    assert!(rule(Some("workout")).applies_to(&bond));
    assert!(!rule(None).applies_to(&bond));
    assert!(!rule(Some("average_life")).applies_to(&bond));
    assert!(!rule(Some("workout")).applies_to(&bullet));
    assert!(rule(None).applies_to(&bullet));
    // With no set, the basis is maturity and places nothing.
    let mut no_set = rule(None);
    no_set.benchmark_set_id = None;
    assert!(no_set.applies_to(&bond));
}
