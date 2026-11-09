//! Portable behavioral scenarios for the authoritative reference master.
use convex_core::contracts::*;
use convex_ports::reference_data::BondReferenceData;
use convex_ports::reference_master::*;
use convex_ports::reference_repository::*;
use std::sync::Arc;

#[path = "reference_terms.rs"]
pub mod reference_terms;

pub fn time(value: &str) -> UtcTimestamp {
    UtcTimestamp::parse(value).unwrap()
}
pub fn version(value: u64) -> Version {
    Version::new(value).unwrap()
}
pub fn lookup(as_of: &str, known_at: &str) -> ReferenceLookup {
    ReferenceLookup {
        as_of: time(as_of),
        known_at: time(known_at),
    }
}
pub fn bond(id: &str) -> BondReferenceData {
    let mut bond: BondReferenceData =
        serde_json::from_str(include_str!("../fixtures/reference_bond.json")).unwrap();
    bond.instrument_id = convex_core::ids::InstrumentId::new(id);
    bond
}
pub fn command(
    key: &str,
    changes: Vec<ReferenceChange>,
    master_changes: Vec<MasterChange>,
) -> ReferenceCommand {
    ReferenceCommand {
        idempotency_key: RequestId::new(key).unwrap(),
        changes,
        master_changes,
    }
}
pub fn mapping(id: &str, identifier: ExternalIdentifier, target: ReferenceTarget) -> MasterChange {
    let terms = bond("TEMPLATE").contractual_terms.unwrap();
    MasterChange {
        expected_version: None,
        definition: MasterDefinition::Identifier(Box::new(IdentifierMapping {
            mapping_id: ReferenceDataId::new(id).unwrap(),
            identifier,
            target: Some(target),
            effective: terms.effective,
            provenance: terms.provenance,
        })),
    }
}
pub fn instrument(id: &str) -> ReferenceTarget {
    ReferenceTarget::Instrument {
        instrument_id: InstrumentId::new(id).unwrap(),
    }
}
pub async fn seed(repository: &Arc<dyn ReferenceRepository>, id: &str) -> ReferenceCommit {
    let bond = bond(id);
    repository
        .commit(
            &command(
                "master-seed",
                vec![ReferenceChange::Create {
                    bond: Box::new(bond.clone()),
                }],
                reference_terms::resources(&bond),
            ),
            time("2025-01-01T00:00:00Z"),
        )
        .await
        .unwrap()
}

pub async fn remap_preserves_identity_and_historical_resolution(
    repository: Arc<dyn ReferenceRepository>,
) {
    seed(&repository, "STABLE").await;
    let old_isin = ExternalIdentifier::Isin {
        value: "OLD-ISIN".into(),
    };
    let new_isin = ExternalIdentifier::Isin {
        value: "NEW-ISIN".into(),
    };
    let old_provider = ExternalIdentifier::Provider {
        namespace: SourceId::new("feed-a").unwrap(),
        value: "OLD.SYMBOL".into(),
    };
    let new_provider = ExternalIdentifier::Provider {
        namespace: SourceId::new("feed-a").unwrap(),
        value: "NEW.SYMBOL".into(),
    };
    let other = ExternalIdentifier::Provider {
        namespace: SourceId::new("feed-b").unwrap(),
        value: "ANOTHER/SYMBOL".into(),
    };
    let initial = vec![
        mapping("isin-old", old_isin.clone(), instrument("STABLE")),
        mapping("provider-old", old_provider.clone(), instrument("STABLE")),
        mapping("other-feed", other.clone(), instrument("STABLE")),
    ];
    let first = repository
        .commit(
            &command("initial-mappings", vec![], initial.clone()),
            time("2025-01-02T00:00:00Z"),
        )
        .await
        .unwrap();
    let mut remaps = vec![];
    for original in initial.into_iter().take(2) {
        let mut revised = original;
        revised.expected_version = Some(version(1));
        if let MasterDefinition::Identifier(mapping) = &mut revised.definition {
            mapping.effective.until = Some(time("2026-01-01T00:00:00Z"));
        }
        remaps.push(revised);
    }
    for (id, identifier) in [
        ("isin-new", new_isin.clone()),
        ("provider-new", new_provider.clone()),
    ] {
        let mut revised = mapping(id, identifier, instrument("STABLE"));
        if let MasterDefinition::Identifier(mapping) = &mut revised.definition {
            mapping.effective.from = time("2026-01-01T00:00:00Z");
        }
        remaps.push(revised);
    }
    let late = repository
        .commit(
            &command("late-remap", vec![], remaps),
            time("2026-02-01T00:00:00Z"),
        )
        .await
        .unwrap();
    let historic = lookup("2025-12-31T23:59:59.999999999Z", "2026-03-01T00:00:00Z");
    let before_known = lookup("2026-01-01T00:00:00Z", "2026-01-20T00:00:00Z");
    let corrected = lookup("2026-01-01T00:00:00Z", "2026-02-01T00:00:00Z");
    for old in [&old_isin, &old_provider] {
        let resolved = repository.resolve_at(old, historic).await.unwrap().unwrap();
        assert_eq!(resolved.target, instrument("STABLE"));
        assert_eq!(resolved.mapping.version, version(2));
        assert_eq!(resolved.target_version, version(1));
        assert_eq!(
            repository
                .resolve_at(old, before_known)
                .await
                .unwrap()
                .unwrap()
                .mapping
                .version,
            version(1)
        );
        assert!(repository
            .resolve_at(old, corrected)
            .await
            .unwrap()
            .is_none());
    }
    for new in [&new_isin, &new_provider] {
        assert!(repository
            .resolve_at(new, before_known)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            repository
                .resolve_at(new, corrected)
                .await
                .unwrap()
                .unwrap()
                .target,
            instrument("STABLE")
        );
    }
    assert_eq!(
        repository
            .resolve_at(&other, corrected)
            .await
            .unwrap()
            .unwrap()
            .target,
        instrument("STABLE")
    );
    for record in first.master_records {
        assert_eq!(
            repository
                .get_master_version(&record.definition.key(), record.version)
                .await
                .unwrap(),
            Some(record)
        );
    }
    assert_eq!(
        repository.get_commit(&late.idempotency_key).await.unwrap(),
        Some(late)
    );
    assert_eq!(
        repository
            .get(&convex_core::ids::InstrumentId::new("STABLE"))
            .await
            .unwrap()
            .unwrap()
            .version,
        version(1)
    );
}

pub async fn ambiguity_and_missing_terms_fail_closed(repository: Arc<dyn ReferenceRepository>) {
    seed(&repository, "FIRST").await;
    repository
        .commit(
            &command(
                "second",
                vec![ReferenceChange::Create {
                    bond: Box::new(bond("SECOND")),
                }],
                vec![],
            ),
            time("2025-01-02T00:00:00Z"),
        )
        .await
        .unwrap();
    let alias = ExternalIdentifier::Isin {
        value: "CONFLICT".into(),
    };
    repository
        .commit(
            &command(
                "conflicting-aliases",
                vec![],
                vec![
                    mapping("first-map", alias.clone(), instrument("FIRST")),
                    mapping("second-map", alias.clone(), instrument("SECOND")),
                ],
            ),
            time("2025-01-03T00:00:00Z"),
        )
        .await
        .unwrap();
    let context = lookup("2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z");
    assert_eq!(
        repository.resolve_at(&alias, context).await,
        Err(ReferenceError::AmbiguousIdentifier)
    );
    for (name, mutate) in [
        ("missing-terms", 0),
        ("unknown-day-count", 1),
        ("missing-coupon", 2),
        ("bad-pin", 3),
        ("wrong-issuer", 4),
        ("bad-dates", 5),
    ] {
        let mut invalid = bond(name);
        match mutate {
            0 => invalid.contractual_terms = None,
            1 => invalid.day_count = "UNKNOWN".into(),
            2 => invalid.coupon_rate = None,
            3 => invalid.contractual_terms.as_mut().unwrap().calendar.version = version(9),
            4 => invalid.issuer_id = "DIFFERENT".into(),
            _ => {
                invalid.contractual_terms.as_mut().unwrap().dated_date =
                    BusinessDate::try_from(invalid.maturity_date).unwrap()
            }
        }
        let request = command(
            name,
            vec![
                ReferenceChange::Create {
                    bond: Box::new(bond("VALID-PREFIX")),
                },
                ReferenceChange::Create {
                    bond: Box::new(invalid),
                },
            ],
            vec![],
        );
        assert!(matches!(
            repository.commit(&request, context.known_at).await,
            Err(ReferenceError::InvalidInput(_))
        ));
        assert!(repository
            .get(&convex_core::ids::InstrumentId::new("VALID-PREFIX"))
            .await
            .unwrap()
            .is_none());
        assert!(repository
            .get_commit(&request.idempotency_key)
            .await
            .unwrap()
            .is_none());
    }
    let mut missing_target = mapping(
        "missing-target",
        ExternalIdentifier::Cusip {
            value: "NO-TARGET".into(),
        },
        instrument("ABSENT"),
    );
    let request = command("missing-target", vec![], vec![missing_target.clone()]);
    assert!(matches!(
        repository.commit(&request, context.known_at).await,
        Err(ReferenceError::InvalidInput(_))
    ));
    if let MasterDefinition::Identifier(mapping) = &mut missing_target.definition {
        mapping.target = None;
    }
    repository
        .commit(
            &command("withdraw-uncommitted", vec![], vec![missing_target]),
            context.known_at,
        )
        .await
        .unwrap();
}

pub async fn schedule_correction_preserves_snapshot_replay(
    repository: Arc<dyn ReferenceRepository>,
) {
    let original = seed(&repository, "SCHEDULE").await;
    let before = lookup("2025-06-01T00:00:00Z", "2025-06-01T00:00:00Z");
    let pinned = repository
        .for_valuation(&convex_core::ids::InstrumentId::new("SCHEDULE"), before)
        .await
        .unwrap()
        .unwrap();
    let mut revised = bond("SCHEDULE");
    revised.first_coupon_date = Some(convex_core::Date::parse("2020-01-15").unwrap());
    revised.contractual_terms.as_mut().unwrap().stub = BondStub::ShortFirst;
    revised.contractual_terms.as_mut().unwrap().dated_date =
        BusinessDate::parse("2019-12-15").unwrap();
    revised.contractual_terms.as_mut().unwrap().redemption_value =
        ExactDecimal::parse("102.50").unwrap();
    let request = command(
        "corrected-schedule",
        vec![ReferenceChange::Update {
            bond: Box::new(revised.clone()),
            expected_version: version(1),
        }],
        vec![],
    );
    let after = repository
        .commit(&request, time("2025-07-01T00:00:00Z"))
        .await
        .unwrap();
    assert_eq!(after.records[0].version, version(2));
    assert_eq!(
        repository
            .get_at(&revised.instrument_id, before)
            .await
            .unwrap(),
        Some(original.records[0].clone())
    );
    assert_eq!(
        repository
            .for_valuation(&revised.instrument_id, before)
            .await
            .unwrap(),
        Some(pinned.clone())
    );
    assert_eq!(
        repository
            .get_at(
                &revised.instrument_id,
                ReferenceLookup {
                    known_at: time("2025-07-01T00:00:00Z"),
                    ..before
                }
            )
            .await
            .unwrap(),
        Some(after.records[0].clone())
    );
    assert_eq!(
        repository
            .get_version(&revised.instrument_id, pinned.record.version)
            .await
            .unwrap(),
        Some(original.records[0].clone())
    );
    assert_eq!(
        repository
            .commit(&request, time("2026-01-01T00:00:00Z"))
            .await
            .unwrap(),
        after
    );
    assert_eq!(
        repository
            .get_master_version(&pinned.calendar.definition.key(), pinned.calendar.version)
            .await
            .unwrap(),
        Some(pinned.calendar)
    );
    let retrograde = command(
        "bad-clock",
        vec![ReferenceChange::Update {
            bond: Box::new(revised),
            expected_version: version(2),
        }],
        vec![],
    );
    assert!(matches!(
        repository
            .commit(&retrograde, time("2025-06-01T00:00:00Z"))
            .await,
        Err(ReferenceError::InvalidInput(_))
    ));
}

pub async fn entity_resources_and_corrections_are_pinned(repository: Arc<dyn ReferenceRepository>) {
    let original = seed(&repository, "ENTITY-BOND").await;
    let old_entity = original.master_records[0].clone();
    let MasterDefinition::Entity(mut entity) = old_entity.definition.clone() else {
        panic!();
    };
    let lei = ExternalIdentifier::Lei {
        value: "FICTIONAL-LEI".into(),
    };
    repository
        .commit(
            &command(
                "lei",
                vec![],
                vec![mapping(
                    "lei-map",
                    lei.clone(),
                    ReferenceTarget::Entity {
                        entity_id: entity.entity_id.clone(),
                    },
                )],
            ),
            time("2025-01-02T00:00:00Z"),
        )
        .await
        .unwrap();
    entity.legal_name = "Corrected legal name".into();
    entity.effective.from = time("2025-06-01T00:00:00Z");
    let key = MasterKey::Entity(entity.entity_id.clone());
    repository
        .commit(
            &command(
                "entity-correction",
                vec![],
                vec![MasterChange {
                    expected_version: Some(version(1)),
                    definition: MasterDefinition::Entity(entity.clone()),
                }],
            ),
            time("2025-07-01T00:00:00Z"),
        )
        .await
        .unwrap();
    let old = lookup("2025-06-15T00:00:00Z", "2025-06-15T00:00:00Z");
    let new = ReferenceLookup {
        known_at: time("2025-07-01T00:00:00Z"),
        ..old
    };
    assert_eq!(
        repository
            .resolve_at(&lei, old)
            .await
            .unwrap()
            .unwrap()
            .target_version,
        version(1)
    );
    assert_eq!(
        repository
            .resolve_at(&lei, new)
            .await
            .unwrap()
            .unwrap()
            .target_version,
        version(2)
    );
    // Existing bond consumes its original issuer pin, not the latest legal attributes.
    assert_eq!(
        repository
            .for_valuation(&convex_core::ids::InstrumentId::new("ENTITY-BOND"), new)
            .await
            .unwrap()
            .unwrap()
            .issuer,
        old_entity
    );
    assert_eq!(
        repository
            .get_master_version(&key, version(1))
            .await
            .unwrap(),
        Some(original.master_records[0].clone())
    );
    entity.parent = Some(EntityPin {
        entity_id: entity.entity_id.clone(),
        version: version(1),
    });
    assert!(matches!(
        repository
            .commit(
                &command(
                    "self-parent",
                    vec![],
                    vec![MasterChange {
                        expected_version: Some(version(2)),
                        definition: MasterDefinition::Entity(entity)
                    }]
                ),
                new.known_at
            )
            .await,
        Err(ReferenceError::InvalidInput(_))
    ));
    let mut calendar = original.master_records[1].clone();
    let MasterDefinition::Entity(template) = original.master_records[0].definition.clone() else {
        panic!();
    };
    let mut a = template.clone();
    let mut b = template;
    a.entity_id = EntityId::new("CYCLE-A").unwrap();
    b.entity_id = EntityId::new("CYCLE-B").unwrap();
    a.parent = Some(EntityPin {
        entity_id: b.entity_id.clone(),
        version: version(1),
    });
    b.parent = Some(EntityPin {
        entity_id: a.entity_id.clone(),
        version: version(1),
    });
    let cycle = command(
        "parent-cycle",
        vec![],
        vec![
            MasterChange {
                expected_version: None,
                definition: MasterDefinition::Entity(a.clone()),
            },
            MasterChange {
                expected_version: None,
                definition: MasterDefinition::Entity(b),
            },
        ],
    );
    assert!(matches!(
        repository.commit(&cycle, new.known_at).await,
        Err(ReferenceError::InvalidInput(_))
    ));
    assert!(repository
        .get_master(&MasterKey::Entity(a.entity_id))
        .await
        .unwrap()
        .is_none());
    assert!(repository
        .get_commit(&cycle.idempotency_key)
        .await
        .unwrap()
        .is_none());

    let MasterDefinition::Calendar(mut unsupported) = calendar.definition.clone() else {
        panic!();
    };
    unsupported.calendar_id = ReferenceDataId::new("future-calendar").unwrap();
    unsupported.rule_version = ModelId::new("unimplemented-calendar-v9").unwrap();
    let mut future_bond = bond("UNSUPPORTED-CALENDAR");
    future_bond.contractual_terms.as_mut().unwrap().calendar.id = unsupported.calendar_id.clone();
    repository
        .commit(
            &command(
                "stage-future-rule",
                vec![ReferenceChange::Create {
                    bond: Box::new(future_bond.clone()),
                }],
                vec![MasterChange {
                    expected_version: None,
                    definition: MasterDefinition::Calendar(unsupported),
                }],
            ),
            new.known_at,
        )
        .await
        .unwrap();
    assert!(matches!(
        repository
            .for_valuation(&future_bond.instrument_id, new)
            .await,
        Err(ReferenceError::Unsupported(_))
    ));
    if let MasterDefinition::Calendar(cal) = &mut calendar.definition {
        cal.code = BondCalendar::Target2;
    }
    repository
        .commit(
            &command(
                "calendar-correction",
                vec![],
                vec![MasterChange {
                    expected_version: Some(version(1)),
                    definition: calendar.definition,
                }],
            ),
            new.known_at,
        )
        .await
        .unwrap();
    assert_eq!(
        repository
            .for_valuation(&convex_core::ids::InstrumentId::new("ENTITY-BOND"), new)
            .await
            .unwrap()
            .unwrap()
            .calendar,
        original.master_records[1]
    );
    let mut incompatible = bond("BAD-CALENDAR");
    incompatible
        .contractual_terms
        .as_mut()
        .unwrap()
        .calendar
        .version = version(2);
    assert!(matches!(
        repository
            .commit(
                &command(
                    "incompatible-calendar",
                    vec![ReferenceChange::Create {
                        bond: Box::new(incompatible)
                    }],
                    vec![]
                ),
                new.known_at
            )
            .await,
        Err(ReferenceError::InvalidInput(_))
    ));
}

pub async fn master_compare_and_set_race(repository: Arc<dyn ReferenceRepository>) {
    let initial = seed(&repository, "RACE-BOND").await;
    let original = initial.master_records[0].definition.clone();
    let barrier = Arc::new(tokio::sync::Barrier::new(16));
    let mut tasks = Vec::new();
    for n in 0..16 {
        let repository = repository.clone();
        let barrier = barrier.clone();
        let mut definition = original.clone();
        if let MasterDefinition::Entity(entity) = &mut definition {
            entity.legal_name = format!("writer-{n}");
        }
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            repository
                .commit(
                    &command(
                        &format!("master-race-{n}"),
                        vec![],
                        vec![MasterChange {
                            expected_version: Some(version(1)),
                            definition,
                        }],
                    ),
                    time("2025-01-02T00:00:00Z"),
                )
                .await
        }));
    }
    let mut winners = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(r) => {
                winners += 1;
                assert_eq!(r.master_records[0].version, version(2));
            }
            Err(ReferenceError::Conflict(_)) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(winners, 1);
    assert!(repository
        .get_master_version(&original.key(), version(3))
        .await
        .unwrap()
        .is_none());
}

fn book(id: &str, instrument: &str, until: Option<&str>) -> MasterChange {
    MasterChange {
        expected_version: None,
        definition: MasterDefinition::Book(Box::new(BookDefinition {
            book_id: ReferenceDataId::new(id).unwrap(),
            owning_system: SourceId::new("bms").unwrap(),
            name: None,
            currency: Some("USD".into()),
            description: None,
            effective: EffectiveInterval {
                from: time("2025-01-01T00:00:00Z"),
                until: until.map(time),
            },
            positions: vec![BookPosition {
                instrument_id: InstrumentId::new(instrument).unwrap(),
                quantity: ExactDecimal::parse("1000000.50").unwrap(),
                unit: QuantityUnit::Face,
                reference_version: None,
                source_identifier: None,
                sector: None,
                rating: None,
            }],
            provenance: ReferenceProvenance {
                source: SourceId::new("bms").unwrap(),
                evidence_reference: "conformance".into(),
            },
        })),
    }
}

/// Books are ordinary versioned masters: ordered cursor paging, compare-and-set,
/// exact history and a typed rejection of duplicate instruments or an oversized book.
pub async fn books_page_in_order_and_version_with_compare_and_set(
    repository: Arc<dyn ReferenceRepository>,
) {
    let at = time("2025-01-02T00:00:00Z");
    let changes = vec![
        book("B-3", "I-3", None),
        book("B-1", "I-1", None),
        book("B-2", "I-2", None),
    ];
    repository
        .commit(&command("books-seed", vec![], changes), at)
        .await
        .unwrap();
    let id = |value: &str| ReferenceDataId::new(value).unwrap();
    let first = repository.list_books(None, 2).await.unwrap();
    let keys: Vec<_> = first.iter().map(|r| r.definition.key()).collect();
    assert_eq!(
        keys,
        vec![MasterKey::Book(id("B-1")), MasterKey::Book(id("B-2"))]
    );
    let rest = repository.list_books(Some(&id("B-2")), 2).await.unwrap();
    assert_eq!(
        rest.iter().map(|r| r.definition.key()).collect::<Vec<_>>(),
        vec![MasterKey::Book(id("B-3"))]
    );
    assert!(repository.list_books(None, 0).await.is_err());

    // Replace version 1 once; the stale expectation then conflicts and history stays exact.
    let mut next = book("B-1", "I-9", None);
    next.expected_version = Some(version(1));
    repository
        .commit(&command("books-v2", vec![], vec![next.clone()]), at)
        .await
        .unwrap();
    next.definition = book("B-1", "I-8", None).definition;
    assert!(matches!(
        repository
            .commit(&command("books-v3", vec![], vec![next]), at)
            .await,
        Err(ReferenceError::Conflict(_))
    ));
    let key = MasterKey::Book(id("B-1"));
    let v1 = repository
        .get_master_version(&key, version(1))
        .await
        .unwrap()
        .unwrap();
    let MasterDefinition::Book(old) = v1.definition else {
        panic!("not a book")
    };
    assert_eq!(old.positions[0].instrument_id.as_str(), "I-1");
    assert_eq!(
        repository.get_master(&key).await.unwrap().unwrap().version,
        version(2)
    );

    // Bounds are typed: duplicates and over-capacity books never commit.
    let MasterDefinition::Book(mut duplicate) = book("B-D", "I-1", None).definition else {
        panic!("not a book")
    };
    duplicate.positions.push(duplicate.positions[0].clone());
    let invalid = |definition: Box<BookDefinition>| MasterChange {
        expected_version: None,
        definition: MasterDefinition::Book(definition),
    };
    assert!(matches!(
        repository
            .commit(&command("books-dup", vec![], vec![invalid(duplicate)]), at)
            .await,
        Err(ReferenceError::InvalidInput(_))
    ));
    assert!(repository
        .get_master(&MasterKey::Book(id("B-D")))
        .await
        .unwrap()
        .is_none());
    let MasterDefinition::Book(mut large) = book("B-L", "I-1", None).definition else {
        panic!("not a book")
    };
    let template = large.positions[0].clone();
    large.positions = (0..=MAX_BOOK_POSITIONS)
        .map(|n| BookPosition {
            instrument_id: InstrumentId::new(format!("I-{n}")).unwrap(),
            ..template.clone()
        })
        .collect();
    assert_eq!(
        repository
            .commit(&command("books-big", vec![], vec![invalid(large)]), at)
            .await
            .err(),
        Some(ReferenceError::CapacityExceeded)
    );
    assert!(repository
        .get_master(&MasterKey::Book(id("B-L")))
        .await
        .unwrap()
        .is_none());
}

fn limits(upper: &str) -> Vec<convex_ports::limits::LimitDefinition> {
    serde_json::from_value(serde_json::json!([
        {"limit_id": "face", "measure": {"kind": "face", "instrument": "each"}, "upper": upper}
    ]))
    .unwrap()
}

fn proposal_change(
    expected: Option<Version>,
    upper: &str,
    closed: Option<convex_ports::limits::LimitProposalClosure>,
) -> MasterChange {
    let from = time("2025-01-02T00:00:00Z");
    MasterChange {
        expected_version: expected,
        definition: MasterDefinition::LimitProposal(Box::new(
            convex_ports::limits::LimitProposalDefinition {
                book_id: ReferenceDataId::new("B-L1").unwrap(),
                base_version: None,
                limits: limits(upper),
                proposed_by: "access:proposer".into(),
                effective: EffectiveInterval {
                    from,
                    until: closed.as_ref().map(|_| time("2025-01-03T00:00:00Z")),
                },
                closed,
                provenance: ReferenceProvenance {
                    source: SourceId::new("limits-api").unwrap(),
                    evidence_reference: "limits-api:conformance".into(),
                },
            },
        )),
    }
}

fn set_change(expected: Option<Version>, upper: &str) -> MasterChange {
    MasterChange {
        expected_version: expected,
        definition: MasterDefinition::LimitSet(Box::new(
            convex_ports::limits::LimitSetDefinition {
                book_id: ReferenceDataId::new("B-L1").unwrap(),
                limits: limits(upper),
                proposed_by: "access:proposer".into(),
                approved_by: "access:approver".into(),
                proposal_version: Some(version(1)),
                effective: EffectiveInterval {
                    from: time("2025-01-03T00:00:00Z"),
                    until: None,
                },
                provenance: ReferenceProvenance {
                    source: SourceId::new("limits-api").unwrap(),
                    evidence_reference: "limits-api:conformance".into(),
                },
            },
        )),
    }
}

/// P14b (D82, D85): an approval commits the limit set and closes its proposal in one
/// transaction, or neither; both kinds list on their own, never as books.
pub async fn limit_sets_and_proposals_commit_together_and_list_by_kind(
    repository: Arc<dyn ReferenceRepository>,
) {
    use convex_ports::limits::LimitProposalClosure;
    let approved = || {
        Some(LimitProposalClosure::Approved {
            by: "access:approver".into(),
            limit_set_version: version(1),
        })
    };
    repository
        .commit(
            &command(
                "limits-propose",
                vec![],
                vec![proposal_change(None, "5", None)],
            ),
            time("2025-01-02T00:00:00Z"),
        )
        .await
        .unwrap();
    let receipt = repository
        .commit(
            &command(
                "limits-approve",
                vec![],
                vec![
                    set_change(None, "5"),
                    proposal_change(Some(version(1)), "5", approved()),
                ],
            ),
            time("2025-01-03T00:00:00Z"),
        )
        .await
        .unwrap();
    assert_eq!(receipt.master_records.len(), 2);
    let id = || ReferenceDataId::new("B-L1").unwrap();

    // A stale proposal expectation refuses the whole approval: the set does not move.
    assert!(matches!(
        repository
            .commit(
                &command(
                    "limits-stale",
                    vec![],
                    vec![
                        set_change(Some(version(1)), "6"),
                        proposal_change(Some(version(1)), "6", approved()),
                    ],
                ),
                time("2025-01-04T00:00:00Z"),
            )
            .await,
        Err(ReferenceError::Conflict(_))
    ));
    let set = repository
        .get_master(&MasterKey::LimitSet(id()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(set.version, version(1));
    let proposal = repository
        .get_master(&MasterKey::LimitProposal(id()))
        .await
        .unwrap()
        .unwrap();
    let MasterDefinition::LimitProposal(closed) = &proposal.definition else {
        panic!("not a proposal")
    };
    assert_eq!((proposal.version, closed.is_open()), (version(2), false));

    let sets = repository
        .list_masters(MasterKind::LimitSet, None, 10)
        .await
        .unwrap();
    let proposals = repository
        .list_masters(MasterKind::LimitProposal, None, 10)
        .await
        .unwrap();
    assert_eq!(sets, vec![set]);
    assert_eq!(proposals, vec![proposal]);
    assert!(repository.list_books(None, 10).await.unwrap().is_empty());

    // A proposal ends exactly when it closes; a set holds at most 64 limits.
    let mut open_ended = proposal_change(Some(version(2)), "7", approved());
    if let MasterDefinition::LimitProposal(p) = &mut open_ended.definition {
        p.effective.until = None;
    }
    let mut oversized = set_change(Some(version(1)), "7");
    if let MasterDefinition::LimitSet(s) = &mut oversized.definition {
        s.limits = (0..65)
            .map(|i| {
                let mut limit = limits("7").remove(0);
                limit.limit_id = ReferenceDataId::new(format!("face-{i}")).unwrap();
                limit
            })
            .collect();
    }
    for (key, change) in [
        ("limits-open-ended", open_ended),
        ("limits-oversized", oversized),
    ] {
        assert!(
            matches!(
                repository
                    .commit(
                        &command(key, vec![], vec![change]),
                        time("2025-01-05T00:00:00Z")
                    )
                    .await,
                Err(ReferenceError::InvalidInput(_))
            ),
            "{key}"
        );
    }
}

fn acknowledgements_change(expected: Option<Version>, entries: usize) -> MasterChange {
    let from = time("2025-01-06T00:00:00Z");
    let acknowledgements = (0..entries)
        .map(|i| {
            let mut limit = limits("5").remove(0);
            limit.limit_id = ReferenceDataId::new(format!("face-{i}")).unwrap();
            convex_ports::limits::LimitAcknowledgement {
                limit,
                limit_set_version: version(1),
                acknowledged_by: "access:risk".into(),
                acknowledged_at: from,
                expires_at: time("2025-01-07T00:00:00Z"),
                note: "seen".into(),
                record_id: None,
            }
        })
        .collect();
    MasterChange {
        expected_version: expected,
        definition: MasterDefinition::LimitAcknowledgements(Box::new(
            convex_ports::limits::LimitAcknowledgementsDefinition {
                book_id: ReferenceDataId::new("B-L1").unwrap(),
                acknowledgements,
                change: convex_ports::limits::LimitAcknowledgementChange::Acknowledged {
                    limit_id: ReferenceDataId::new("face-0").unwrap(),
                },
                effective: EffectiveInterval { from, until: None },
                provenance: ReferenceProvenance {
                    source: SourceId::new("limits-api").unwrap(),
                    evidence_reference: "limits-api:conformance".into(),
                },
            },
        )),
    }
}

/// P15c (D94): a book's acknowledgements are one compare-and-set record of their own kind,
/// listed on their own, holding at most 64 entries.
pub async fn limit_acknowledgements_compare_and_set_and_list_by_kind(
    repository: Arc<dyn ReferenceRepository>,
) {
    let at = time("2025-01-06T00:00:00Z");
    for (key, change) in [
        ("acks-1", acknowledgements_change(None, 1)),
        ("acks-2", acknowledgements_change(Some(version(1)), 2)),
    ] {
        repository
            .commit(&command(key, vec![], vec![change]), at)
            .await
            .unwrap();
    }
    for (key, change, refused) in [
        (
            "acks-stale",
            acknowledgements_change(Some(version(1)), 3),
            "conflict",
        ),
        (
            "acks-oversized",
            acknowledgements_change(Some(version(2)), 65),
            "invalid",
        ),
    ] {
        let result = repository
            .commit(&command(key, vec![], vec![change]), at)
            .await;
        assert!(
            match refused {
                "conflict" => matches!(result, Err(ReferenceError::Conflict(_))),
                _ => matches!(result, Err(ReferenceError::InvalidInput(_))),
            },
            "{key}: {result:?}"
        );
    }
    let key = MasterKey::LimitAcknowledgements(ReferenceDataId::new("B-L1").unwrap());
    let current = repository.get_master(&key).await.unwrap().unwrap();
    assert_eq!(current.version, version(2));
    let first = repository
        .get_master_version(&key, version(1))
        .await
        .unwrap()
        .unwrap();
    let MasterDefinition::LimitAcknowledgements(first) = &first.definition else {
        panic!("not acknowledgements")
    };
    assert_eq!(first.acknowledgements.len(), 1);
    assert_eq!(
        repository
            .list_masters(MasterKind::LimitAcknowledgements, None, 10)
            .await
            .unwrap(),
        vec![current]
    );
    assert!(repository
        .list_masters(MasterKind::LimitSet, None, 10)
        .await
        .unwrap()
        .is_empty());
}
