//! Differential JSON/UTF-8 admission and canonical bytes through the real file port.
#![cfg(unix)]
use convex_core::contracts::*;
use convex_ext_file::FileSnapshotRepository;
use convex_ports::{
    core_workflow::*, reference_master::*, snapshot::SnapshotRepository, TraitError,
};
use std::{collections::BTreeMap, path::Path};

fn snapshot() -> CoreSnapshot {
    let at = UtcTimestamp::parse("2026-10-18T12:00:00Z").unwrap();
    let id = ReferenceDataId::new("json.decode.integrity").unwrap();
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
            source: SourceId::new("json.decode.test").unwrap(),
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
fn path(root: &Path, snapshot: &CoreSnapshot) -> std::path::PathBuf {
    root.join(format!(
        "{}.core.json",
        snapshot.id.as_str().strip_prefix("sha256:").unwrap()
    ))
}

#[tokio::test]
async fn string_and_slice_decoders_preserve_precision_and_exact_reopened_bytes() {
    let root = tempfile::tempdir().unwrap();
    let saved = snapshot();
    let bytes = serde_json::to_vec(&saved).unwrap();
    let sliced: CoreSnapshot = serde_json::from_slice(&bytes).unwrap();
    let text: CoreSnapshot = serde_json::from_str(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert_eq!(serde_json::to_vec(&sliced).unwrap(), bytes);
    assert_eq!(serde_json::to_vec(&text).unwrap(), bytes);
    assert_eq!(text, sliced);
    sliced.verify().unwrap();
    text.verify().unwrap();
    let repository = FileSnapshotRepository::new(root.path()).unwrap();
    repository.publish_core(&saved).await.unwrap();
    drop(repository);
    for _ in 0..3 {
        let reopened = FileSnapshotRepository::new(root.path()).unwrap();
        let actual = reopened.get_core(&saved.id).await.unwrap().unwrap();
        assert_eq!(serde_json::to_vec(&actual).unwrap(), bytes);
        assert_eq!(std::fs::read(path(root.path(), &saved)).unwrap(), bytes);
    }
}

#[tokio::test]
async fn invalid_utf8_and_json_never_return_a_previous_valid_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let saved = snapshot();
    let bytes = serde_json::to_vec(&saved).unwrap();
    let repository = FileSnapshotRepository::new(root.path()).unwrap();
    repository.publish_core(&saved).await.unwrap();
    let file = path(root.path(), &saved);
    let marker = bytes.windows(6).position(|v| v == b"stable").unwrap();
    let invalid = [
        vec![0xff],
        vec![0xc0, 0xaf],
        vec![0xed, 0xa0, 0x80],
        vec![0xf4, 0x90, 0x80, 0x80],
    ];
    let mut cases = Vec::new();
    for bad in invalid {
        let mut raw = bytes.clone();
        raw.splice(marker..marker + 6, bad);
        cases.push(raw);
    }
    cases.push(bytes[..bytes.len() - 1].to_vec());
    cases.push([bytes.as_slice(), b"{}"].concat());
    cases.push(
        String::from_utf8(bytes.clone())
            .unwrap()
            .replacen("stable", "\\uD800", 1)
            .into_bytes(),
    );
    cases.push(
        String::from_utf8(bytes.clone())
            .unwrap()
            .replacen("stable", "bad\ncontrol", 1)
            .into_bytes(),
    );
    for raw in cases {
        assert!(serde_json::from_slice::<CoreSnapshot>(&raw).is_err());
        assert!(std::str::from_utf8(&raw)
            .ok()
            .and_then(|v| serde_json::from_str::<CoreSnapshot>(v).ok())
            .is_none());
        std::fs::write(&file, &raw).unwrap();
        assert!(matches!(
            repository.get_core(&saved.id).await,
            Err(TraitError::InvalidInput(_))
        ));
        assert!(repository.publish_core(&saved).await.is_err());
        std::fs::write(&file, &bytes).unwrap();
        assert_eq!(
            repository.get_core(&saved.id).await.unwrap(),
            Some(saved.clone())
        );
    }
}
