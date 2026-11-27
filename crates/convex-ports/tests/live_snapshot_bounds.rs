//! A Live snapshot of 4,096 instruments at the largest content the
//! validators admit fits the Live identity bound.
#[path = "support/configuration_fixtures.rs"]
mod configuration;

use configuration::snapshot as fixture;
use convex_core::contracts::*;
use convex_ports::{
    level_pricing::MAX_SNAPSHOT_BENCHMARK_SETS, quote_event::*, reference_master::*, snapshot::*,
    TraitError,
};
use std::collections::BTreeMap;

fn text(fill: char, len: usize) -> String {
    std::iter::repeat_n(fill, len).collect()
}

/// A 128-character identifier ending in `n`.
fn id(prefix: char, n: usize) -> String {
    let suffix = format!("{n:06}");
    format!("{}{suffix}", text(prefix, 128 - suffix.len()))
}

fn evidence() -> String {
    text('e', 512)
}

fn provenance() -> ReferenceProvenance {
    ReferenceProvenance {
        source: SourceId::new(id('s', 0)).unwrap(),
        evidence_reference: evidence(),
    }
}

/// The fixture's reference with every bounded text at its bound.
fn largest_reference(n: usize) -> ResolvedBondReference {
    let mut reference = fixture::content().references[0].clone();
    let instrument = convex_core::ids::InstrumentId::new(id('i', n));
    reference.record.instrument_id = instrument.clone();
    let bond = reference.record.bond.as_mut().unwrap();
    bond.instrument_id = instrument;
    for value in [
        &mut bond.description,
        &mut bond.issuer_id,
        &mut bond.issuer_name,
        &mut bond.day_count,
        &mut bond.seniority,
        &mut bond.country_of_risk,
        &mut bond.sector,
        &mut bond.source,
    ] {
        *value = text('t', 1024);
    }
    for value in [
        &mut bond.isin,
        &mut bond.cusip,
        &mut bond.sedol,
        &mut bond.bbgid,
    ] {
        *value = Some(text('x', 128));
    }
    let terms = bond.contractual_terms.as_mut().unwrap();
    terms.provenance = provenance();
    terms.issuer.entity_id = EntityId::new(id('n', 0)).unwrap();
    terms.calendar.id = ReferenceDataId::new(id('c', 0)).unwrap();
    terms.convention.id = ReferenceDataId::new(id('v', 0)).unwrap();
    for record in [
        &mut reference.issuer,
        &mut reference.calendar,
        &mut reference.convention,
    ] {
        match &mut record.definition {
            MasterDefinition::Entity(entity) => {
                entity.entity_id = EntityId::new(id('n', 0)).unwrap();
                entity.legal_name = text('l', 1024);
                entity.country = text('k', 128);
                entity.provenance = provenance();
            }
            MasterDefinition::Calendar(calendar) => {
                calendar.calendar_id = ReferenceDataId::new(id('c', 0)).unwrap();
                calendar.rule_version = ModelId::new(id('r', 0)).unwrap();
                calendar.provenance = provenance();
            }
            MasterDefinition::Convention(convention) => {
                convention.convention_id = ReferenceDataId::new(id('v', 0)).unwrap();
                convention.provenance = provenance();
            }
            other => panic!("unexpected resource {other:?}"),
        }
    }
    reference
}

/// The longest decimal one quote value writes.
fn longest(field: QuoteField) -> QuoteValue {
    let digits = if field.unit() == QuoteUnit::FaceAmount {
        "7.9228162514264337593543950335"
    } else {
        "-7.9228162514264337593543950335"
    };
    QuoteValue {
        value: ExactDecimal::parse(digits).unwrap().decimal(),
        unit: field.unit(),
    }
}

const FIELDS: [QuoteField; MAX_QUOTE_FIELDS] = [
    QuoteField::BidPrice,
    QuoteField::AskPrice,
    QuoteField::MidPrice,
    QuoteField::LastPrice,
    QuoteField::BidYield,
    QuoteField::AskYield,
    QuoteField::MidYield,
    QuoteField::BidSpread,
    QuoteField::AskSpread,
    QuoteField::MidSpread,
    QuoteField::BidSize,
    QuoteField::AskSize,
];

/// A quote entry with every field, and an event at every text bound, written twice.
fn largest_quote(n: usize, at_ms: i64, quality: QuoteQuality) -> SharedQuote {
    let key = QuoteKey {
        instrument_id: convex_core::ids::InstrumentId::new(id('i', n)),
        source: id('f', 0),
    };
    let event = QuoteEvent {
        schema_version: 1,
        key: key.clone(),
        session: id('q', 0),
        sequence: u64::MAX,
        source_time_ms: at_ms,
        receive_time_ms: at_ms,
        quality,
        entitled: true,
        entitlement_tags: (0..16).map(|t| id('g', t)).collect(),
        evidence_reference: Some(evidence()),
        event: QuoteEventKind::Image {
            fields: FIELDS
                .iter()
                .map(|&field| QuoteChange {
                    field,
                    update: FieldUpdate::Set(longest(field)),
                })
                .collect(),
        },
    };
    event.validate().unwrap();
    SnapshotQuote {
        requirement: QuoteRequirement {
            key,
            fields: FIELDS.to_vec(),
            max_age_ms: 86_400_000,
            quoted_by: None,
        },
        delivery_watermark: Some(event.clone()),
        image: Some(QuoteSnapshot {
            fields: FIELDS
                .iter()
                .map(|&field| {
                    (
                        field,
                        FieldObservation {
                            value: longest(field),
                            source_time_ms: at_ms,
                            receive_time_ms: at_ms,
                            sequence: u64::MAX,
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
            watermark: event,
            state: DataState::Live,
        }),
    }
    .into()
}

fn most_conventions() -> FloatingConventions {
    FloatingConventions {
        lookback_business_days: Some(30),
        observation_shift: Some(true),
        lockout_business_days: Some(30),
        fixing_offset_business_days: Some(-10),
        projection_day_count: Some(convex_core::daycounts::DayCountConvention::ActActIcma),
    }
}

/// [`largest_reference`] as a floating rate note: its terms, its mechanics, and its
/// index and the index's calendar, each at its bounds.
fn largest_note(n: usize) -> ResolvedBondReference {
    let mut reference = largest_reference(n);
    let bond = reference.record.bond.as_mut().unwrap();
    let longest = ExactDecimal::parse("-7.9228162514264337593543950335")
        .unwrap()
        .decimal();
    bond.floating_terms = Some(convex_ports::reference_data::FloatingRateTerms {
        index: convex_core::ids::FloatingRateIndex::Other(text('o', 128)),
        spread: longest,
        reset_frequency: 12,
        cap: Some(longest),
        floor: Some(longest),
    });
    let terms = bond.contractual_terms.as_mut().unwrap();
    let pin = |prefix| ReferencePin {
        id: ReferenceDataId::new(id(prefix, 0)).unwrap(),
        version: Version::new(u64::MAX).unwrap(),
    };
    terms.floating_mechanics = Some(FloatingMechanics {
        index: pin('x'),
        overrides: most_conventions(),
    });
    let mut calendar = reference.calendar.clone();
    if let MasterDefinition::Calendar(definition) = &mut calendar.definition {
        definition.calendar_id = ReferenceDataId::new(id('w', 0)).unwrap();
    }
    let effective = terms.effective;
    reference.floating = Some(ResolvedFloating {
        index: MasterRecord {
            definition: MasterDefinition::RateIndex(Box::new(RateIndexDefinition {
                rate_index_id: ReferenceDataId::new(id('x', 0)).unwrap(),
                index_id: IndexId::new(id('y', 0)).unwrap(),
                currency: convex_core::Currency::USD,
                kind: RateIndexKind::Overnight,
                fixing_source: SourceId::new(id('z', 0)).unwrap(),
                calendar: pin('w'),
                calendar_code: BondCalendar::Sifma,
                publication_lag_business_days: 10,
                publication_utc_seconds: 86_399,
                conventions: most_conventions(),
                effective,
                provenance: provenance(),
            })),
            ..reference.calendar.clone()
        },
        calendar,
    });
    reference
}

/// [`largest_reference`] with the most calls a schedule holds, as one of a plan's eight
/// callables.
fn largest_callable(n: usize) -> ResolvedBondReference {
    let mut reference = largest_reference(n);
    let bond = reference.record.bond.as_mut().unwrap();
    let price = ExactDecimal::parse("7.9228162514264337593543950335")
        .unwrap()
        .decimal();
    bond.is_callable = true;
    bond.call_schedule = (0..1024)
        .map(|_| convex_ports::reference_data::CallScheduleEntry {
            call_date: bond.maturity_date.unwrap(),
            call_price: price,
            is_make_whole: false,
        })
        .collect();
    bond.contractual_terms.as_mut().unwrap().call_terms = Some(CallTerms {
        style: CallStyle::Bermudan,
        notice_days: 366,
    });
    reference
}

fn bytes<T: serde::Serialize + ?Sized>(value: &T) -> usize {
    serde_json::to_vec(value).unwrap().len()
}

/// The JSON a Live snapshot's identity hashes, for 4,096 instruments with every text at
/// the bound its validator sets: floating rate notes, with an index and its calendar, but
/// for the eight callables a plan values, each with 1,024 calls, every instrument with a
/// pricing rule, 64 benchmark sets of 32 securities, and each quote with every field and
/// an event at every bound, written twice. Some of these values no bond could hold
/// together (a day count of 1,024 bytes, a note with a cap), so this is above the largest
/// content validation admits: 132,875,866 bytes with the fixture's frame, 126.7 MiB. It
/// fits the Live bound, with 1.3 MiB left for a larger frame, and would not fit a stored
/// snapshot's.
#[test]
fn the_largest_admitted_instruments_fit_the_live_identity_bound() {
    let content = fixture::content();
    let at_ms = content.as_of.datetime().timestamp_millis();
    let mut note = largest_note(1);
    let alone = bytes(&note);
    note.pricing_rule = Some(largest_rule());
    let note = bytes(&note);
    let callable = bytes(&largest_callable(1)) + (note - alone);
    let quote = bytes(&*largest_quote(1, at_ms, QuoteQuality::Indicative));
    assert!(callable > note, "{callable} {note}");
    let mut frame = content;
    frame.references = SnapshotReferences::new(vec![]);
    frame.quotes.clear();
    frame.benchmark_sets = (0..MAX_SNAPSHOT_BENCHMARK_SETS).map(largest_set).collect();
    let instruments = MAX_LIVE_SNAPSHOT_INSTRUMENTS;
    // The references are one array: a comma between records, and its brackets.
    let total = (instruments - 8) * note
        + 8 * callable
        + instruments
        + 1
        + instruments * quote
        + bytes(&frame);
    assert!(
        total <= MAX_LIVE_IDENTITY_BYTES && total > MAX_SNAPSHOT_BYTES,
        "{total}: a note {note}, a callable {callable}, a quote {quote}"
    );
}

/// A pricing rule pin at its bounds: a benchmark spread at the longest tenor over a set
/// whose id is at its bound.
fn largest_rule() -> convex_ports::level_pricing::ResolvedPricingRule {
    convex_ports::level_pricing::ResolvedPricingRule {
        version: Version::new(u64::MAX).unwrap(),
        recorded_at: fixture::time("2025-01-01T00:00:00Z"),
        method: convex_ports::level_pricing::PricingMethod::BenchmarkSpread {
            tenor: "600M".into(),
        },
        benchmark_set_id: Some(ReferenceDataId::new(id('b', 0)).unwrap()),
        tenor_basis: Default::default(),
    }
}

/// Benchmark set `n` at its bounds: 32 securities, each id at its bound.
fn largest_set(n: usize) -> convex_ports::level_pricing::ResolvedBenchmarkSet {
    use convex_ports::level_pricing::{BenchmarkSecurity, ResolvedBenchmarkSet};
    ResolvedBenchmarkSet {
        benchmark_set_id: ReferenceDataId::new(id('b', n)).unwrap(),
        version: Version::new(u64::MAX).unwrap(),
        recorded_at: fixture::time("2025-01-01T00:00:00Z"),
        securities: (0..convex_ports::level_pricing::MAX_BENCHMARK_SECURITIES)
            .map(|security| BenchmarkSecurity {
                tenor: format!("{}M", 500 + security),
                instrument_id: InstrumentId::new(id('t', security)).unwrap(),
                version: Version::new(u64::MAX).unwrap(),
            })
            .collect(),
    }
}

/// The fixture's reference under instrument `n`, with each free text at its bound: what
/// validation admits of [`largest_reference`].
fn large_reference(n: usize) -> ResolvedBondReference {
    let mut reference = fixture::content().references[0].clone();
    let instrument = convex_core::ids::InstrumentId::new(id('i', n));
    reference.record.instrument_id = instrument.clone();
    let bond = reference.record.bond.as_mut().unwrap();
    bond.instrument_id = instrument;
    for value in [
        &mut bond.description,
        &mut bond.seniority,
        &mut bond.sector,
        &mut bond.source,
    ] {
        *value = text('t', 1024);
    }
    for value in [
        &mut bond.isin,
        &mut bond.cusip,
        &mut bond.sedol,
        &mut bond.bbgid,
    ] {
        *value = Some(text('x', 128));
    }
    reference
}

/// Schema-4 content of `instruments` large instruments, each quoted.
fn content(instruments: usize) -> SnapshotContent {
    let mut content = fixture::content();
    content.schema_version = SNAPSHOT_SCHEMA;
    let at_ms = content.as_of.datetime().timestamp_millis();
    content.references = SnapshotReferences::new((0..instruments).map(large_reference).collect());
    content.quotes = (0..instruments)
        .map(|n| largest_quote(n, at_ms, QuoteQuality::Live))
        .collect();
    content
}

/// A Live snapshot holds 4,096 instruments, whose identity hashes more than a
/// stored snapshot's 16 MiB, and its next is made within the same bounds, as
/// [`MarketSnapshot::bounded`] makes it. A snapshot made within the standard bounds holds
/// 256, and a Live one 4,096.
#[test]
fn a_live_snapshot_holds_4096_instruments_and_its_next_keeps_the_live_bounds() {
    let full = content(MAX_LIVE_SNAPSHOT_INSTRUMENTS);
    let hashed = bytes(&full.references) + full.quotes.iter().map(|q| bytes(&**q)).sum::<usize>();
    assert!(hashed > MAX_SNAPSHOT_BYTES, "{hashed}");
    assert!(matches!(
        MarketSnapshot::new(full.clone()),
        Err(TraitError::CapacityExceeded)
    ));
    let live = MarketSnapshot::bounded(full.clone(), SnapshotBounds::Live).unwrap();
    assert_eq!(live.bounds(), SnapshotBounds::Live);

    let mut next = full;
    assert_eq!(next.known_at, fixture::time("2026-10-02T15:00:00Z"));
    next.known_at = fixture::time("2026-10-02T15:00:00.001Z");
    let at_ms = next.as_of.datetime().timestamp_millis();
    let mut rebuilt = SnapshotQuote::clone(&largest_quote(7, at_ms, QuoteQuality::Live));
    rebuilt
        .image
        .as_mut()
        .unwrap()
        .fields
        .values_mut()
        .for_each(|field| {
            field.value.value = ExactDecimal::parse("99.5").unwrap().decimal();
        });
    next.quotes[7] = rebuilt.into();
    assert!(live.follows(&next));
    let derived = live.next(next.clone()).unwrap();
    assert_eq!(derived.bounds(), SnapshotBounds::Live);
    assert_eq!(
        derived,
        MarketSnapshot::bounded(next, SnapshotBounds::Live).unwrap()
    );
    assert_ne!(derived.id(), live.id());

    let small = |instruments| {
        let mut content = fixture::content();
        content.schema_version = SNAPSHOT_SCHEMA;
        for n in 1..instruments {
            fixture::add_bond(&mut content, &id('b', n), 0);
        }
        assert_eq!(content.quotes.len(), instruments);
        content
    };
    assert!(MarketSnapshot::new(small(256)).is_ok());
    assert!(matches!(
        MarketSnapshot::new(small(257)),
        Err(TraitError::CapacityExceeded)
    ));
    assert!(MarketSnapshot::bounded(small(257), SnapshotBounds::Live).is_ok());
    assert!(matches!(
        MarketSnapshot::bounded(small(4097), SnapshotBounds::Live),
        Err(TraitError::CapacityExceeded)
    ));
}
