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
            0 => content.references[0].calendar.recorded_at = fixture::time("2026-10-03T00:00:00Z"),
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
