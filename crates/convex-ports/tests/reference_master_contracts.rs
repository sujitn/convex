use convex_core::contracts::*;
use convex_ports::reference_data::BondReferenceData;
use convex_ports::reference_master::*;
use convex_ports::reference_repository::*;

#[test]
fn mandatory_terms_and_resource_envelopes_reject_unknown_fields() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/reference_bond.json")).unwrap();
    let bond: BondReferenceData = serde_json::from_value(fixture.clone()).unwrap();
    validate_bond_reference(&bond).unwrap();
    for field in [
        "dated_date",
        "redemption_value",
        "calendar",
        "convention",
        "settings",
        "issuer",
        "stub",
        "payment_lag",
    ] {
        let mut value = fixture.clone();
        value["contractual_terms"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(serde_json::from_value::<BondReferenceData>(value).is_err());
    }
    for path in [
        "",
        "/contractual_terms",
        "/contractual_terms/settings",
        "/contractual_terms/calendar",
        "/contractual_terms/issuer",
        "/contractual_terms/provenance",
    ] {
        let mut value = fixture.clone();
        value.pointer_mut(path).unwrap()["unexpected"] = true.into();
        assert!(serde_json::from_value::<BondReferenceData>(value).is_err());
    }
    for malformed in [
        serde_json::json!(100.0),
        serde_json::json!("1e2"),
        serde_json::json!("0.12345678901234567890123456789"),
    ] {
        let mut value = fixture.clone();
        value["contractual_terms"]["redemption_value"] = malformed;
        assert!(serde_json::from_value::<BondReferenceData>(value).is_err());
    }
    for field in ["calendar", "convention"] {
        let mut value = fixture.clone();
        value["contractual_terms"][field]["version"] = 0.into();
        assert!(serde_json::from_value::<BondReferenceData>(value).is_err());
    }
}

#[test]
fn effective_boundaries_and_unsupported_terms_are_explicit() {
    let from = UtcTimestamp::parse("2026-01-01T00:00:00.000000001Z").unwrap();
    let until = UtcTimestamp::parse("2026-01-01T00:00:00.000000002Z").unwrap();
    let interval = EffectiveInterval {
        from,
        until: Some(until),
    };
    interval.validate().unwrap();
    assert!(interval.contains(from));
    assert!(!interval.contains(until));
    assert!(EffectiveInterval {
        from,
        until: Some(from)
    }
    .validate()
    .is_err());
    let mut bond: BondReferenceData =
        serde_json::from_str(include_str!("fixtures/reference_bond.json")).unwrap();
    bond.contractual_terms.as_mut().unwrap().stub = BondStub::LongFirst;
    assert!(matches!(
        validate_bond_reference(&bond),
        Err(ReferenceError::Unsupported(_))
    ));
    bond.contractual_terms.as_mut().unwrap().stub = BondStub::ShortLast;
    bond.contractual_terms
        .as_mut()
        .unwrap()
        .penultimate_coupon_date = Some(BusinessDate::parse("2029-12-15").unwrap());
    assert!(matches!(
        validate_bond_reference(&bond),
        Err(ReferenceError::Unsupported(_))
    ));
    bond.contractual_terms
        .as_mut()
        .unwrap()
        .penultimate_coupon_date = None;
    bond.contractual_terms.as_mut().unwrap().stub = BondStub::None;
    bond.maturity_date = convex_core::Date::parse("2030-02-15").unwrap();
    assert!(matches!(
        validate_bond_reference(&bond),
        Err(ReferenceError::InvalidInput(_))
    ));
    bond.maturity_date = convex_core::Date::parse("2030-01-15").unwrap();
    bond.contractual_terms.as_mut().unwrap().payment_lag = 1;
    assert!(matches!(
        validate_bond_reference(&bond),
        Err(ReferenceError::Unsupported(_))
    ));
    assert!(serde_json::from_str::<ExternalIdentifier>(
        r#"{"kind":"provider","namespace":"feed","value":"X","extra":true}"#
    )
    .is_err());
    assert!(
        serde_json::from_str::<ExternalIdentifier>(r#"{"kind":"provider","value":"X"}"#).is_err()
    );
    assert!(ExternalIdentifier::Isin {
        value: "x".repeat(129)
    }
    .validate()
    .is_err());
    assert!(ExternalIdentifier::Isin { value: "\n".into() }
        .validate()
        .is_err());
    assert!(ReferenceDataId::new("provider symbol with spaces").is_err());
}

#[test]
fn call_terms_are_optional_bytes_and_belong_only_to_a_dated_priced_callable() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/reference_bond.json")).unwrap();
    let bullet: BondReferenceData = serde_json::from_value(fixture.clone()).unwrap();
    // Absent call terms are omitted, so stored records keep their exact bytes.
    let bytes = serde_json::to_value(&bullet).unwrap();
    assert!(bytes["contractual_terms"].get("call_terms").is_none());
    let callable = || {
        let mut bond = bullet.clone();
        bond.bond_type = convex_ports::reference_data::BondType::FixedCallable;
        bond.is_callable = true;
        bond.call_schedule = ["2027-01-15", "2028-01-15"]
            .iter()
            .map(|date| convex_ports::reference_data::CallScheduleEntry {
                call_date: convex_core::Date::parse(date).unwrap(),
                call_price: rust_decimal::Decimal::ONE_HUNDRED,
                is_make_whole: false,
            })
            .collect();
        bond.contractual_terms.as_mut().unwrap().call_terms = Some(CallTerms {
            style: CallStyle::Bermudan,
            notice_days: 30,
        });
        bond
    };
    validate_bond_reference(&callable()).unwrap();
    let text = serde_json::to_value(callable()).unwrap();
    assert_eq!(
        text["contractual_terms"]["call_terms"],
        serde_json::json!({"style": "bermudan", "notice_days": 30})
    );
    // A callable without call terms still records; it is refused only when valued.
    let mut legacy = callable();
    legacy.contractual_terms.as_mut().unwrap().call_terms = None;
    validate_bond_reference(&legacy).unwrap();
    let invalid: Vec<Box<dyn Fn(&mut BondReferenceData)>> = vec![
        Box::new(|b| b.bond_type = convex_ports::reference_data::BondType::FixedBullet),
        Box::new(|b| b.is_callable = false),
        Box::new(|b| b.call_schedule.clear()),
        Box::new(|b| b.call_schedule.swap(0, 1)),
        Box::new(|b| b.call_schedule[1].call_date = b.maturity_date.add_days(1)),
        Box::new(|b| b.call_schedule[0].call_price = rust_decimal::Decimal::ZERO),
        Box::new(|b| {
            b.contractual_terms
                .as_mut()
                .unwrap()
                .call_terms
                .as_mut()
                .unwrap()
                .notice_days = 367
        }),
    ];
    for change in invalid {
        let mut bond = callable();
        change(&mut bond);
        assert!(matches!(
            validate_bond_reference(&bond),
            Err(ReferenceError::InvalidInput(_))
        ));
    }
    let mut make_whole = callable();
    make_whole.call_schedule[0].is_make_whole = true;
    assert!(matches!(
        validate_bond_reference(&make_whole),
        Err(ReferenceError::Unsupported(_))
    ));
}
