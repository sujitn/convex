//! P17c-ii (D108): a snapshot derived from the last one is the snapshot `new` builds.
#[path = "support/configuration_fixtures.rs"]
mod configuration;

use configuration::snapshot as fixture;
use convex_core::contracts::*;
use convex_ports::{
    configuration::{ActivationRecord, ConfigurationCapture, PolicyOverride},
    quote_event::*,
    reference_master::MasterDefinition,
    snapshot::*,
    TraitError,
};
use std::collections::BTreeMap;

const IDS: [&str; 6] = [
    "BOND-001", "BOND-002", "BOND-003", "BOND-004", "BOND-005", "BOND-006",
];

/// A deterministic script.
struct Script(u64);
impl Script {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % bound
    }
}

fn start_ms() -> i64 {
    fixture::time("2026-10-02T15:00:00Z")
        .datetime()
        .timestamp_millis()
}

/// The UTC instant `ms` milliseconds and `nanos` nanoseconds after the Unix epoch, on the
/// fixture's day.
fn at(ms: i64, nanos: u32) -> UtcTimestamp {
    let day_ms = start_ms() - 15 * 3_600_000;
    let total = (ms - day_ms) * 1_000_000 + i64::from(nanos);
    assert!(
        (0..86_400 * 1_000_000_000).contains(&total),
        "outside the day"
    );
    let (seconds, nanos) = (total / 1_000_000_000, total % 1_000_000_000);
    UtcTimestamp::parse(&format!(
        "2026-10-02T{:02}:{:02}:{:02}.{nanos:09}Z",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    ))
    .unwrap()
}

/// The entry of `key` after its `sequence`th image, observed and received at the times.
fn entry(
    key: &QuoteKey,
    max_age_ms: i64,
    sequence: u64,
    source_ms: i64,
    receive_ms: i64,
) -> SharedQuote {
    let mut event = fixture::event(sequence, source_ms, receive_ms, "99.25");
    event.key = key.clone();
    let observation = FieldObservation {
        value: QuoteValue {
            value: ExactDecimal::parse("99.25").unwrap().decimal(),
            unit: QuoteUnit::PricePer100,
        },
        source_time_ms: source_ms,
        receive_time_ms: receive_ms,
        sequence,
    };
    SnapshotQuote {
        requirement: QuoteRequirement {
            key: key.clone(),
            fields: vec![QuoteField::MidPrice],
            max_age_ms,
        },
        delivery_watermark: Some(event.clone()),
        image: Some(QuoteSnapshot {
            watermark: event,
            fields: BTreeMap::from([(QuoteField::MidPrice, observation)]),
            state: DataState::Live,
        }),
    }
    .into()
}

/// Six instruments, two with a second source. One instrument's terms end 200 s after the
/// start and another's calendar 250 s after it; one calendar is recorded 1 s after it.
fn content() -> SnapshotContent {
    let mut content = fixture::content();
    content.schema_version = SNAPSHOT_SCHEMA;
    content.max_source_skew_ms = 5_000;
    let reference = content.references[0].clone();
    let mut references = Vec::new();
    let mut quotes = Vec::new();
    for (i, id) in IDS.iter().enumerate() {
        let mut reference = reference.clone();
        let instrument = convex_core::ids::InstrumentId::new(*id);
        reference.record.instrument_id = instrument.clone();
        let bond = reference.record.bond.as_mut().unwrap();
        bond.instrument_id = instrument.clone();
        if i == 2 {
            let terms = bond.contractual_terms.as_mut().unwrap();
            terms.effective.until = Some(at(start_ms() + 200_000, 0));
        }
        // A changed calendar is another revision of it, which the terms pin.
        if i >= 4 {
            let revision = fixture::version(i as u64 - 2);
            let terms = bond.contractual_terms.as_mut().unwrap();
            terms.calendar.version = revision;
            reference.calendar.version = revision;
        }
        if let (4, MasterDefinition::Calendar(calendar)) = (i, &mut reference.calendar.definition) {
            calendar.effective.until = Some(at(start_ms() + 250_000, 0));
        }
        if i == 5 {
            reference.calendar.recorded_at = at(start_ms() + 1_000, 0);
        }
        references.push(reference);
        let sources: &[&str] = if i % 3 == 0 {
            &["feed-a", "feed-b"]
        } else {
            &["feed-a"]
        };
        for source in sources {
            let key = QuoteKey {
                instrument_id: instrument.clone(),
                source: (*source).into(),
            };
            let max_age_ms = if i == 1 { 30_000 } else { 10_000 };
            quotes.push(entry(
                &key,
                max_age_ms,
                1,
                start_ms() - 500,
                start_ms() - 500,
            ));
        }
    }
    content.references = SnapshotReferences::new(references);
    content.quotes = quotes;
    content
}

/// A capture of the fixture's activation that applies one override to BOND-002, from
/// `approved_ms` to `expires_ms` after the start.
fn capture(approved_ms: i64, expires_ms: i64) -> ConfigurationCapture {
    ConfigurationCapture {
        activation: ActivationRecord {
            revision: fixture::version(1),
            command: configuration::activate(1, 0, "activate-1"),
            previous_version: None,
            configuration_digest: "a".repeat(64),
        },
        applied_overrides: vec![PolicyOverride {
            id: SourceId::new("override-1").unwrap(),
            scope: QuoteKey {
                instrument_id: convex_core::ids::InstrumentId::new("BOND-002"),
                source: "feed-a".into(),
            },
            max_age_ms: 30_000,
            actor: "fixture-operator".into(),
            reason: "Fictional override".into(),
            created_at: at(start_ms() + approved_ms, 0),
            expires_at: at(start_ms() + expires_ms, 0),
            approved_by: "fixture-approver".into(),
            approved_at: at(start_ms() + approved_ms, 0),
        }],
    }
}

fn outcome(result: &Result<MarketSnapshot, TraitError>) -> String {
    match result {
        Ok(snapshot) => snapshot.id().as_str().to_owned(),
        Err(error) => format!("{error:?}"),
    }
}

/// `next` returns what `new` returns, value or error, over long chains of ticks: both
/// cutoffs move on their own or hold, a few entries are rebuilt (some received after the
/// cutoff, some malformed, some under another requirement), a section or capture is
/// replaced, an override starts and one expires, a reference's terms and another's
/// calendar end, and the cutoffs go back. Each derived snapshot also survives a JSON round
/// trip, whose deserialization builds every entry and section afresh and re-derives the
/// identity and quality.
#[test]
fn next_returns_what_new_returns_at_every_tick() {
    let base = content();
    let mut list = base.quotes.clone();
    let mut references = base.references.clone();
    let mut script = Script(11);
    let mut sequence = 1;
    let mut last: Option<MarketSnapshot> = None;
    let (mut as_of_ms, mut known_at_ms) = (start_ms(), start_ms());
    let mut derived = 0;
    let mut refused: BTreeMap<String, usize> = BTreeMap::new();
    let mut walked = 0;
    for step in 0..3_000 {
        // Each segment restarts the clock and the quotes under one capture: none, an
        // override that expires 120 s in, or one that starts 60 s in and expires at 180 s.
        let segment = step / 200;
        if step % 200 == 0 {
            (as_of_ms, known_at_ms) = (start_ms() + 1_000, start_ms() + 1_000);
            list = base.quotes.clone();
        }
        let capture = match segment % 3 {
            0 => None,
            1 => Some(capture(0, 120_000)),
            _ => Some(capture(60_000, 180_000)),
        };
        for cutoff in [&mut as_of_ms, &mut known_at_ms] {
            *cutoff += match script.next(20) {
                0..=5 => 0,
                6 => -(script.next(500) as i64) - 1,
                _ => script.next(4_000) as i64,
            };
        }
        let as_of = at(as_of_ms, script.next(1_000_000) as u32);
        let known_at = at(known_at_ms, script.next(1_000_000) as u32);
        // A few entries are rebuilt, observed and received by the cutoffs.
        for _ in 0..script.next(3) {
            let position = script.next(list.len() as u64) as usize;
            let prior = list[position].requirement.clone();
            sequence += 1;
            let (source_ms, receive_ms) = (
                as_of_ms - script.next(3_000) as i64,
                known_at_ms - script.next(3_000) as i64,
            );
            list[position] = entry(
                &prior.key,
                prior.max_age_ms,
                sequence,
                source_ms,
                receive_ms,
            );
        }
        if script.next(50) == 0 {
            references = SnapshotReferences::new(references.to_vec());
        }
        // This tick alone may carry one entry that a snapshot refuses or that has another
        // requirement, or its entries out of order.
        let mut quotes = list.clone();
        let position = script.next(quotes.len() as u64) as usize;
        let prior = quotes[position].requirement.clone();
        let odd = |source_ms, receive_ms| {
            entry(
                &prior.key,
                prior.max_age_ms,
                sequence + 1,
                source_ms,
                receive_ms,
            )
        };
        match script.next(40) {
            0 => quotes[position] = odd(as_of_ms - 100, known_at_ms + 1),
            1 => quotes[position] = odd(as_of_ms + 1, known_at_ms - 100),
            2 => {
                // Malformed: a field observed before any sequence.
                let mut quote = odd(as_of_ms - 100, known_at_ms - 100);
                let image = quote.make_mut().image.as_mut().unwrap();
                image
                    .fields
                    .get_mut(&QuoteField::MidPrice)
                    .unwrap()
                    .sequence = 0;
                quotes[position] = quote;
            }
            3 => quotes[position].make_mut().requirement.max_age_ms += 1,
            4 => {
                let last = quotes.len() - 1;
                quotes.swap(0, last);
            }
            _ => {}
        }
        let content = SnapshotContent {
            as_of,
            known_at,
            generation: fixture::version(step + 1),
            configuration_capture: capture,
            references: references.clone(),
            quotes,
            ..base.clone()
        };
        let built = MarketSnapshot::new(content.clone());
        let Some(prior) = &last else {
            last = built.ok();
            walked += 1;
            continue;
        };
        let follows = prior.follows(&content);
        let next = prior.next(content);
        assert_eq!(outcome(&next), outcome(&built), "step {step}");
        if let (Ok(next), Ok(built)) = (&next, &built) {
            assert_eq!(next, built, "step {step}");
            let json = serde_json::to_vec(next).unwrap();
            let read: MarketSnapshot = serde_json::from_slice(&json).unwrap();
            assert_eq!(&read, next, "step {step}");
        }
        match (&next, follows) {
            (Ok(_), true) => derived += 1,
            (Err(error), true) => *refused.entry(format!("{error:?}")).or_default() += 1,
            _ => walked += 1,
        }
        if let Ok(next) = next {
            last = Some(next);
        }
    }
    // A refused tick leaves the last snapshot in place, so the ticks after it until one
    // succeeds compare with that one, and many of them are built whole.
    assert!(derived > 800, "{derived} derived");
    assert!(walked > 100, "{walked} built whole");
    for reason in [
        "expired or mismatched captured override",
        "invalid resolved snapshot reference",
        "quote outside selected source/time scope",
        "invalid snapshot field observation",
    ] {
        let count: usize = refused
            .iter()
            .filter(|(error, _)| error.contains(reason))
            .map(|(_, count)| count)
            .sum();
        assert!(
            count > 5,
            "{count} derived ticks refused for {reason}: {refused:?}"
        );
    }
}

/// A checked section's cutoff checks, read from its bounds in O(1), refuse exactly the
/// lookups at which some record or resource is not yet recorded or not effective, as a
/// new section's full checks do: at each side of every boundary, to the nanosecond.
#[test]
fn a_checked_section_refuses_the_lookups_its_records_refuse() {
    let mut base = content();
    (base.as_of, base.known_at) = (at(start_ms() + 2_000, 0), at(start_ms() + 2_000, 0));
    let checked = MarketSnapshot::new(base.clone()).unwrap();
    let start = start_ms();
    let mut boundaries = vec![start, start + 1_000, start + 200_000, start + 250_000];
    boundaries.extend([start + 100_000, start + 300_000]);
    for boundary in boundaries {
        // A nanosecond before the boundary, at it, and a nanosecond after.
        for (ms, nanos) in [(boundary - 1, 999_999u32), (boundary, 0), (boundary, 1)] {
            for (as_of, known_at) in [
                (at(ms, nanos), at(ms, nanos)),
                (at(start + 2_000, 0), at(ms, nanos)),
                (at(ms, nanos), at(start + 2_000, 0)),
            ] {
                let mut content = base.clone();
                content.as_of = as_of;
                content.known_at = known_at;
                // The checked section, and the same records in a section not yet checked.
                let shared = MarketSnapshot::new(content.clone()).map(|s| s.id().clone());
                content.references = SnapshotReferences::new(base.references.to_vec());
                let fresh = MarketSnapshot::new(content).map(|s| s.id().clone());
                assert_eq!(
                    format!("{shared:?}"),
                    format!("{fresh:?}"),
                    "as of {as_of:?}, known at {known_at:?}"
                );
            }
        }
    }
    drop(checked);
}
