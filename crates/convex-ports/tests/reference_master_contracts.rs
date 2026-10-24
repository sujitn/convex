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
    let invalid: [fn(&mut BondReferenceData); 7] = [
        |b| b.bond_type = convex_ports::reference_data::BondType::FixedBullet,
        |b| b.is_callable = false,
        |b| b.call_schedule.clear(),
        |b| b.call_schedule.swap(0, 1),
        |b| b.call_schedule[1].call_date = b.maturity_date.add_days(1),
        |b| b.call_schedule[0].call_price = rust_decimal::Decimal::ZERO,
        |b| {
            b.contractual_terms
                .as_mut()
                .unwrap()
                .call_terms
                .as_mut()
                .unwrap()
                .notice_days = 367
        },
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

fn frn() -> BondReferenceData {
    let mut bond: BondReferenceData =
        serde_json::from_str(include_str!("fixtures/reference_bond.json")).unwrap();
    bond.bond_type = convex_ports::reference_data::BondType::FloatingRate;
    bond.coupon_rate = None;
    bond.floating_terms = Some(convex_ports::reference_data::FloatingRateTerms {
        index: convex_core::ids::FloatingRateIndex::Sofr,
        spread: rust_decimal::Decimal::from(45),
        reset_frequency: 2,
        cap: None,
        floor: None,
    });
    let terms = bond.contractual_terms.as_mut().unwrap();
    terms.payment_lag = 2;
    terms.floating_mechanics = Some(FloatingMechanics {
        index: ReferencePin {
            id: ReferenceDataId::new("usd-sofr").unwrap(),
            version: Version::new(1).unwrap(),
        },
        overrides: FloatingConventions::default(),
    });
    bond
}
fn rate_index(bond: &BondReferenceData) -> RateIndexDefinition {
    let terms = bond.contractual_terms.as_ref().unwrap();
    RateIndexDefinition {
        rate_index_id: ReferenceDataId::new("usd-sofr").unwrap(),
        index_id: IndexId::new("USD-SOFR").unwrap(),
        currency: bond.currency,
        kind: RateIndexKind::Overnight,
        fixing_source: SourceId::new("fixture-administrator").unwrap(),
        calendar: terms.calendar.clone(),
        calendar_code: terms.calendar_code,
        publication_lag_business_days: 1,
        publication_utc_seconds: 13 * 3600,
        conventions: FloatingConventions {
            lookback_business_days: Some(2),
            observation_shift: Some(false),
            lockout_business_days: Some(0),
            ..FloatingConventions::default()
        },
        effective: terms.effective,
        provenance: terms.provenance.clone(),
    }
}

#[test]
fn floating_mechanics_are_optional_bytes_and_belong_only_to_an_uncapped_frn() {
    let bullet: BondReferenceData =
        serde_json::from_str(include_str!("fixtures/reference_bond.json")).unwrap();
    // Absent mechanics are omitted, so stored records keep their exact bytes.
    let bytes = serde_json::to_value(&bullet).unwrap();
    assert!(bytes["contractual_terms"]
        .get("floating_mechanics")
        .is_none());
    let note = frn();
    validate_bond_reference(&note).unwrap();
    assert!(is_valued_floating(&note));
    let text = serde_json::to_value(&note).unwrap();
    assert_eq!(
        text["contractual_terms"]["floating_mechanics"],
        serde_json::json!({"index": {"id": "usd-sofr", "version": 1}, "overrides": {}})
    );
    // A payment lag is admitted for a floating rate note only.
    let mut lagged = bullet.clone();
    lagged.contractual_terms.as_mut().unwrap().payment_lag = 2;
    assert!(matches!(
        validate_bond_reference(&lagged),
        Err(ReferenceError::Unsupported(_))
    ));
    // Mechanics on a bullet, and overrides that mix overnight and term fields, are invalid.
    let mut wrong = bullet;
    wrong.contractual_terms.as_mut().unwrap().floating_mechanics = note
        .contractual_terms
        .as_ref()
        .unwrap()
        .floating_mechanics
        .clone();
    assert!(matches!(
        validate_bond_reference(&wrong),
        Err(ReferenceError::InvalidInput(_))
    ));
    let mut mixed = frn();
    let overrides = &mut mixed
        .contractual_terms
        .as_mut()
        .unwrap()
        .floating_mechanics
        .as_mut()
        .unwrap()
        .overrides;
    overrides.lookback_business_days = Some(2);
    overrides.fixing_offset_business_days = Some(-2);
    assert!(matches!(
        validate_bond_reference(&mixed),
        Err(ReferenceError::InvalidInput(_))
    ));
    // A capped note records but is not one the service values.
    let mut capped = frn();
    capped.floating_terms.as_mut().unwrap().cap = Some(rust_decimal::Decimal::new(8, 2));
    validate_bond_reference(&capped).unwrap();
    assert!(!is_valued_floating(&capped));
}

#[test]
fn rate_index_resources_bound_their_rule_and_keep_conventions_to_their_kind() {
    let note = frn();
    let valid = rate_index(&note);
    let definition = MasterDefinition::RateIndex(Box::new(valid.clone()));
    definition.validate().unwrap();
    assert_eq!(definition.key().storage_key(), "rate_index/usd-sofr");
    let invalid = [
        RateIndexDefinition {
            publication_lag_business_days: 11,
            ..valid.clone()
        },
        RateIndexDefinition {
            publication_utc_seconds: 86_400,
            ..valid.clone()
        },
        RateIndexDefinition {
            conventions: FloatingConventions {
                fixing_offset_business_days: Some(-2),
                ..valid.conventions.clone()
            },
            ..valid.clone()
        },
        RateIndexDefinition {
            conventions: FloatingConventions {
                lookback_business_days: Some(31),
                ..valid.conventions.clone()
            },
            ..valid
        },
    ];
    for definition in invalid {
        assert!(matches!(
            MasterDefinition::RateIndex(Box::new(definition)).validate(),
            Err(ReferenceError::InvalidInput(_))
        ));
    }
}

#[test]
fn floating_resources_must_be_the_pinned_index_of_the_same_kind_and_currency() {
    let note = frn();
    let record = |definition: MasterDefinition| MasterRecord {
        schema_version: SchemaVersion::V2,
        version: Version::new(1).unwrap(),
        recorded_at: UtcTimestamp::parse("2025-01-01T00:00:00Z").unwrap(),
        definition,
    };
    let calendar = || {
        let terms = note.contractual_terms.as_ref().unwrap();
        record(MasterDefinition::Calendar(Box::new(CalendarDefinition {
            calendar_id: terms.calendar.id.clone(),
            code: terms.calendar_code,
            rule_version: ModelId::new(SUPPORTED_BOND_CALENDAR_RULES).unwrap(),
            coverage_from: BusinessDate::parse("1900-01-01").unwrap(),
            coverage_until: BusinessDate::parse("2100-12-31").unwrap(),
            effective: terms.effective,
            provenance: terms.provenance.clone(),
        })))
    };
    let resolved = |index: RateIndexDefinition| ResolvedFloating {
        index: record(MasterDefinition::RateIndex(Box::new(index))),
        calendar: calendar(),
    };
    let lookup = ReferenceLookup {
        as_of: UtcTimestamp::parse("2026-10-02T15:00:00Z").unwrap(),
        known_at: UtcTimestamp::parse("2026-10-02T15:00:00Z").unwrap(),
    };
    validate_floating_resources(&note, &resolved(rate_index(&note)), lookup).unwrap();
    // The bond and its index cannot pin two revisions of one calendar.
    let mut later = resolved(RateIndexDefinition {
        calendar: ReferencePin {
            version: Version::new(2).unwrap(),
            ..rate_index(&note).calendar
        },
        ..rate_index(&note)
    });
    later.calendar.version = Version::new(2).unwrap();
    assert!(matches!(
        validate_floating_resources(&note, &later, lookup),
        Err(ReferenceError::InvalidInput(
            "the bond and its rate index pin different revisions of one calendar"
        ))
    ));
    // A EURIBOR note cannot use an overnight index resource.
    let mut euribor = frn();
    euribor.floating_terms.as_mut().unwrap().index =
        convex_core::ids::FloatingRateIndex::Euribor(convex_core::ids::Tenor::Months(3));
    for (bond, index) in [
        (
            note.clone(),
            RateIndexDefinition {
                rate_index_id: ReferenceDataId::new("usd-sofr-other").unwrap(),
                ..rate_index(&note)
            },
        ),
        (
            note.clone(),
            RateIndexDefinition {
                currency: convex_core::Currency::EUR,
                ..rate_index(&note)
            },
        ),
        (euribor, rate_index(&note)),
    ] {
        assert!(matches!(
            validate_floating_resources(&bond, &resolved(index), lookup),
            Err(ReferenceError::InvalidInput(_))
        ));
    }
}
