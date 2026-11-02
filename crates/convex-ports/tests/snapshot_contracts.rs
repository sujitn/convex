#[path = "support/snapshot_fixtures.rs"]
mod fixture;

use convex_core::contracts::*;
use convex_ports::{fixing::select_fixing_at, snapshot::*};

#[test]
fn model_values_roundtrip_bit_exactly_for_content_identity() {
    let mut state = 17_u64;
    for _ in 0..4096 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let value = f64::from_bits(0x3fb0000000000000 | (state & 0x000fffffffffffff));
        let finite = FiniteF64::new(value).unwrap();
        let wire = serde_json::to_vec(&finite).unwrap();
        let replay: FiniteF64 = serde_json::from_slice(&wire).unwrap();
        assert_eq!(finite.get().to_bits(), replay.get().to_bits());
    }
}

#[test]
fn canonical_content_identity_and_wire_validation_preserve_pins() {
    let snapshot = fixture::snapshot();
    let bytes = snapshot.canonical_bytes().unwrap();
    assert_eq!(
        serde_json::from_slice::<MarketSnapshot>(&bytes).unwrap(),
        snapshot
    );
    let original = snapshot.id().clone();
    let mut content = snapshot.content().clone();
    content.curves[0].points[0].continuous_zero = FiniteF64::new(-0.01).unwrap();
    assert_ne!(MarketSnapshot::new(content).unwrap().id(), &original);
    for field in ["generation", "quality", "snapshot_id"] {
        let mut wire: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        match field {
            "generation" => wire["content"][field] = 2.into(),
            "quality" => wire[field]["complete"] = false.into(),
            _ => {
                wire[field] =
                    "sha256:0000000000000000000000000000000000000000000000000000000000000000".into()
            }
        }
        assert!(serde_json::from_value::<MarketSnapshot>(wire).is_err());
    }
}

#[test]
fn exact_fixing_cutoffs_exclude_later_publications_receipts_and_revisions() {
    let initial = fixture::fixing(1, "2026-10-02T09:00:00Z", "2026-10-02T09:00:01Z", "-0.001");
    let revision = fixture::fixing(2, "2026-10-02T10:00:00Z", "2026-10-03T09:00:00Z", "0.041");
    let history = vec![revision.clone(), initial.clone()];
    assert!(select_fixing_at(
        &history,
        &fixture::fixing_key(),
        fixture::time("2026-10-02T09:00:00.999999999Z")
    )
    .unwrap()
    .is_none());
    assert_eq!(
        select_fixing_at(&history, &fixture::fixing_key(), initial.recorded_at).unwrap(),
        Some(initial.clone())
    );
    assert_eq!(
        select_fixing_at(
            &history,
            &fixture::fixing_key(),
            fixture::time("2026-10-03T08:59:59.999999999Z")
        )
        .unwrap(),
        Some(initial)
    );
    assert_eq!(
        select_fixing_at(&history, &fixture::fixing_key(), revision.recorded_at).unwrap(),
        Some(revision.clone())
    );
    assert!(select_fixing_at(
        &[revision.clone(), revision],
        &fixture::fixing_key(),
        fixture::time("2026-10-04T00:00:00Z")
    )
    .is_err());
}

#[test]
fn missing_gapped_stale_unentitled_inputs_cannot_pass_execution_data_policy() {
    let good = fixture::snapshot();
    assert!(good
        .require_execution_quality_at(fixture::time("2026-10-02T15:00:10Z"))
        .is_ok());
    assert!(good
        .require_execution_quality_at(fixture::time("2026-10-02T15:00:10.001Z"))
        .is_err());
    assert!(good
        .require_execution_quality_at(fixture::time("2026-10-02T15:00:10.000000001Z"))
        .is_err());
    for mutate in 0..6 {
        let mut content = fixture::content();
        match mutate {
            0 => content.quotes[0].make_mut().image = None,
            1 => {
                let quote = content.quotes[0].make_mut();
                quote.image.as_mut().unwrap().state = convex_ports::quote_event::DataState::Gap;
                quote.delivery_watermark.as_mut().unwrap().event =
                    convex_ports::quote_event::QuoteEventKind::Gap;
            }
            2 => content.fixings.push(SnapshotFixing {
                key: fixture::fixing_key(),
                fixing: None,
            }),
            3 => content.curves.clear(),
            4 => content.curves[0].quality = DataQuality::Demo,
            _ => {
                let quote = content.quotes[0].make_mut();
                quote.image.as_mut().unwrap().state =
                    convex_ports::quote_event::DataState::Unauthorized;
                quote.delivery_watermark.as_mut().unwrap().entitled = false;
            }
        }
        let snapshot = MarketSnapshot::new(content).unwrap();
        assert!(!snapshot.quality().issues.is_empty());
        assert!(snapshot
            .require_execution_quality_at(snapshot.content().as_of)
            .is_err());
    }
}

#[test]
fn unknown_later_known_resources_and_invalid_curves_are_rejected_before_publication() {
    for case in 0..6 {
        let mut content = fixture::content();
        match case {
            0 => {
                content.references.make_mut()[0].calendar.recorded_at =
                    fixture::time("2026-10-03T00:00:00Z")
            }
            1 => content.configuration.version = 0,
            2 => content.curves[0].points.swap(0, 1),
            3 => content.curves[0].points[1].tenor_years = content.curves[0].points[0].tenor_years,
            4 => content.curves[0].recorded_at = fixture::time("2026-10-03T00:00:00Z"),
            _ => content.quotes[0]
                .make_mut()
                .requirement
                .fields
                .push(convex_ports::quote_event::QuoteField::MidPrice),
        }
        assert!(MarketSnapshot::new(content).is_err());
    }
}

/// D68: the identity equals SHA-256 over the content's plain serialization, one quote
/// field changes it, and a change to a section whose JSON was written drops that JSON.
#[test]
fn identity_is_the_hash_of_the_plain_content_bytes() {
    let mut content = fixture::content();
    fixture::add_bond(&mut content, "BOND-002", 0);
    let first = MarketSnapshot::new(content.clone()).unwrap();
    fixture::assert_plain_identity(&first);
    let id = first.id().clone();

    let mut quote = content.clone();
    for field in quote.quotes[0]
        .make_mut()
        .image
        .as_mut()
        .unwrap()
        .fields
        .values_mut()
    {
        field.source_time_ms -= 1;
    }
    let quote = MarketSnapshot::new(quote).unwrap();
    fixture::assert_plain_identity(&quote);
    assert_ne!(quote.id(), &id);
    assert_eq!(quote.content().references, first.content().references);

    // The section is no longer shared and its JSON is written: a change made in place.
    drop((first, quote));
    fixture::add_bond(&mut content, "BOND-003", 0);
    let grown = MarketSnapshot::new(content).unwrap();
    fixture::assert_plain_identity(&grown);
    assert_ne!(grown.id(), &id);
}

/// D68: a shared section's checks that read no cutoff run once, each snapshot still
/// checks it at its own cutoff, and a change drops the checks.
#[test]
fn a_checked_reference_section_is_still_checked_at_each_cutoff_and_after_a_change() {
    let mut content = fixture::content();
    content.references.make_mut()[0].calendar.recorded_at = fixture::time("2026-10-02T12:00:00Z");
    let checked = MarketSnapshot::new(content.clone()).unwrap();

    // Before the calendar revision was recorded.
    let mut earlier = content.clone();
    earlier.as_of = fixture::time("2026-10-02T11:00:00Z");
    earlier.known_at = earlier.as_of;
    assert!(matches!(
        MarketSnapshot::new(earlier),
        Err(convex_ports::TraitError::InvalidInput(
            "invalid resolved snapshot reference"
        ))
    ));

    // A duplicate added in place, once the section is no longer shared.
    drop(checked);
    let duplicate = content.references[0].clone();
    content.references.make_mut().push(duplicate);
    assert!(matches!(
        MarketSnapshot::new(content),
        Err(convex_ports::TraitError::InvalidInput(
            "duplicate snapshot reference"
        ))
    ));
}

#[test]
fn snapshot_configuration_keeps_exact_decimals_under_workspace_feature_unification() {
    let mut content = fixture::content();
    let ratio = ExactDecimal::parse("1.1234567890123456789012345678").unwrap();
    content.configuration.pricing_spec = convex_ports::storage::PricingSpec::RealYield {
        real_yield_pct: 1.25,
        inflation_curve: "USD-FIXTURE".into(),
        index_ratio: ratio.decimal(),
    };
    let snapshot = MarketSnapshot::new(content).unwrap();
    let bytes = snapshot.canonical_bytes().unwrap();
    assert!(String::from_utf8(bytes.clone())
        .unwrap()
        .contains("\"index_ratio\":\"1.1234567890123456789012345678\""));
    assert_eq!(
        serde_json::from_slice::<MarketSnapshot>(&bytes).unwrap(),
        snapshot
    );
}

#[test]
fn instrument_lookups_find_each_reference_and_every_source_in_canonical_order() {
    let mut content = fixture::content();
    let reference = content.references[0].clone();
    let quote = SnapshotQuote::clone(&content.quotes[0]);
    content.references.make_mut().clear();
    content.quotes.clear();
    // Supplied out of order; one instrument has two sources.
    for (id, sources) in [
        ("BOND-003", &["feed-a"][..]),
        ("BOND-001", &["feed-b", "feed-a"][..]),
        ("BOND-002", &["feed-a"][..]),
    ] {
        let mut reference = reference.clone();
        reference.record.instrument_id = convex_core::ids::InstrumentId::new(id);
        if let Some(bond) = &mut reference.record.bond {
            bond.instrument_id = reference.record.instrument_id.clone();
        }
        content.references.make_mut().push(reference);
        for source in sources {
            let mut quote = quote.clone();
            quote.requirement.key.instrument_id = convex_core::ids::InstrumentId::new(id);
            quote.requirement.key.source = (*source).into();
            let key = quote.requirement.key.clone();
            for event in quote
                .delivery_watermark
                .iter_mut()
                .chain(quote.image.iter_mut().map(|image| &mut image.watermark))
            {
                event.key = key.clone();
            }
            content.quotes.push(quote.into());
        }
    }
    let snapshot = MarketSnapshot::new(content).unwrap();
    for id in ["BOND-001", "BOND-002", "BOND-003"] {
        assert_eq!(
            snapshot
                .reference(id)
                .unwrap()
                .record
                .instrument_id
                .as_str(),
            id
        );
    }
    let sources: Vec<_> = snapshot
        .quotes_for("BOND-001")
        .iter()
        .map(|q| q.requirement.key.source.as_str())
        .collect();
    assert_eq!(sources, ["feed-a", "feed-b"]);
    assert_eq!(snapshot.quotes_for("BOND-002").len(), 1);
    assert_eq!(
        snapshot.quotes_for("BOND-003")[0]
            .requirement
            .key
            .instrument_id
            .as_str(),
        "BOND-003"
    );
    for absent in ["BOND-000", "BOND-0015", "BOND-004"] {
        assert!(snapshot.reference(absent).is_none());
        assert!(snapshot.quotes_for(absent).is_empty());
    }
}

/// D58: one instrument's quality reads its own quotes, a floating note's fixings and the
/// required curves its valuation reads.
#[test]
fn instrument_quality_reads_only_that_instruments_inputs() {
    let mut content = fixture::content();
    // Observed 20 s before the other: past its 10 s age and the 1 s skew policy.
    fixture::add_bond(&mut content, "BOND-002", 20_000);
    content.fixings.push(SnapshotFixing {
        key: fixture::fixing_key(),
        fixing: None,
    });
    let snapshot = MarketSnapshot::new(content).unwrap();
    let reasons: Vec<_> = snapshot.quality().issues.iter().map(|i| i.reason).collect();
    assert_eq!(snapshot.quality().quality, DataQuality::Invalid);
    for reason in [
        SnapshotIssueKind::Stale,
        SnapshotIssueKind::SourceSkew,
        SnapshotIssueKind::MissingFixing,
    ] {
        assert!(reasons.contains(&reason), "{reasons:?}");
    }
    // A fixed-coupon bond reads no fixing; its own fresh quote and curve are valid.
    let fresh = snapshot.inputs("BOND-001", &["USD-FIXTURE"]);
    assert!(fresh.fixings().is_empty());
    assert_eq!(fresh.quotes().len(), 1);
    assert_eq!(fresh.curves(), [&snapshot.content().required_curves[0]]);
    let quality = snapshot.quality_of(&fresh);
    assert!(quality.complete && quality.issues.is_empty());
    assert_eq!(quality.quality, DataQuality::Valid);
    let stale = snapshot.quality_of(&snapshot.inputs("BOND-002", &[]));
    assert_eq!(stale.quality, DataQuality::Stale);
    assert_eq!(
        stale.issues,
        [SnapshotIssue {
            dependency: "quote:BOND-002:feed-a:MidPrice".into(),
            reason: SnapshotIssueKind::Stale,
        }]
    );
    assert!(snapshot.inputs("BOND-404", &[]).quotes().is_empty());

    // A curve counts only for a valuation that reads it. A one-instrument set that reads
    // the stale curve gives the snapshot-wide answer; a curve no snapshot requires is
    // not judged.
    let mut content = fixture::content();
    let mut other = content.curves[0].clone();
    other.curve_id = CurveId::new("USD-OTHER").unwrap();
    content.required_curves.push(other.curve_id.clone());
    content.curves.push(other);
    let old = fixture::time("2026-10-02T14:59:00Z");
    content.curves[0].source_time = old;
    content.curves[0].recorded_at = old;
    let snapshot = MarketSnapshot::new(content).unwrap();
    assert_eq!(snapshot.quality().quality, DataQuality::Stale);
    let reads = snapshot.inputs("BOND-001", &["USD-FIXTURE", "USD-ABSENT"]);
    assert_eq!(reads.curves(), [&snapshot.content().required_curves[0]]);
    assert_eq!(snapshot.quality_of(&reads), *snapshot.quality());
    for unread in [&["USD-OTHER"][..], &[]] {
        let quality = snapshot.quality_of(&snapshot.inputs("BOND-001", unread));
        assert!(quality.issues.is_empty(), "{unread:?}");
        assert_eq!(quality.quality, DataQuality::Valid);
    }
    assert_eq!(snapshot.curve_quality("USD-FIXTURE"), DataQuality::Stale);
    assert_eq!(snapshot.curve_quality("USD-OTHER"), DataQuality::Valid);
    assert_eq!(snapshot.curve_quality("USD-ABSENT"), DataQuality::Invalid);
}

/// D55: a later snapshot keeps one instrument's result when its frame, reference, quotes,
/// fixings and read curves are those of the snapshot before.
#[test]
fn an_instrument_reads_the_same_inputs_until_its_own_change() {
    let base = || {
        let mut content = fixture::content();
        fixture::add_bond(&mut content, "BOND-002", 0);
        let mut other = content.curves[0].clone();
        other.curve_id = CurveId::new("USD-OTHER").unwrap();
        content.required_curves.push(other.curve_id.clone());
        content.curves.push(other);
        content
    };
    let previous = MarketSnapshot::new(base()).unwrap();
    let id = previous.content().references[0]
        .record
        .instrument_id
        .clone();
    let id = id.as_str();
    // A field observed one millisecond earlier: another observation of the same quote.
    let observe = |quote: &mut SnapshotQuote| {
        for field in quote.image.as_mut().unwrap().fields.values_mut() {
            field.source_time_ms -= 1;
        }
    };
    let same = |content: SnapshotContent, curves: &[&str]| {
        let snapshot = MarketSnapshot::new(content).unwrap();
        (
            snapshot.same_frame(&previous),
            snapshot.same_inputs(&previous, id, curves),
        )
    };
    // Later the same day, with another generation: the same frame and inputs.
    let mut later = base();
    later.as_of = fixture::time("2026-10-02T23:59:00Z");
    later.known_at = later.as_of;
    later.generation = fixture::version(2);
    assert_eq!(same(later, &["USD-FIXTURE"]), (true, true));

    // Another instrument's quote and a curve this valuation does not read.
    let mut others = base();
    observe(others.quotes[1].make_mut());
    others.curves[1].version = fixture::version(2);
    assert_eq!(same(others.clone(), &["USD-FIXTURE"]), (true, true));
    assert_eq!(same(others, &["USD-FIXTURE", "USD-OTHER"]), (true, false));

    // Its own quote, its read curve and its reference.
    let mut quote = base();
    observe(quote.quotes[0].make_mut());
    assert_eq!(same(quote, &[]), (true, false));
    let mut curve = base();
    curve.curves[0].version = fixture::version(2);
    assert_eq!(same(curve.clone(), &[]), (true, true));
    assert_eq!(same(curve, &["USD-FIXTURE"]), (true, false));
    let mut reference = base();
    reference.references.make_mut()[0].record.version = fixture::version(2);
    assert_eq!(same(reference, &[]), (true, false));

    // A shared reference section is the same section; a copy made to change it is not.
    let mut shared = base();
    shared.references = previous.content().references.clone();
    assert!(shared.references.ptr_eq(&previous.content().references));
    assert_eq!(same(shared.clone(), &["USD-FIXTURE"]), (true, true));
    shared.references.make_mut()[0].record.version = fixture::version(2);
    assert!(!shared.references.ptr_eq(&previous.content().references));
    assert_eq!(same(shared, &[]), (true, false));

    // The frame: the next day, the skew policy and the configuration.
    let mut tomorrow = base();
    tomorrow.as_of = fixture::time("2026-10-03T00:00:00Z");
    tomorrow.known_at = tomorrow.as_of;
    assert_eq!(same(tomorrow, &[]), (false, true));
    let mut skew = base();
    skew.max_source_skew_ms = 2_000;
    assert!(!same(skew, &[]).0);
    let mut configuration = base();
    configuration.configuration.version += 1;
    assert!(!same(configuration, &[]).0);
}

/// P13b-ii (D79): schema 4's id is the sectioned layout, written independently; a change
/// to the frame, a quote or the references changes it, and so does the schema. A section
/// changed in place after its digest was computed is hashed again.
#[test]
fn schema_four_identity_is_the_sectioned_layout() {
    let mut content = fixture::content();
    fixture::add_bond(&mut content, "BOND-002", 0);
    let whole = MarketSnapshot::new(content.clone()).unwrap();
    content.schema_version = SNAPSHOT_SCHEMA;
    let first = MarketSnapshot::new(content.clone()).unwrap();
    fixture::assert_sectioned_identity(&first);
    assert_ne!(first.id(), whole.id());
    let id = first.id().clone();

    let mut frame = content.clone();
    frame.max_source_skew_ms += 1;
    let frame = MarketSnapshot::new(frame).unwrap();
    fixture::assert_sectioned_identity(&frame);
    assert_ne!(frame.id(), &id);

    let mut quote = content.clone();
    for field in quote.quotes[1]
        .make_mut()
        .image
        .as_mut()
        .unwrap()
        .fields
        .values_mut()
    {
        field.source_time_ms -= 1;
    }
    let quote = MarketSnapshot::new(quote).unwrap();
    fixture::assert_sectioned_identity(&quote);
    assert_ne!(quote.id(), &id);
    assert!(quote
        .content()
        .references
        .ptr_eq(&first.content().references));

    // Only `content` holds the section now, so the change is made in place.
    drop((whole, first, frame, quote));
    fixture::add_bond(&mut content, "BOND-003", 0);
    let grown = MarketSnapshot::new(content).unwrap();
    fixture::assert_sectioned_identity(&grown);
    assert_ne!(grown.id(), &id);
}

/// Schema 4 round-trips through its wire form, which verifies it, and a changed byte is
/// refused. Schemas outside 1–4 are refused.
#[test]
fn schema_four_round_trips_and_other_schemas_are_refused() {
    let snapshot = fixture::sectioned_snapshot();
    let wire = snapshot.canonical_bytes().unwrap();
    assert_eq!(
        serde_json::from_slice::<MarketSnapshot>(&wire).unwrap(),
        snapshot
    );
    let tampered = String::from_utf8(wire).unwrap().replace("99.25", "99.26");
    assert!(serde_json::from_str::<MarketSnapshot>(&tampered).is_err());
    for schema in [0, 5, u32::MAX] {
        let mut content = fixture::content();
        content.schema_version = schema;
        assert!(matches!(
            MarketSnapshot::new(content),
            Err(convex_ports::TraitError::InvalidInput(_))
        ));
    }
}

/// Restore's re-derivation: `with_schema` equals building the content under that schema,
/// and refuses a schema whose rules the content breaks.
#[test]
fn with_schema_builds_the_same_content_under_another_schema() {
    let one = fixture::snapshot();
    let four = fixture::sectioned_snapshot();
    assert_eq!(four.with_schema(1).unwrap(), one);
    assert_eq!(one.with_schema(SNAPSHOT_SCHEMA).unwrap(), four);
    // Schema 2 requires recipes, which the fixture does not hold.
    assert!(four.with_schema(2).is_err());
}

/// P13b-ii: snapshots of one content share its quote entries, and so the entries' digests
/// and checks. A changed entry is checked and hashed again, even when it is changed in
/// place after both were computed: a change that breaks a check is refused, and one that
/// does not hashes to the independently written id. Fields supplied out of order are
/// sorted in their own entry; the other entries stay shared. Equal snapshots that share
/// an entry compare it by pointer, and the same quote in another entry is equal.
#[test]
fn quote_entries_are_shared_and_a_changed_entry_is_checked_and_hashed_again() {
    let mut content = fixture::content();
    fixture::add_bond(&mut content, "BOND-002", 0);
    content.schema_version = SNAPSHOT_SCHEMA;
    let first = MarketSnapshot::new(content.clone()).unwrap();
    let second = MarketSnapshot::new(content.clone()).unwrap();
    assert_eq!(second.id(), first.id());
    let shared = |a: &MarketSnapshot, b: &MarketSnapshot| -> Vec<bool> {
        let quotes = a.content().quotes.iter().zip(&b.content().quotes);
        quotes.map(|(a, b)| a.ptr_eq(b)).collect()
    };
    assert_eq!(shared(&second, &first), [true, true]);
    let id = first.id().clone();

    // Only `content` holds the entries now, so each change is made in place.
    drop((first, second));
    let max_age_ms = content.quotes[1].requirement.max_age_ms;
    content.quotes[1].make_mut().requirement.max_age_ms = 0;
    assert!(MarketSnapshot::new(content.clone()).is_err());
    content.quotes[1].make_mut().requirement.max_age_ms = max_age_ms;
    assert_eq!(MarketSnapshot::new(content.clone()).unwrap().id(), &id);
    let image = content.quotes[1].make_mut().image.as_mut().unwrap();
    for field in image.fields.values_mut() {
        field.source_time_ms -= 1;
    }
    let changed = MarketSnapshot::new(content.clone()).unwrap();
    fixture::assert_sectioned_identity(&changed);
    assert_ne!(changed.id(), &id);

    let mut unsorted = changed.content().clone();
    let fields = &mut unsorted.quotes[0].make_mut().requirement.fields;
    fields.push(convex_ports::quote_event::QuoteField::BidPrice);
    fields.sort();
    fields.reverse();
    let sorted = MarketSnapshot::new(unsorted).unwrap();
    assert!(sorted.content().quotes[0].requirement.fields.is_sorted());
    assert_eq!(shared(&sorted, &changed), [false, true]);
    fixture::assert_sectioned_identity(&sorted);

    let copied = MarketSnapshot::new(SnapshotContent {
        quotes: changed
            .content()
            .quotes
            .iter()
            .map(|quote| SnapshotQuote::clone(quote).into())
            .collect(),
        ..changed.content().clone()
    })
    .unwrap();
    assert_eq!(shared(&copied, &changed), [false, false]);
    assert_eq!(copied, changed);
}

/// The fixture's day, `ms` milliseconds and `nanos` nanoseconds after its 15:00 as-of.
fn after_as_of(ms: i64, nanos: i64) -> UtcTimestamp {
    let total = (15 * 3_600_000 + ms) * 1_000_000 + nanos;
    let (seconds, nanos) = (total / 1_000_000_000, total % 1_000_000_000);
    UtcTimestamp::parse(&format!(
        "2026-10-02T{:02}:{:02}:{:02}.{nanos:09}Z",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    ))
    .unwrap()
}

/// The snapshot of `content` at `time`, sharing its quote entries and references.
fn snapshot_at(content: &SnapshotContent, time: UtcTimestamp) -> MarketSnapshot {
    let mut content = content.clone();
    content.as_of = time;
    content.known_at = time;
    MarketSnapshot::new(content).unwrap()
}

/// Set every observation time of one quote's fields.
fn observe_fields(quote: &mut SnapshotQuote, source_ms: i64, receive_ms: i64) {
    for field in quote.image.as_mut().unwrap().fields.values_mut() {
        field.source_time_ms = source_ms;
        field.receive_time_ms = receive_ms;
    }
}

/// P13b-iii (D81): the same inputs keep their quality at every snapshot time in the
/// window `judge` returns. The window ends at the first millisecond in which one of
/// quality's time comparisons can turn: a quote field's source age, its receive age, or
/// a curve's age, whichever comes first. A time before the window's start is outside it,
/// and inputs with nothing time-bound have no end.
#[test]
fn a_quality_window_holds_until_one_of_its_time_comparisons_can_turn() {
    let as_of_ms = fixture::time("2026-10-02T15:00:00Z")
        .datetime()
        .timestamp_millis();
    let mut content = fixture::content();
    fixture::add_bond(&mut content, "BOND-002", 4_000);
    fixture::add_bond(&mut content, "BOND-003", 0);
    // BOND-002's source is older than its receipt, BOND-003's receipt older than its
    // source; each quote's age limit is 10 s. BOND-001 reads a curve 8 s old.
    observe_fields(
        content.quotes[1].make_mut(),
        as_of_ms - 4_000,
        as_of_ms - 1_000,
    );
    observe_fields(
        content.quotes[2].make_mut(),
        as_of_ms - 1_000,
        as_of_ms - 3_000,
    );
    let curve_time = fixture::time("2026-10-02T14:59:52Z");
    content.curves[0].source_time = curve_time;
    content.curves[0].recorded_at = curve_time;
    let snapshot = snapshot_at(&content, after_as_of(0, 0));
    for (instrument, curves, until) in [
        ("BOND-002", &[][..], as_of_ms + 6_000),
        ("BOND-003", &[][..], as_of_ms + 7_000),
        ("BOND-001", &["USD-FIXTURE"][..], as_of_ms + 2_000),
    ] {
        let (quality, window) = snapshot.judge(&snapshot.inputs(instrument, curves));
        assert_eq!(
            quality,
            snapshot.quality_of(&snapshot.inputs(instrument, curves))
        );
        assert_eq!(quality.quality, DataQuality::Valid, "{instrument}");
        assert_eq!(window.judged_at(), snapshot.content().as_of);
        assert_eq!(window.until_ms(), Some(until), "{instrument}");
        let quality_at = |time| {
            let later = snapshot_at(&content, time);
            let judged = later.quality_of(&later.inputs(instrument, curves));
            (later.within(&window), judged.quality)
        };
        let deadline = until - as_of_ms;
        // To the window's last nanosecond the quality holds.
        assert_eq!(
            quality_at(after_as_of(deadline - 1, 999_999)),
            (true, DataQuality::Valid)
        );
        // In the deadline's own millisecond it can turn, so that millisecond is outside
        // the window: still valid at its start, stale one nanosecond later.
        assert_eq!(
            quality_at(after_as_of(deadline, 0)),
            (false, DataQuality::Valid)
        );
        assert_eq!(
            quality_at(after_as_of(deadline, 1)),
            (false, DataQuality::Stale)
        );
    }

    // Inputs with nothing time-bound hold at every later time, but not before.
    let later = snapshot_at(&content, after_as_of(1_000, 0));
    let (quality, window) = later.judge(&later.inputs("BOND-404", &[]));
    assert_eq!(quality.quality, DataQuality::Valid);
    assert_eq!(window.until_ms(), None);
    assert!(snapshot_at(&content, after_as_of(28_800_000, 0)).within(&window));
    assert!(!snapshot.within(&window));
}

/// P13b-iii (D81): `changed_quotes` names the instruments whose quote entries a later
/// snapshot does not share, once each, while everything else a valuation reads is the
/// same. When anything else differs, the frame included, it names none, so every
/// instrument is compared.
#[test]
fn changed_quotes_names_the_unshared_entries_only_while_nothing_else_differs() {
    let mut content = fixture::content();
    fixture::add_bond(&mut content, "BOND-002", 0);
    fixture::add_bond(&mut content, "BOND-003", 0);
    let previous = MarketSnapshot::new(content).unwrap();
    let next = |change: &dyn Fn(&mut SnapshotContent)| {
        let mut content = previous.content().clone();
        content.generation = fixture::version(2);
        change(&mut content);
        MarketSnapshot::new(content)
            .unwrap()
            .changed_quotes(&previous)
            .map(|names| names.into_iter().map(str::to_owned).collect::<Vec<_>>())
    };
    assert_eq!(next(&|_| {}), Some(vec![]));
    // An equal quote in a new entry is named; so is each instrument whose entry changed.
    assert_eq!(
        next(&|content| content.quotes[1] = SnapshotQuote::clone(&content.quotes[1]).into()),
        Some(vec!["BOND-002".to_owned()])
    );
    assert_eq!(
        next(&|content| {
            for index in [2, 0] {
                let image = content.quotes[index].image.as_ref().unwrap();
                let observed = image.fields.values().next().unwrap().source_time_ms;
                observe_fields(content.quotes[index].make_mut(), observed - 1, observed);
            }
        }),
        Some(vec!["BOND-001".to_owned(), "BOND-003".to_owned()])
    );

    // Anything else: the frame (the next day, the skew policy, the configuration, the
    // schema), the reference section copied, a fixing, a curve, a required curve, or
    // another quote key.
    let changes: [&dyn Fn(&mut SnapshotContent); 9] = [
        &|content| {
            content.as_of = fixture::time("2026-10-03T00:00:00Z");
            content.known_at = content.as_of;
        },
        &|content| content.max_source_skew_ms = 2_000,
        &|content| content.configuration.version += 1,
        &|content| content.schema_version = SNAPSHOT_SCHEMA,
        &|content| content.references = SnapshotReferences::new(content.references.to_vec()),
        &|content| {
            content.fixings.push(SnapshotFixing {
                key: fixture::fixing_key(),
                fixing: None,
            })
        },
        &|content| content.curves[0].version = fixture::version(2),
        &|content| {
            let mut other = content.curves[0].clone();
            other.curve_id = CurveId::new("USD-OTHER").unwrap();
            content.required_curves.push(other.curve_id.clone());
            content.curves.push(other);
        },
        &|content| {
            let quote = content.quotes[2].make_mut();
            quote.requirement.key.source = "feed-b".into();
            for event in quote
                .delivery_watermark
                .iter_mut()
                .chain(quote.image.iter_mut().map(|image| &mut image.watermark))
            {
                event.key.source = "feed-b".into();
            }
        },
    ];
    for (index, change) in changes.into_iter().enumerate() {
        assert_eq!(next(change), None, "change {index}");
    }
}
