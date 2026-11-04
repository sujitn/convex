//! Core byte integrity and file admission controls; no financial answers supplied.
#![cfg(unix)]
use convex_core::contracts::*;
use convex_ext_file::FileSnapshotRepository;
use convex_ports::{core_workflow::*, reference_master::*, snapshot::*, TraitError};
use std::{collections::BTreeMap, path::Path};

fn snapshot() -> CoreSnapshot {
    let at = UtcTimestamp::parse("2026-10-09T12:00:00Z").unwrap();
    let id = ReferenceDataId::new("snapshot.io.integrity").unwrap();
    // Opaque port payload, deliberately including nested numbers and escaping.
    // Native financial admission belongs to the engine and existing E2E cases.
    let definition = CoreDefinition {
        id: id.clone(),
        reference_inputs: serde_json::json!({"marker":"stable","nested":[1.25,-0.0,"bond\nΩ"]}),
        calculations: BTreeMap::from([(
            "probe".into(),
            CoreCalculation {
                request: serde_json::json!({"schema_version":1,"payload":{"nested":[true,null,"\\\""]}}),
                curve_sets: vec![],
            },
        )]),
        effective: EffectiveInterval {
            from: at,
            until: None,
        },
        provenance: ReferenceProvenance {
            source: SourceId::new("snapshot.io.test").unwrap(),
            evidence_reference: "Opaque byte integrity control, not pricing".into(),
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
fn path(directory: &Path, snapshot: &CoreSnapshot) -> std::path::PathBuf {
    directory.join(format!(
        "{}.core.json",
        snapshot.id.as_str().strip_prefix("sha256:").unwrap()
    ))
}

#[tokio::test]
async fn repeated_verified_reads_reopen_and_publication_keep_exact_owned_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let saved = snapshot();
    let wire = serde_json::to_vec(&saved).unwrap();
    let repository = FileSnapshotRepository::new(dir.path()).unwrap();
    for _ in 0..3 {
        saved.verify().unwrap();
        repository.publish_core(&saved).await.unwrap();
        assert_eq!(
            repository.get_core(&saved.id).await.unwrap(),
            Some(saved.clone())
        );
        assert_eq!(std::fs::read(path(dir.path(), &saved)).unwrap(), wire);
    }
    let reopened = FileSnapshotRepository::new(dir.path()).unwrap();
    assert_eq!(reopened.get_core(&saved.id).await.unwrap(), Some(saved));
}

#[tokio::test]
async fn duplicate_keys_alternate_number_spelling_and_noncanonical_envelopes_still_reject() {
    let dir = tempfile::tempdir().unwrap();
    let saved = snapshot();
    let repository = FileSnapshotRepository::new(dir.path()).unwrap();
    repository.publish_core(&saved).await.unwrap();
    let file = path(dir.path(), &saved);
    let original = std::fs::read_to_string(&file).unwrap();
    assert_eq!(original.matches("\"marker\":\"stable\"").count(), 1);
    assert_eq!(original.matches("1.25").count(), 1);
    let duplicate = original.replace(
        "\"marker\":\"stable\"",
        "\"marker\":\"discarded\",\"marker\":\"stable\"",
    );
    let alternate = original.replace("1.25", "1.250");
    // Both decode to the original typed value and pass content hash checks.
    // The independent canonical file check must still reject their raw bytes.
    for raw in [&duplicate, &alternate] {
        let decoded: CoreSnapshot = serde_json::from_str(raw).unwrap();
        assert_eq!(decoded, saved);
        decoded.verify().unwrap();
    }
    let suffix = "\"fixings\":[]}}";
    assert!(original.ends_with(suffix));
    let explicit_null = format!(
        "{}\"fixings\":[],\"issuer_scope\":null}}}}",
        &original[..original.len() - suffix.len()]
    );
    let decoded: CoreSnapshot = serde_json::from_str(&explicit_null).unwrap();
    assert_eq!(decoded, saved);
    decoded.verify().unwrap();
    for raw in [duplicate, alternate, explicit_null, original.clone() + "\n"] {
        std::fs::write(&file, raw).unwrap();
        assert!(repository.get_core(&saved.id).await.is_err());
        assert!(repository.publish_core(&saved).await.is_err());
    }
    std::fs::write(file, original).unwrap();
    assert_eq!(repository.get_core(&saved.id).await.unwrap(), Some(saved));
}

#[tokio::test]
async fn nested_content_tampering_and_wrong_requested_identity_never_reuse_verified_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let saved = snapshot();
    let repository = FileSnapshotRepository::new(dir.path()).unwrap();
    repository.publish_core(&saved).await.unwrap();
    let mut corrupt = saved.clone();
    let MasterDefinition::Calculation(ref mut definition) = corrupt.content.definition.definition
    else {
        panic!("fixture calculation");
    };
    definition.reference_inputs["nested"][0] = serde_json::json!(1.5);
    assert!(matches!(corrupt.verify(), Err(TraitError::Conflict(_))));
    assert!(repository.publish_core(&corrupt).await.is_err());
    let other = SnapshotId::new(format!("sha256:{}", "0".repeat(64))).unwrap();
    assert!(repository.get_core(&other).await.unwrap().is_none());
    let wrong = dir.path().join(format!("{}.core.json", "0".repeat(64)));
    std::fs::write(wrong, serde_json::to_vec(&saved).unwrap()).unwrap();
    assert!(repository.get_core(&other).await.is_err());
    assert!(repository
        .get_core(&SnapshotId::new("relative-path").unwrap())
        .await
        .is_err());
    assert_eq!(repository.get_core(&saved.id).await.unwrap(), Some(saved));
}

#[test]
fn construction_and_verification_share_schema_plan_and_size_guards() {
    let saved = snapshot();
    let mut invalid = saved.clone();
    invalid.content.schema_version = 2;
    assert!(matches!(invalid.verify(), Err(TraitError::InvalidInput(_))));
    assert!(matches!(
        CoreSnapshot::new(invalid.content),
        Err(TraitError::InvalidInput(_))
    ));
    let mut invalid = saved.clone();
    invalid
        .content
        .plan
        .curve_sets
        .insert("empty".into(), vec![]);
    assert!(matches!(
        invalid.verify(),
        Err(TraitError::CapacityExceeded)
    ));
    assert!(matches!(
        CoreSnapshot::new(invalid.content),
        Err(TraitError::CapacityExceeded)
    ));
    let mut invalid = saved.clone();
    let pin = invalid.content.plan.definition.clone();
    invalid
        .content
        .plan
        .curve_sets
        .insert("duplicate".into(), vec![pin.clone(), pin]);
    assert!(matches!(invalid.verify(), Err(TraitError::InvalidInput(_))));
    assert!(matches!(
        CoreSnapshot::new(invalid.content),
        Err(TraitError::InvalidInput(_))
    ));
    let mut invalid = saved;
    let MasterDefinition::Calculation(ref mut definition) = invalid.content.definition.definition
    else {
        panic!("fixture calculation");
    };
    definition.reference_inputs["marker"] =
        serde_json::Value::String("x".repeat(MAX_SNAPSHOT_BYTES));
    assert!(matches!(
        invalid.verify(),
        Err(TraitError::CapacityExceeded)
    ));
    assert!(matches!(
        CoreSnapshot::new(invalid.content),
        Err(TraitError::CapacityExceeded)
    ));
}

#[tokio::test]
async fn symlinks_nonregular_files_and_oversized_raw_files_remain_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let saved = snapshot();
    let repository = FileSnapshotRepository::new(dir.path()).unwrap();
    repository.publish_core(&saved).await.unwrap();
    let file = path(dir.path(), &saved);
    let original = std::fs::read(&file).unwrap();
    let backup = dir.path().join("backup.json");
    std::fs::rename(&file, &backup).unwrap();
    std::os::unix::fs::symlink(&backup, &file).unwrap();
    assert!(repository.get_core(&saved.id).await.is_err());
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    assert!(repository.get_core(&saved.id).await.is_err());
    std::fs::remove_dir(&file).unwrap();
    let oversized = std::fs::File::create(&file).unwrap();
    oversized.set_len(MAX_SNAPSHOT_BYTES as u64 + 1).unwrap();
    assert!(matches!(
        repository.get_core(&saved.id).await,
        Err(TraitError::CapacityExceeded)
    ));
    drop(oversized);
    std::fs::write(file, original).unwrap();
    assert_eq!(repository.get_core(&saved.id).await.unwrap(), Some(saved));
}
