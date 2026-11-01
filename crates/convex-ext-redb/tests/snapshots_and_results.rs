//! Snapshots and archived results on the redb database: exact retries, tamper
//! refusal, reopen, and the 16 MiB snapshot bound.
#[path = "../../convex-ports/tests/support/snapshot_fixtures.rs"]
mod fixture;

use convex_core::contracts::*;
use convex_ext_redb::{open_redb_storage, RedbStorage};
use convex_ports::{
    core_workflow::*,
    reference_master::*,
    runtime::{PublishedValuation, ResultPublisher},
    snapshot::{SnapshotRepository, MAX_SNAPSHOT_BYTES},
    TraitError,
};
use redb::{ReadableDatabase, TableDefinition};
use std::{collections::BTreeMap, path::Path};

const MARKET: TableDefinition<&str, &[u8]> = TableDefinition::new("market_snapshots_v1");

fn open(path: &Path) -> RedbStorage {
    open_redb_storage(path).unwrap()
}

/// Overwrite one stored market snapshot, as on-disk corruption would.
fn tamper(path: &Path, id: &SnapshotId, bytes: &[u8]) {
    let db = redb::Database::create(path).unwrap();
    let write = db.begin_write().unwrap();
    write
        .open_table(MARKET)
        .unwrap()
        .insert(id.as_str(), bytes)
        .unwrap();
    write.commit().unwrap();
}

fn stored(path: &Path, id: &SnapshotId) -> Vec<u8> {
    let db = redb::Database::create(path).unwrap();
    let read = db.begin_read().unwrap();
    let table = read.open_table(MARKET).unwrap();
    let value = table.get(id.as_str()).unwrap().unwrap();
    value.value().to_vec()
}

/// An opaque calculation definition; the adapter stores bytes, not financial answers.
fn core_snapshot(padding: usize) -> CoreSnapshot {
    let at = UtcTimestamp::parse("2026-10-23T12:00:00Z").unwrap();
    let id = ReferenceDataId::new("snapshot.redb.integrity").unwrap();
    let definition = CoreDefinition {
        id: id.clone(),
        reference_inputs: serde_json::json!({"padding": "x".repeat(padding)}),
        calculations: BTreeMap::from([(
            "probe".into(),
            CoreCalculation {
                request: serde_json::json!({"schema_version":1,"payload":{}}),
                curve_sets: vec![],
            },
        )]),
        effective: EffectiveInterval {
            from: at,
            until: None,
        },
        provenance: ReferenceProvenance {
            source: SourceId::new("snapshot.redb.test").unwrap(),
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

fn valuation(revision: u64, message: &str) -> PublishedValuation {
    let at = UtcTimestamp::parse("2026-10-23T12:00:00Z").unwrap();
    let error = StructuredError::new(ErrorCode::MissingMarketData, None, message, false).unwrap();
    let result = ResultEnvelope::new(
        RequestId::new(format!("request-{revision}")).unwrap(),
        ResultStatus::Failed,
        DataQuality::Invalid,
        BTreeMap::new(),
        vec![error],
        Provenance {
            subject: ValuationSubject::InstrumentVersion {
                instrument_id: InstrumentId::new("BOND-001").unwrap(),
                version: Version::new(1).unwrap(),
            },
            snapshot_id: SnapshotId::new(format!("sha256:{}", "0".repeat(64))).unwrap(),
            configuration_version: Version::new(1).unwrap(),
            valuation_time: at,
            known_at: at,
            calculated_at: at,
            model_version: ModelId::new("model-1").unwrap(),
            build_id: ModelId::new("build-1").unwrap(),
            input_versions: vec![],
        },
    )
    .unwrap();
    PublishedValuation {
        runtime_id: SourceId::new("runtime.redb.test").unwrap(),
        revision,
        generation: Version::new(1).unwrap(),
        context_key: "context".into(),
        live_valid_until: None,
        result,
        last_good: None,
        retained_quality: None,
    }
}

#[tokio::test]
async fn market_snapshots_retry_exactly_reopen_and_refuse_tampering() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.redb");
    let snapshot = fixture::snapshot();
    {
        let storage = open(&path);
        for _ in 0..3 {
            storage.snapshots.publish(&snapshot).await.unwrap();
        }
        let absent = SnapshotId::new(format!("sha256:{}", "0".repeat(64))).unwrap();
        assert!(storage.snapshots.get(&absent).await.unwrap().is_none());
        let arbitrary = SnapshotId::new("relative-path").unwrap();
        assert!(storage.snapshots.get(&arbitrary).await.is_err());
    }
    let storage = open(&path);
    assert_eq!(
        storage.snapshots.get(snapshot.id()).await.unwrap(),
        Some(snapshot.clone())
    );
    drop(storage);

    let original = stored(&path, snapshot.id());
    let corrupt = String::from_utf8(original.clone())
        .unwrap()
        .replace("99.25", "99.26");
    tamper(&path, snapshot.id(), corrupt.as_bytes());
    let storage = open(&path);
    assert!(storage.snapshots.get(snapshot.id()).await.is_err());
    assert!(matches!(
        storage.snapshots.publish(&snapshot).await,
        Err(TraitError::Conflict(_))
    ));
    drop(storage);
    assert_eq!(stored(&path, snapshot.id()), corrupt.as_bytes());
    // Only canonical bytes are admitted, so an alternate encoding cannot hide tampering.
    tamper(&path, snapshot.id(), &[original.as_slice(), b"\n"].concat());
    assert!(open(&path).snapshots.get(snapshot.id()).await.is_err());
}

/// Schema 4 (P13b-ii) stores and reopens as schemas 1–3 do. A changed quote and a changed
/// reference, which it hashes in separate sections, are each refused.
#[tokio::test]
async fn sectioned_snapshots_reopen_and_refuse_tampering() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.redb");
    let snapshot = fixture::sectioned_snapshot();
    open(&path).snapshots.publish(&snapshot).await.unwrap();
    let storage = open(&path);
    assert_eq!(
        storage.snapshots.get(snapshot.id()).await.unwrap(),
        Some(snapshot.clone())
    );
    drop(storage);
    let original = String::from_utf8(stored(&path, snapshot.id())).unwrap();
    // The price is in the quotes alone, the bond's name in the references alone.
    for (from, to) in [
        ("99.25", "99.26"),
        ("Fictional reference bond", "Fictional reference bonD"),
    ] {
        assert!(original.contains(from));
        tamper(&path, snapshot.id(), original.replace(from, to).as_bytes());
        assert!(open(&path).snapshots.get(snapshot.id()).await.is_err());
    }
}

#[tokio::test]
async fn core_snapshots_round_trip_at_the_16_mib_bound_and_refuse_one_byte_more() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.redb");
    let overhead = core_snapshot(0).canonical_bytes().unwrap().len();
    let boundary = core_snapshot(MAX_SNAPSHOT_BYTES - overhead);
    assert_eq!(
        boundary.canonical_bytes().unwrap().len(),
        MAX_SNAPSHOT_BYTES
    );
    {
        let storage = open(&path);
        storage.snapshots.publish_core(&boundary).await.unwrap();
        storage.snapshots.publish_core(&boundary).await.unwrap();
        let over = core_snapshot(MAX_SNAPSHOT_BYTES - overhead + 1);
        assert!(matches!(
            storage.snapshots.publish_core(&over).await,
            Err(TraitError::CapacityExceeded)
        ));
        assert!(storage
            .snapshots
            .get_core(&over.id)
            .await
            .unwrap()
            .is_none());
    }
    let storage = open(&path);
    assert_eq!(
        storage.snapshots.get_core(&boundary.id).await.unwrap(),
        Some(boundary.clone())
    );
    // Core and market snapshots are separate tables under the same digest form.
    assert!(storage.snapshots.get(&boundary.id).await.unwrap().is_none());
}

#[tokio::test]
async fn results_archive_exact_redelivery_and_refuses_a_changed_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.redb");
    let first = valuation(1, "fixture gap");
    {
        let storage = open(&path);
        assert!(storage.results.is_empty().unwrap());
        storage.results.publish(&first).await.unwrap();
        storage.results.publish(&first).await.unwrap();
        assert!(matches!(
            storage.results.publish(&valuation(1, "different")).await,
            Err(TraitError::Conflict(_))
        ));
        storage
            .results
            .publish(&valuation(2, "fixture gap"))
            .await
            .unwrap();
    }
    let storage = open(&path);
    assert_eq!(storage.results.len().unwrap(), 2);
    assert_eq!(
        storage.results.get(&first.runtime_id, 1).unwrap(),
        Some(first.clone())
    );
    assert!(storage.results.get(&first.runtime_id, 3).unwrap().is_none());
}
