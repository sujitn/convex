use convex_core::contracts::{RequestId, UtcTimestamp, Version};
use convex_ext_redb::create_redb_storage;
// Offline backup and restore run on Unix only.
#[cfg(unix)]
use convex_ext_redb::{backup_database, restore_database};
#[cfg(unix)]
use convex_ports::error::TraitError;
use convex_ports::reference_data::BondReferenceData;
use convex_ports::reference_repository::{ReferenceChange, ReferenceCommand, ReferenceError};
use convex_ports::storage::*;
use serde::Deserialize;
use std::path::Path;
use std::process::Command;
#[path = "../../convex-ports/tests/support/configuration_fixtures.rs"]
mod configuration_fixture;
use configuration_fixture::snapshot::reference_terms;

#[derive(Deserialize)]
struct Seed {
    fixture_id: String,
    configs: Vec<BondPricingConfig>,
    curve_recipes: Vec<convex_ports::snapshot::SnapshotCurveRecipe>,
    audit: Vec<AuditEntry>,
    reference_bond: BondReferenceData,
    reference_revision_description: String,
    reference_create_key: String,
    reference_update_key: String,
    recorded_at: UtcTimestamp,
}
fn seed() -> Seed {
    serde_json::from_str(include_str!("fixtures/restore_seed.json")).unwrap()
}
fn initial(seed: &Seed) -> ReferenceCommand {
    ReferenceCommand {
        master_changes: reference_terms::resources(&seed.reference_bond),
        idempotency_key: RequestId::new(&seed.reference_create_key).unwrap(),
        changes: vec![ReferenceChange::Create {
            bond: Box::new(seed.reference_bond.clone()),
        }],
    }
}

fn staged_configurations(seed: &Seed) -> Vec<convex_ports::configuration::StagedConfiguration> {
    seed.configs
        .iter()
        .map(|pricing| {
            let mut staged = configuration_fixture::staged(pricing.version);
            // A separate managed identity preserves the original inactive pricing v2 fixture exactly.
            staged.document.pricing = seed.configs[0].clone();
            staged.document.pricing.config_id = "CONFIG-MANAGED-RESTORE".into();
            staged.document.pricing.version = pricing.version;
            staged.document.pricing.updated_at = pricing.updated_at;
            staged.document.assembly.configuration.id =
                convex_core::contracts::SourceId::new("CONFIG-MANAGED-RESTORE").unwrap();
            staged
        })
        .collect()
}
fn activation_commands() -> Vec<convex_ports::configuration::ActivationCommand> {
    use convex_ports::configuration::{ActivationKind, ReloadClass};
    let initial = configuration_fixture::activate(1, 0, "restore-activation-1");
    let mut second = configuration_fixture::activate(2, 1, "restore-activation-2");
    second.mode = ReloadClass::RestartRequired;
    let mut rollback = configuration_fixture::activate(1, 2, "restore-rollback-1");
    rollback.mode = ReloadClass::RestartRequired;
    rollback.action = ActivationKind::Rollback {
        to_revision: Version::new(1).unwrap(),
    };
    let mut commands = vec![initial, second, rollback];
    for command in &mut commands {
        command.id = convex_core::contracts::SourceId::new("CONFIG-MANAGED-RESTORE").unwrap();
    }
    commands
}

async fn populate(path: &Path) {
    let seed = seed();
    assert_eq!(seed.fixture_id, "CVX-004-RESTORE-001");
    let storage = create_redb_storage(path).unwrap();
    for config in &seed.configs {
        storage.configs.save(config).await.unwrap();
    }
    for recipe in &seed.curve_recipes {
        storage.curves.save_recipe(recipe).await.unwrap();
    }
    for entry in &seed.audit {
        storage.audit.append(entry).await.unwrap();
    }
    let create = initial(&seed);
    storage
        .bonds
        .commit(&create, seed.recorded_at)
        .await
        .unwrap();
    let mut bond = seed.reference_bond.clone();
    bond.description = seed.reference_revision_description.clone();
    storage
        .bonds
        .commit(
            &ReferenceCommand {
                master_changes: Vec::new(),
                idempotency_key: RequestId::new(&seed.reference_update_key).unwrap(),
                changes: vec![ReferenceChange::Update {
                    bond: Box::new(bond),
                    expected_version: Version::new(1).unwrap(),
                }],
            },
            seed.recorded_at,
        )
        .await
        .unwrap();
    for staged in staged_configurations(&seed) {
        storage.configuration.stage(&staged).await.unwrap();
    }
    for command in activation_commands() {
        storage.configuration.activate(&command).await.unwrap();
    }
    // Exit with live storage handles: no destructor, close or reply after the final durable commit.
    std::process::exit(17);
}

async fn verify(path: &Path) {
    verify_state(path, true).await;
    // Offline compaction normalizes allocator/free-page state after the larger fixture.
    // Verify physical integrity and then reconcile all business records again.
    // Keep backup_database's fail-closed repair guard unchanged.
    let compact = Command::new(env!("CARGO_BIN_EXE_convex-redb"))
        .args(["compact".as_ref(), path.as_os_str()])
        .output()
        .unwrap();
    assert!(
        compact.status.success(),
        "{}",
        String::from_utf8_lossy(&compact.stderr)
    );
    // Explicit physical repair is followed by exact business reconciliation and another clean check.
    let repair = Command::new(env!("CARGO_BIN_EXE_convex-redb"))
        .args(["repair".as_ref(), path.as_os_str()])
        .output()
        .unwrap();
    assert!(
        repair.status.success(),
        "{}",
        String::from_utf8_lossy(&repair.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&repair.stdout).unwrap();
    assert_eq!(report["physical_integrity_checked"], true);
    assert_eq!(report["business_records_reconciled"], false);
    verify_state(path, false).await;
    let mut database = redb::Database::open(path).unwrap();
    assert!(
        database.check_integrity().unwrap(),
        "reconciled fixture must pass integrity without another repair"
    );
    drop(database);
    // Reconcile every retained business value again after any physical repair,
    // without issuing retry writes that would change allocator state before backup.
    verify_state(path, false).await;
}

async fn verify_state(path: &Path, check_retries: bool) {
    let seed = seed();
    let storage = create_redb_storage(path).unwrap();
    for config in &seed.configs {
        assert_eq!(
            storage
                .configs
                .get_version(&config.config_id, config.version)
                .await
                .unwrap(),
            Some(config.clone())
        );
        if check_retries {
            storage.configs.save(config).await.unwrap();
        }
    }
    assert_eq!(
        storage
            .configs
            .get(&seed.configs[0].config_id)
            .await
            .unwrap(),
        Some(seed.configs[1].clone())
    );
    assert_eq!(
        storage
            .configs
            .list_versions(&seed.configs[0].config_id)
            .await
            .unwrap()
            .len(),
        2
    );
    for recipe in &seed.curve_recipes {
        assert_eq!(
            storage
                .curves
                .get_recipe(&recipe.curve_id.clone().into(), recipe.version)
                .await
                .unwrap()
                .as_ref(),
            Some(recipe)
        );
    }
    let audit = storage
        .audit
        .query(&AuditFilter::default(), &Pagination::new(0, 1000))
        .await
        .unwrap();
    assert_eq!(audit.items, seed.audit);
    assert_eq!(audit.total, 2);
    for staged in staged_configurations(&seed) {
        assert_eq!(
            storage
                .configuration
                .get_staged(
                    &convex_core::contracts::SourceId::new(&staged.document.pricing.config_id)
                        .unwrap(),
                    Version::new(staged.document.pricing.version).unwrap()
                )
                .await
                .unwrap(),
            Some(staged.clone())
        );
        if check_retries {
            storage.configuration.stage(&staged).await.unwrap();
        }
    }
    let commands = activation_commands();
    for (index, command) in commands.iter().enumerate() {
        let retained = storage
            .configuration
            .activation_for_request(&command.idempotency_key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retained.command, *command);
        assert_eq!(retained.revision.get(), index as u64 + 1);
        assert_eq!(
            storage
                .configuration
                .get_activation(&command.id, retained.revision)
                .await
                .unwrap(),
            Some(retained.clone())
        );
        if check_retries {
            assert_eq!(
                storage.configuration.activate(command).await.unwrap(),
                retained
            );
        }
    }
    let active = storage
        .configuration
        .active(&commands[0].id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.activation.command, commands[2]);
    assert_eq!(active.activation.revision.get(), 3);
    assert_eq!(active.staged, staged_configurations(&seed)[0]);
    assert_eq!(
        storage
            .configuration
            .history(&commands[0].id, &Pagination::new(0, 1000))
            .await
            .unwrap()
            .total,
        3
    );
    let first = initial(&seed);
    let receipt = storage
        .bonds
        .get_commit(&first.idempotency_key)
        .await
        .unwrap()
        .unwrap();
    if check_retries {
        assert_eq!(
            storage
                .bonds
                .commit(&first, UtcTimestamp::parse("2026-10-04T12:00:00Z").unwrap())
                .await
                .unwrap(),
            receipt
        );
    }
    for record in &receipt.master_records {
        assert_eq!(
            storage
                .bonds
                .get_master_version(&record.definition.key(), record.version)
                .await
                .unwrap(),
            Some(record.clone())
        );
    }
    assert_eq!(
        storage
            .bonds
            .get_version(&seed.reference_bond.instrument_id, Version::new(1).unwrap())
            .await
            .unwrap(),
        Some(receipt.records[0].clone())
    );
    let current = storage
        .bonds
        .get(&seed.reference_bond.instrument_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.version.get(), 2);
    assert_eq!(
        current.bond.unwrap().description,
        seed.reference_revision_description
    );
    let mut reused = first.clone();
    if let ReferenceChange::Create { bond } = &mut reused.changes[0] {
        bond.description = "changed key content".into();
    }
    if check_retries {
        assert!(matches!(
            storage.bonds.commit(&reused, seed.recorded_at).await,
            Err(ReferenceError::Conflict(_))
        ));
    }
}

#[test]
fn process_fixture_child() {
    let Ok(path) = std::env::var("CVX004_TEST_DATABASE") else {
        return;
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    match std::env::var("CVX004_TEST_ACTION").unwrap().as_str() {
        "commit-and-exit" => runtime.block_on(populate(Path::new(&path))),
        "verify" => runtime.block_on(verify(Path::new(&path))),
        other => panic!("unexpected fixture operation {other}"),
    }
}

#[cfg(unix)]
fn child(path: &Path, action: &str) -> std::process::Output {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "process_fixture_child", "--nocapture"])
        .env("CVX004_TEST_DATABASE", path)
        .env("CVX004_TEST_ACTION", action)
        .output()
        .unwrap()
}
#[cfg(unix)]
fn verify_child(path: &Path) {
    let output = child(path, "verify");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg(unix)]
fn unclean_process_restart_and_offline_restore_preserve_all_committed_records() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("original.redb");
    assert_eq!(child(&path, "commit-and-exit").status.code(), Some(17));
    verify_child(&path);
    let backup = directory.path().join("backup");
    let manifest = backup_database(&path, &backup).unwrap();
    assert_eq!(manifest.application_schema_version, 1);
    assert_eq!(manifest.sha256.len(), 64);
    let restored = directory.path().join("restored.redb");
    assert_eq!(restore_database(&backup, &restored).unwrap(), manifest);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&restored).unwrap().permissions().mode() & 0o777,
        0o600
    );
    verify_child(&restored);
    verify_child(&path);
    assert_eq!(
        std::fs::read_to_string(backup.join("manifest.json"))
            .unwrap()
            .trim(),
        serde_json::to_string_pretty(&manifest).unwrap()
    );
}

#[test]
#[cfg(unix)]
fn cli_backup_restore_proves_operator_path() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.redb");
    assert_eq!(child(&path, "commit-and-exit").status.code(), Some(17));
    verify_child(&path);
    let backup = directory.path().join("cli-backup");
    let target = directory.path().join("cli-restore.redb");
    for args in [
        vec!["backup".as_ref(), path.as_os_str(), backup.as_os_str()],
        vec!["restore".as_ref(), backup.as_os_str(), target.as_os_str()],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_convex-redb"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    verify_child(&target);
}

#[test]
#[cfg(unix)]
fn backup_refuses_live_database_and_restore_never_clobbers_existing_state() {
    let directory = tempfile::tempdir().unwrap();
    let empty = directory.path().join("empty.redb");
    let empty_backup = directory.path().join("empty-backup");
    std::fs::write(&empty, []).unwrap();
    assert!(matches!(
        backup_database(&empty, &empty_backup),
        Err(TraitError::CapacityExceeded)
    ));
    assert!(!empty_backup.exists());
    let path = directory.path().join("source.redb");
    let storage = create_redb_storage(&path).unwrap();
    let backup = directory.path().join("backup");
    assert!(backup_database(&path, &backup).is_err());
    assert!(!backup.exists());
    drop(storage);
    backup_database(&path, &backup).unwrap();
    let sentinel = b"existing financial state";
    let existing = directory.path().join("existing.redb");
    std::fs::write(&existing, sentinel).unwrap();
    assert!(matches!(
        restore_database(&backup, &existing),
        Err(TraitError::Conflict(_))
    ));
    assert_eq!(std::fs::read(&existing).unwrap(), sentinel);
    assert!(backup_database(&path, &backup).is_err());
}

#[test]
#[cfg(unix)]
fn corrupt_truncated_incomplete_and_newer_backups_are_rejected_before_publication() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.redb");
    drop(create_redb_storage(&source).unwrap());
    let backup = directory.path().join("backup");
    let manifest = backup_database(&source, &backup).unwrap();
    let original_data = std::fs::read(backup.join("data.redb")).unwrap();
    let original_manifest = std::fs::read(backup.join("manifest.json")).unwrap();
    let target = directory.path().join("invalid.redb");
    let mut corrupt = original_data.clone();
    let middle = corrupt.len() / 2;
    corrupt[middle] ^= 1;
    std::fs::write(backup.join("data.redb"), corrupt).unwrap();
    assert!(matches!(
        restore_database(&backup, &target),
        Err(TraitError::InvalidInput("backup checksum mismatch"))
    ));
    assert!(!target.exists());
    std::fs::write(
        backup.join("data.redb"),
        &original_data[..original_data.len() / 2],
    )
    .unwrap();
    assert!(restore_database(&backup, &target).is_err());
    assert!(!target.exists());
    std::fs::write(backup.join("data.redb"), &original_data).unwrap();
    let mut newer = manifest.clone();
    newer.application_schema_version = 2;
    std::fs::write(
        backup.join("manifest.json"),
        serde_json::to_vec(&newer).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        restore_database(&backup, &target),
        Err(TraitError::Unsupported(_))
    ));
    assert!(!target.exists());
    let mut inconsistent = manifest;
    inconsistent.application_schema_version = 0;
    std::fs::write(
        backup.join("manifest.json"),
        serde_json::to_vec(&inconsistent).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        restore_database(&backup, &target),
        Err(TraitError::InvalidInput(
            "backup schema differs from manifest"
        ))
    ));
    assert!(!target.exists());
    std::fs::remove_file(backup.join("manifest.json")).unwrap();
    assert!(restore_database(&backup, &target).is_err());
    assert!(!target.exists());
    std::fs::write(backup.join("manifest.json"), &original_manifest).unwrap();
    let mut oversized =
        serde_json::from_slice::<convex_ext_redb::BackupManifest>(&original_manifest).unwrap();
    oversized.bytes = 64 * 1024 * 1024 * 1024 + 1;
    std::fs::write(
        backup.join("manifest.json"),
        serde_json::to_vec(&oversized).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        restore_database(&backup, &target),
        Err(TraitError::CapacityExceeded)
    ));
    assert!(!target.exists());
    std::fs::write(backup.join("manifest.json"), vec![b' '; 8193]).unwrap();
    assert!(matches!(
        restore_database(&backup, &target),
        Err(TraitError::CapacityExceeded)
    ));
    assert!(!target.exists());
    std::fs::write(backup.join("manifest.json"), &original_manifest).unwrap();
    restore_database(&backup, &target).unwrap();
    drop(create_redb_storage(&target).unwrap());
}
