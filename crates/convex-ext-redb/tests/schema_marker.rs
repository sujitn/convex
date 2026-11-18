use convex_ext_redb::{migrate_database, RedbConfigStore};
use convex_ports::error::TraitError;
use convex_ports::storage::{BondPricingConfig, ConfigStore};
use redb::{Database, ReadableDatabase, TableDefinition};
use std::sync::Arc;

const CURRENT: TableDefinition<&str, &[u8]> = TableDefinition::new("pricing_configs");
const HISTORY: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("pricing_config_history");
const SCHEMA: TableDefinition<&str, u64> = TableDefinition::new("convex_schema");

fn unmarked_database(records: &[(&str, Vec<u8>)]) -> (tempfile::TempDir, Arc<Database>) {
    let directory = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::create(directory.path().join("unmarked.redb")).unwrap());
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

#[test]
fn configurations_without_a_schema_marker_are_refused_and_left_untouched() {
    let original = include_bytes!("../../convex-ports/tests/fixtures/pricing_config.json").to_vec();
    let (_directory, db) = unmarked_database(&[("CONFIG-RESTORE", original.clone())]);
    assert!(matches!(
        migrate_database(&db),
        Err(TraitError::Unsupported(_))
    ));
    assert!(RedbConfigStore::new(db.clone()).is_err());
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
}

#[test]
fn a_newer_marker_or_a_marker_without_history_is_refused() {
    for version in [1, 2] {
        let (_directory, db) = unmarked_database(&[]);
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
fn unversioned_bonds_are_refused_before_any_schema_change() {
    let (_directory, db) = unmarked_database(&[]);
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
    let (_directory, db) = unmarked_database(&[]);
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
