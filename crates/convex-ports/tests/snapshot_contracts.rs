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
            0 => content.quotes[0].image = None,
            1 => {
                content.quotes[0].image.as_mut().unwrap().state =
                    convex_ports::quote_event::DataState::Gap;
                content.quotes[0].delivery_watermark.as_mut().unwrap().event =
                    convex_ports::quote_event::QuoteEventKind::Gap;
            }
            2 => content.fixings.push(SnapshotFixing {
                key: fixture::fixing_key(),
                fixing: None,
            }),
            3 => content.curves.clear(),
            4 => content.curves[0].quality = DataQuality::Demo,
            _ => {
                content.quotes[0].image.as_mut().unwrap().state =
                    convex_ports::quote_event::DataState::Unauthorized;
                content.quotes[0]
                    .delivery_watermark
                    .as_mut()
                    .unwrap()
                    .entitled = false;
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
    for field in quote.quotes[0].image.as_mut().unwrap().fields.values_mut() {
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
    let quote = content.quotes[0].clone();
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
            content.quotes.push(quote);
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
    observe(&mut others.quotes[1]);
    others.curves[1].version = fixture::version(2);
    assert_eq!(same(others.clone(), &["USD-FIXTURE"]), (true, true));
    assert_eq!(same(others, &["USD-FIXTURE", "USD-OTHER"]), (true, false));

    // Its own quote, its read curve and its reference.
    let mut quote = base();
    observe(&mut quote.quotes[0]);
    assert_eq!(same(quote, &[]), (true, false));
    let mut curve = base();
    curve.curves[0].version = fixture::version(2);
    assert_eq!(same(curve.clone(), &[]), (true, true));
    assert_eq!(same(curve, &["USD-FIXTURE"]), (true, false));
    let mut reference = base();
    reference.references.make_mut()[0].record.version = fixture::version(2);
    assert_eq!(same(reference, &[]), (true, false));

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
