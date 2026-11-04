//! Existing reference CAS, complete issuer snapshots and saved-input replay.
#![cfg(unix)]
#[path = "support/core_fixture.rs"]
mod fixture;
use convex_core::contracts::*;
use convex_engine::ports::{reference_master::*, reference_repository::*};
use fixture::*;
use serde_json::json;

async fn command(f: &Fixture) -> ReferenceCommand {
    let record = f
        .storage
        .bonds
        .get_master_version(
            &MasterKey::Calculation(f.plan.definition.id.clone()),
            f.plan.definition.version,
        )
        .await
        .unwrap()
        .unwrap();
    let MasterDefinition::Calculation(mut definition) = record.definition else {
        panic!("calculation");
    };
    definition.reference_inputs["issuer_scope"] = json!({"id":"test.issuers","version":1});
    let instrument = serde_json::to_value(&f.request).unwrap()["operation"]["input"]["identity"]
        ["instrument"]
        .clone();
    let entity: MasterDefinition = serde_json::from_value(json!({"kind":"entity","definition":{
        "entity_id":"ACME","legal_name":"ACME synthetic legal issuer","parent":null,"country":"GB",
        "effective":definition.effective,"provenance":definition.provenance}}))
    .unwrap();
    let scope: MasterDefinition=serde_json::from_value(json!({"kind":"issuer_scope","definition":{
        "id":"test.issuers","directory":{"issuers":[{"entity_id":"ACME","version":1,"default":null}],
            "instruments":[{"id":instrument["id"],"version":instrument["version"],"entity_id":"ACME"}]},
        "effective":definition.effective,"provenance":definition.provenance}})).unwrap();
    ReferenceCommand {
        idempotency_key: RequestId::new("test.issuer.initial").unwrap(),
        changes: vec![],
        master_changes: vec![
            MasterChange {
                expected_version: Some(Version::new(1).unwrap()),
                definition: MasterDefinition::Calculation(definition),
            },
            MasterChange {
                expected_version: None,
                definition: entity,
            },
            MasterChange {
                expected_version: None,
                definition: scope,
            },
        ],
    }
}

#[tokio::test]
async fn directory_and_book_are_atomic_and_saved_inputs_survive_new_notices() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = Fixture::new(dir.path()).await;
    let initial = command(&f).await;
    let receipt = f.storage.bonds.commit(&initial, time()).await.unwrap();
    assert_eq!(
        receipt,
        f.storage.bonds.commit(&initial, time()).await.unwrap()
    );
    f.plan.definition.version = Version::new(2).unwrap();
    let core = f.workflow();
    let saved = core.assemble(f.plan.clone()).await.unwrap();
    assert_eq!(
        saved.content.issuer_scope.as_ref().unwrap().version,
        Version::new(1).unwrap()
    );
    let row = core
        .run(saved.id.clone(), "risk".into(), false)
        .await
        .unwrap();
    let bytes = core.result(&row.result_id).await.unwrap().output;
    let mut next = initial.master_changes[2].clone();
    next.expected_version = Some(Version::new(1).unwrap());
    let MasterDefinition::IssuerScope(scope) = &mut next.definition else {
        panic!("scope")
    };
    scope.directory.issuers[0].default = Some(
        serde_json::from_value(json!({"id":"actual.default","effective_date":"2026-10-07",
        "known_at":"2026-10-07T12:00:00Z","reference_id":"actual.source","reference_version":1}))
        .unwrap(),
    );
    let update = ReferenceCommand {
        idempotency_key: RequestId::new("test.issuer.notice").unwrap(),
        changes: vec![],
        master_changes: vec![next],
    };
    f.storage.bonds.commit(&update, time()).await.unwrap();
    // Current directory2 blocks an obsolete book pin; replay uses captured directory1.
    assert!(core.assemble(f.plan.clone()).await.is_err());
    let replay = core.run(saved.id, "risk".into(), true).await.unwrap();
    assert_eq!(core.result(&replay.result_id).await.unwrap().output, bytes);
}

#[tokio::test]
async fn missing_entity_and_scope_cas_conflicts_leave_no_partial_book() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path()).await;
    let mut invalid = command(&f).await;
    invalid.master_changes.remove(1);
    assert!(f.storage.bonds.commit(&invalid, time()).await.is_err());
    assert!(f
        .storage
        .bonds
        .get_master_version(
            &MasterKey::Calculation(f.plan.definition.id.clone()),
            Version::new(2).unwrap()
        )
        .await
        .unwrap()
        .is_none());
    let initial = command(&f).await;
    f.storage.bonds.commit(&initial, time()).await.unwrap();
    let mut stale = initial;
    stale.idempotency_key = RequestId::new("competing.issuer").unwrap();
    assert!(f.storage.bonds.commit(&stale, time()).await.is_err());
    assert!(f
        .storage
        .bonds
        .get_master_version(
            &MasterKey::Calculation(f.plan.definition.id.clone()),
            Version::new(3).unwrap()
        )
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn issuer_directory_cannot_outlive_its_pinned_entity_attributes() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path()).await;
    let mut initial = command(&f).await;
    let until = UtcTimestamp::parse("2026-10-08T12:00:00Z").unwrap();
    let MasterDefinition::Entity(entity) = &mut initial.master_changes[1].definition else {
        panic!("entity");
    };
    entity.effective.until = Some(until);
    assert!(f.storage.bonds.commit(&initial, time()).await.is_err());
    let MasterDefinition::IssuerScope(scope) = &mut initial.master_changes[2].definition else {
        panic!("scope");
    };
    scope.effective.until = Some(until);
    f.storage.bonds.commit(&initial, time()).await.unwrap();
}

#[tokio::test]
async fn native_default_coverage_and_exact_security_versions_are_required() {
    for mode in ["missing_mapping", "wrong_version", "unbound_default"] {
        let dir = tempfile::tempdir().unwrap();
        let mut f = Fixture::new(dir.path()).await;
        let mut initial = command(&f).await;
        let MasterDefinition::IssuerScope(scope) = &mut initial.master_changes[2].definition else {
            panic!("scope")
        };
        if mode == "missing_mapping" {
            scope.directory.instruments[0].id = ReferenceDataId::new("other.security").unwrap();
        }
        if mode == "wrong_version" {
            scope.directory.instruments[0].version = Version::new(2).unwrap();
        }
        if mode == "unbound_default" {
            scope.directory.issuers[0].default=Some(serde_json::from_value(json!({"id":"actual.default",
            "effective_date":"2026-10-07","known_at":"2026-10-07T12:00:00Z","reference_id":"actual.source","reference_version":1})).unwrap());
        }
        f.storage.bonds.commit(&initial, time()).await.unwrap();
        f.plan.definition.version = Version::new(2).unwrap();
        assert!(f.workflow().assemble(f.plan).await.is_err(), "{mode}");
    }
}

#[tokio::test]
async fn future_known_notices_and_clearing_or_reassigning_retained_facts_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let f = Fixture::new(dir.path()).await;
    let mut initial = command(&f).await;
    let MasterDefinition::IssuerScope(scope) = &mut initial.master_changes[2].definition else {
        panic!("scope")
    };
    scope.directory.issuers[0].default = Some(
        serde_json::from_value(json!({"id":"actual.default","effective_date":"2026-10-07",
        "known_at":"2026-10-07T12:00:01Z","reference_id":"actual.source","reference_version":1}))
        .unwrap(),
    );
    assert!(f.storage.bonds.commit(&initial, time()).await.is_err());
    let MasterDefinition::IssuerScope(scope) = &mut initial.master_changes[2].definition else {
        panic!("scope")
    };
    scope.directory.issuers[0]
        .default
        .as_mut()
        .unwrap()
        .known_at = time();
    f.storage.bonds.commit(&initial, time()).await.unwrap();
    for mode in ["clear", "rewrite", "remove"] {
        let mut change = initial.master_changes[2].clone();
        change.expected_version = Some(Version::new(1).unwrap());
        let MasterDefinition::IssuerScope(scope) = &mut change.definition else {
            panic!("scope")
        };
        if mode == "clear" {
            scope.directory.issuers[0].default = None;
        }
        if mode == "rewrite" {
            scope.directory.issuers[0].default.as_mut().unwrap().id =
                ReferenceDataId::new("replacement").unwrap();
        }
        if mode == "remove" {
            scope.directory.instruments.clear();
        }
        let next = ReferenceCommand {
            idempotency_key: RequestId::new(format!("invalid.{mode}")).unwrap(),
            changes: vec![],
            master_changes: vec![change],
        };
        assert!(
            f.storage.bonds.commit(&next, time()).await.is_err(),
            "{mode}"
        );
    }
}
