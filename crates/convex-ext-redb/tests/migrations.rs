use convex_ext_redb::{migrate_database, RedbConfigStore};
use convex_ports::error::TraitError;
use convex_ports::storage::{BondPricingConfig, ConfigStore};
use redb::{Database, ReadableDatabase, TableDefinition};
use std::sync::Arc;

const CURRENT: TableDefinition<&str, &[u8]> = TableDefinition::new("pricing_configs");
const HISTORY: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("pricing_config_history");
const SCHEMA: TableDefinition<&str, u64> = TableDefinition::new("convex_schema");

fn old_database(records: &[(&str, Vec<u8>)]) -> (tempfile::TempDir, Arc<Database>) {
    let directory = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::create(directory.path().join("old.redb")).unwrap());
    let write = db.begin_write().unwrap();
    {
        let mut table = write.open_table(CURRENT).unwrap();
        for (id, bytes) in records {
            table.insert(*id, bytes.as_slice()).unwrap();
        }
    }
    write.commit().unwrap();
    (directory, db)
}

#[tokio::test]
async fn migration_retains_observed_version_without_inventing_lost_history() {
    let mut config: BondPricingConfig = serde_json::from_str(include_str!(
        "../../convex-ports/tests/fixtures/pricing_config.json"
    ))
    .unwrap();
    config.version = 7;
    let original = serde_json::to_vec(&config).unwrap();
    let (_directory, db) = old_database(&[(&config.config_id, original.clone())]);
    migrate_database(&db).unwrap();
    migrate_database(&db).unwrap();
    let store = RedbConfigStore::new(db.clone()).unwrap();
    assert_eq!(
        store.get_version(&config.config_id, 7).await.unwrap(),
        Some(config.clone())
    );
    for lost in 1..7 {
        assert!(store
            .get_version(&config.config_id, lost)
            .await
            .unwrap()
            .is_none());
    }
    assert_eq!(
        store
            .list_versions(&config.config_id)
            .await
            .unwrap()
            .iter()
            .map(|v| v.version)
            .collect::<Vec<_>>(),
        [7]
    );
    let read = db.begin_read().unwrap();
    assert_eq!(
        read.open_table(CURRENT)
            .unwrap()
            .get(config.config_id.as_str())
            .unwrap()
            .unwrap()
            .value(),
        original.as_slice()
    );
    drop(read);
    let mut next = config.clone();
    next.version = 8;
    next.updated_at = 200;
    store.save(&next).await.unwrap();
    assert_eq!(
        store.get_version(&config.config_id, 7).await.unwrap(),
        Some(config)
    );
    assert_eq!(
        store.get_version(&next.config_id, 8).await.unwrap(),
        Some(next)
    );
}

#[test]
fn failed_migration_rolls_back_all_history_and_schema_changes() {
    let original = include_bytes!("../../convex-ports/tests/fixtures/pricing_config.json").to_vec();
    let (_directory, db) = old_database(&[
        ("CONFIG-RESTORE", original.clone()),
        ("ZZZ-invalid", b"{}".to_vec()),
    ]);
    assert!(migrate_database(&db).is_err());
    let read = db.begin_read().unwrap();
    assert!(matches!(
        read.open_table(SCHEMA),
        Err(redb::TableError::TableDoesNotExist(_))
    ));
    assert!(matches!(
        read.open_table(HISTORY),
        Err(redb::TableError::TableDoesNotExist(_))
    ));
    assert_eq!(
        read.open_table(CURRENT)
            .unwrap()
            .get("CONFIG-RESTORE")
            .unwrap()
            .unwrap()
            .value(),
        original.as_slice()
    );
    assert_eq!(
        read.open_table(CURRENT)
            .unwrap()
            .get("ZZZ-invalid")
            .unwrap()
            .unwrap()
            .value(),
        b"{}"
    );
}

#[test]
fn mismatched_config_identity_cannot_be_migrated() {
    let (_directory, db) = old_database(&[(
        "WRONG-ID",
        include_bytes!("../../convex-ports/tests/fixtures/pricing_config.json").to_vec(),
    )]);
    assert!(matches!(
        migrate_database(&db),
        Err(TraitError::Unsupported(_))
    ));
    assert!(matches!(
        db.begin_read().unwrap().open_table(SCHEMA),
        Err(redb::TableError::TableDoesNotExist(_))
    ));
}

#[test]
fn newer_schema_and_missing_history_are_not_silently_adopted() {
    for version in [1, 2] {
        let (_directory, db) = old_database(&[]);
        let write = db.begin_write().unwrap();
        write
            .open_table(SCHEMA)
            .unwrap()
            .insert("version", version)
            .unwrap();
        write.commit().unwrap();
        assert!(matches!(
            migrate_database(&db),
            Err(TraitError::Unsupported(_))
        ));
        assert!(matches!(
            db.begin_read().unwrap().open_table(HISTORY),
            Err(redb::TableError::TableDoesNotExist(_))
        ));
    }
}

#[test]
fn old_bonds_fail_before_any_schema_change() {
    let (_directory, db) = old_database(&[]);
    let write = db.begin_write().unwrap();
    write
        .open_table(TableDefinition::<&str, &[u8]>::new("bonds"))
        .unwrap()
        .insert("old", b"{}".as_slice())
        .unwrap();
    write.commit().unwrap();
    assert!(matches!(
        migrate_database(&db),
        Err(TraitError::Unsupported(_))
    ));
    assert!(matches!(
        db.begin_read().unwrap().open_table(SCHEMA),
        Err(redb::TableError::TableDoesNotExist(_))
    ));
}

#[tokio::test]
async fn current_history_disagreement_is_rejected_on_reopen() {
    let (_directory, db) = old_database(&[]);
    let store = RedbConfigStore::new(db.clone()).unwrap();
    let config: BondPricingConfig = serde_json::from_str(include_str!(
        "../../convex-ports/tests/fixtures/pricing_config.json"
    ))
    .unwrap();
    store.save(&config).await.unwrap();
    let mut altered = config.clone();
    altered.description = "changed current only".into();
    let write = db.begin_write().unwrap();
    write
        .open_table(CURRENT)
        .unwrap()
        .insert(
            config.config_id.as_str(),
            serde_json::to_vec(&altered).unwrap().as_slice(),
        )
        .unwrap();
    write.commit().unwrap();
    assert!(matches!(
        RedbConfigStore::new(db.clone()),
        Err(TraitError::Unsupported(_))
    ));
    // Verification does not rewrite the previously retained historical content.
    assert_eq!(
        store.get_version(&config.config_id, 1).await.unwrap(),
        Some(config)
    );
}

#[tokio::test]
#[cfg(unix)]
async fn backup_before_migration_restores_original_schema_and_retained_content() {
    use convex_ext_redb::{backup_database, restore_database};
    let mut config: BondPricingConfig = serde_json::from_str(include_str!(
        "../../convex-ports/tests/fixtures/pricing_config.json"
    ))
    .unwrap();
    config.version = 7;
    let bytes = serde_json::to_vec(&config).unwrap();
    let (directory, db) = old_database(&[(&config.config_id, bytes.clone())]);
    drop(db);
    let source = directory.path().join("old.redb");
    let backup = directory.path().join("before-migration");
    assert_eq!(
        backup_database(&source, &backup)
            .unwrap()
            .application_schema_version,
        0
    );
    let store = RedbConfigStore::new(Arc::new(Database::open(&source).unwrap())).unwrap();
    let mut next = config.clone();
    next.version = 8;
    next.updated_at = 200;
    store.save(&next).await.unwrap();
    drop(store);
    let target = directory.path().join("rollback-check.redb");
    assert_eq!(
        restore_database(&backup, &target)
            .unwrap()
            .application_schema_version,
        0
    );
    let restored = Arc::new(Database::open(&target).unwrap());
    let read = restored.begin_read().unwrap();
    assert!(matches!(
        read.open_table(SCHEMA),
        Err(redb::TableError::TableDoesNotExist(_))
    ));
    assert!(matches!(
        read.open_table(HISTORY),
        Err(redb::TableError::TableDoesNotExist(_))
    ));
    assert_eq!(
        read.open_table(CURRENT)
            .unwrap()
            .get(config.config_id.as_str())
            .unwrap()
            .unwrap()
            .value(),
        bytes.as_slice()
    );
    drop(read);
    let restored = RedbConfigStore::new(restored).unwrap();
    assert_eq!(
        restored.get_version(&config.config_id, 7).await.unwrap(),
        Some(config.clone())
    );
    assert!(restored
        .get_version(&config.config_id, 8)
        .await
        .unwrap()
        .is_none());
    let source = RedbConfigStore::new(Arc::new(Database::open(&source).unwrap())).unwrap();
    assert_eq!(
        source.get_version(&config.config_id, 8).await.unwrap(),
        Some(next)
    );
}
