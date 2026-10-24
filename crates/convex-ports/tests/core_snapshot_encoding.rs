//! Canonical JSON byte contracts, independent serde output and original guards.
//! Opaque port payloads; native financial/issuer/checkpoint admission stays in the engine.
use convex_core::contracts::*;
use convex_ports::{
    core_workflow::*, reference_master::*, snapshot::MAX_SNAPSHOT_BYTES, TraitError,
};
use std::collections::BTreeMap;

fn snapshot() -> CoreSnapshot {
    let at = UtcTimestamp::parse("2026-10-18T12:00:00Z").unwrap();
    let id = ReferenceDataId::new("json.encode.integrity").unwrap();
    let atoms: serde_json::Value = serde_json::from_str(
        r#"[null,true,false,0,-0.0,1.0000000000000002,9007199254740993,18446744073709551615,-9223372036854775808,5e-324,1.7976931348623157e308,"Ω東京😀\n\t\\\"",{"a":[1,2,3]}]"#,
    )
    .unwrap();
    let definition = CoreDefinition {
        id: id.clone(),
        reference_inputs: serde_json::json!({"marker":"stable","precision":atoms}),
        calculations: BTreeMap::from([(
            "probe".into(),
            CoreCalculation {
                request: serde_json::json!({"rows":vec![atoms;512]}),
                curve_sets: vec![],
            },
        )]),
        effective: EffectiveInterval {
            from: at,
            until: None,
        },
        provenance: ReferenceProvenance {
            source: SourceId::new("json.encode.test").unwrap(),
            evidence_reference: "Opaque precision/UTF-8 admission control".into(),
        },
    };
    CoreSnapshot::new(CoreSnapshotContent {
        schema_version: 1,
        plan: CoreSnapshotPlan {
            definition: ReferencePin {
                id,
                version: Version::new(1).unwrap(),
            },
            as_of: at,
            known_at: at,
            curve_sets: BTreeMap::new(),
            fixings: vec![],
        },
        definition: MasterRecord {
            schema_version: SchemaVersion::V2,
            version: Version::new(1).unwrap(),
            recorded_at: at,
            definition: MasterDefinition::Calculation(Box::new(definition)),
        },
        curves: BTreeMap::new(),
        fixings: vec![],
        issuer_scope: None,
        checkpoint: None,
    })
    .unwrap()
}

#[test]
fn canonical_envelopes_match_serde_for_precision_escaping_and_optional_fields() {
    for index in 0..32 {
        let mut content = snapshot().content;
        if let MasterDefinition::Calculation(ref mut definition) = content.definition.definition {
            definition.reference_inputs["variant"] = serde_json::json!({
                "number":index,"text":"\n\t\\\"Ω東京😀","nested":[true,null,-0.0,1.0000000000000002]});
        }
        // Optional record/totals are deliberately opaque wire controls. They
        // do not claim a valid issuer directory or a financially admitted carry.
        if index % 2 == 0 {
            content.issuer_scope = Some(content.definition.clone());
        }
        if index % 3 == 0 {
            content.checkpoint = Some(CoreCheckpoint {
                source: CoreCheckpointSource {
                    snapshot_id: SnapshotId::new(format!("sha256:{}", "0".repeat(64))).unwrap(),
                    calculation: "pnl".into(),
                    build_id: ModelId::new("encoding.test").unwrap(),
                },
                result_id: "a".repeat(64),
                output_digest: "b".repeat(64),
                opening_digest: "c".repeat(64),
                completed_windows: 1,
                completed_causes: 16,
                retired_fill_ids: vec!["filled.1".into()],
                local_totals: serde_json::json!([{"amount":"1.00000000000000000001"}]),
                reporting_totals: serde_json::json!({"amount":"-0.00000000000000000001"}),
            });
        }
        let saved = CoreSnapshot::new(content).unwrap();
        let expected = serde_json::to_vec(&saved).unwrap();
        let actual = saved.canonical_bytes().unwrap();
        assert_eq!(actual, expected);
        let reopened: CoreSnapshot = serde_json::from_slice(&actual).unwrap();
        reopened.verify().unwrap();
        assert_eq!(reopened.id, saved.id);
        assert_eq!(reopened, saved);
        assert_eq!(serde_json::to_vec(&reopened).unwrap(), expected);
    }
}

#[test]
fn encoding_preserves_identity_schema_and_plan_rejections() {
    let saved = snapshot();
    let mut cases = Vec::new();
    let mut bad = saved.clone();
    bad.id = SnapshotId::new(format!("sha256:{}", "0".repeat(64))).unwrap();
    cases.push(bad);
    let mut bad = saved.clone();
    bad.content.schema_version = 2;
    cases.push(bad);
    let mut bad = saved.clone();
    bad.content.plan.curve_sets.insert("empty".into(), vec![]);
    cases.push(bad);
    let mut bad = saved.clone();
    let pin = bad.content.plan.definition.clone();
    bad.content
        .plan
        .curve_sets
        .insert("duplicate".into(), vec![pin.clone(), pin]);
    cases.push(bad);
    let mut bad = saved;
    if let MasterDefinition::Calculation(ref mut definition) = bad.content.definition.definition {
        definition.reference_inputs["marker"] = serde_json::json!("tampered");
    }
    cases.push(bad);
    for bad in cases {
        let existing = bad.verify().unwrap_err();
        let combined = bad.canonical_bytes().unwrap_err();
        assert_eq!(existing.to_string(), combined.to_string());
    }
}

#[test]
fn content_and_whole_file_size_bounds_stay_separate() {
    let mut content = snapshot().content;
    if let MasterDefinition::Calculation(ref mut definition) = content.definition.definition {
        definition.reference_inputs["padding"] = serde_json::json!("");
    }
    let overhead = serde_json::to_vec(&content).unwrap().len();
    if let MasterDefinition::Calculation(ref mut definition) = content.definition.definition {
        definition.reference_inputs["padding"] =
            serde_json::json!("x".repeat(MAX_SNAPSHOT_BYTES - overhead));
    }
    let boundary = CoreSnapshot::new(content).unwrap();
    boundary.verify().unwrap();
    assert_eq!(
        serde_json::to_vec(&boundary.content).unwrap().len(),
        MAX_SNAPSHOT_BYTES
    );
    let encoded = boundary.canonical_bytes().unwrap();
    assert!(encoded.len() > MAX_SNAPSHOT_BYTES);
    assert_eq!(encoded, serde_json::to_vec(&boundary).unwrap());
    // File adapters retain their original additional whole-envelope bound.
    let mut oversized = boundary;
    if let MasterDefinition::Calculation(ref mut definition) =
        oversized.content.definition.definition
    {
        definition.reference_inputs["padding"] =
            serde_json::json!("x".repeat(MAX_SNAPSHOT_BYTES - overhead + 1));
    }
    assert!(matches!(
        oversized.verify(),
        Err(TraitError::CapacityExceeded)
    ));
    assert!(matches!(
        oversized.canonical_bytes(),
        Err(TraitError::CapacityExceeded)
    ));
}
