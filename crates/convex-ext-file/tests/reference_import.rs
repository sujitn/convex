use convex_core::contracts::UtcTimestamp;
use convex_core::ids::InstrumentId;
use convex_ext_file::{
    import_reference_file, load_reference_import, MemoryReferenceRepository,
    MAX_REFERENCE_IMPORT_BYTES,
};
use convex_ports::reference_repository::{ReferenceError, ReferenceRepository};

const INITIAL: &str = include_str!("fixtures/instrument-master.json");
const CORRECTION: &str = include_str!("fixtures/instrument-master-correction.json");
fn time(value: &str) -> UtcTimestamp {
    UtcTimestamp::parse(value).unwrap()
}

#[tokio::test]
async fn full_import_and_correction_are_atomic_and_retryable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("import.json");
    std::fs::write(&path, INITIAL).unwrap();
    let repository = MemoryReferenceRepository::default();
    let first = import_reference_file(&repository, &path, time("2025-01-01T00:00:00Z"))
        .await
        .unwrap();
    assert_eq!(first.records.len(), 1);
    assert_eq!(first.master_records.len(), 5);
    let initial = load_reference_import(&path).unwrap();
    std::fs::write(&path, CORRECTION).unwrap();
    let revised = import_reference_file(&repository, &path, time("2026-02-01T00:00:00Z"))
        .await
        .unwrap();
    assert_eq!(revised.records[0].version.get(), 2);
    assert_eq!(
        import_reference_file(&repository, &path, time("2026-03-01T00:00:00Z"))
            .await
            .unwrap(),
        revised
    );
    assert_eq!(
        repository
            .get_version(
                &InstrumentId::new("INSTRUMENT-001"),
                first.records[0].version
            )
            .await
            .unwrap(),
        Some(first.records[0].clone())
    );
    assert_eq!(
        repository
            .commit(&initial.command, time("2026-03-01T00:00:00Z"))
            .await
            .unwrap(),
        first
    );
}

#[tokio::test]
async fn invalid_rows_and_transaction_conflicts_report_locations_and_commit_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.json");
    let mut value: serde_json::Value = serde_json::from_str(INITIAL).unwrap();
    let mut second = value["command"]["changes"][0].clone();
    second["bond"]["instrument_id"] = "SECOND".into();
    second["bond"]["day_count"] = "UNKNOWN".into();
    value["command"]["changes"]
        .as_array_mut()
        .unwrap()
        .push(second);
    value["command"]["changes"][0]["bond"]["contractual_terms"] = serde_json::Value::Null;
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let error = load_reference_import(&path).unwrap_err();
    assert_eq!(error.diagnostics.len(), 2);
    assert_eq!(error.diagnostics[0].row, Some(0));
    assert_eq!(error.diagnostics[0].field, "contractual_terms");
    assert_eq!(error.diagnostics[1].row, Some(1));
    assert_eq!(error.diagnostics[1].field, "day_count");
    let repository = MemoryReferenceRepository::default();
    assert!(
        import_reference_file(&repository, &path, time("2025-01-01T00:00:00Z"))
            .await
            .is_err()
    );
    assert!(repository
        .get(&InstrumentId::new("INSTRUMENT-001"))
        .await
        .unwrap()
        .is_none());
    std::fs::write(&path, INITIAL).unwrap();
    import_reference_file(&repository, &path, time("2025-01-01T00:00:00Z"))
        .await
        .unwrap();
    let mut conflicting: serde_json::Value = serde_json::from_str(CORRECTION).unwrap();
    conflicting["command"]["changes"][0]["expected_version"] = 9.into();
    std::fs::write(&path, serde_json::to_vec(&conflicting).unwrap()).unwrap();
    let error = import_reference_file(&repository, &path, time("2026-02-01T00:00:00Z"))
        .await
        .unwrap_err();
    assert!(matches!(error.error, ReferenceError::Conflict(_)));
    assert_eq!(error.diagnostics[0].section, "changes");
    assert_eq!(error.diagnostics[0].row, Some(0));
    assert_eq!(error.diagnostics[0].code, "VERSION_CONFLICT");
    assert_eq!(
        repository
            .get(&InstrumentId::new("INSTRUMENT-001"))
            .await
            .unwrap()
            .unwrap()
            .version
            .get(),
        1
    );
}

#[test]
fn strict_decoding_and_bounded_files_never_supply_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");
    for input in [
        "{",
        "[]",
        "{\"schema_version\":\"9.0\",\"command\":{}}",
        "{\"schema_version\":\"2.0\",\"schema_version\":\"2.0\",\"command\":{}}",
    ] {
        std::fs::write(&path, input).unwrap();
        let error = load_reference_import(&path).unwrap_err();
        assert_eq!(error.diagnostics[0].code, "INVALID_INPUT");
        assert!(error.diagnostics[0].line.is_some());
    }
    for field in ["dated_date", "redemption_value", "calendar", "settings"] {
        let mut value: serde_json::Value = serde_json::from_str(INITIAL).unwrap();
        value["command"]["changes"][0]["bond"]["contractual_terms"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(load_reference_import(&path).is_err());
    }
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(MAX_REFERENCE_IMPORT_BYTES + 1).unwrap();
    assert_eq!(
        load_reference_import(&path).unwrap_err().error,
        ReferenceError::CapacityExceeded
    );
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        load_reference_import(&path).unwrap_err().error,
        ReferenceError::Unavailable
    );
    assert!(load_reference_import(dir.path()).is_err());
}
