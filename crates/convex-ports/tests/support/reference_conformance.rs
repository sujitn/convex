//! Backend-independent executable reference transaction contract.

use convex_ports::reference_master::ExternalIdentifier;

use std::sync::Arc;

use convex_core::contracts::{ExactDecimal, RequestId, UtcTimestamp, Version};
use convex_core::ids::InstrumentId;
use convex_core::Currency;
use convex_ports::reference_data::BondReferenceData;
use convex_ports::reference_repository::*;
use convex_ports::storage::{BondFilter, Pagination};

#[path = "reference_terms.rs"]
pub mod reference_terms;

async fn seed(repository: &Arc<dyn ReferenceRepository>) {
    let initial = bond("CONTEXT");
    let mut resources = reference_terms::resources(&initial);
    let mut other = initial.clone();
    other.issuer_id = "OTHER".into();
    other.contractual_terms.as_mut().unwrap().issuer.entity_id =
        convex_core::contracts::EntityId::new("OTHER").unwrap();
    resources.push(reference_terms::resources(&other).remove(0));
    repository
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("fixture-context").unwrap(),
                changes: vec![],
                master_changes: resources,
            },
            UtcTimestamp::parse("2000-01-01T00:00:00Z").unwrap(),
        )
        .await
        .unwrap();
}

pub fn bond(id: &str) -> BondReferenceData {
    let mut bond: BondReferenceData =
        serde_json::from_str(include_str!("../fixtures/reference_bond.json")).unwrap();
    bond.instrument_id = InstrumentId::new(id);
    bond
}
pub fn time() -> UtcTimestamp {
    UtcTimestamp::parse("2026-10-03T12:00:00.123456789Z").unwrap()
}
pub fn version(value: u64) -> Version {
    Version::new(value).unwrap()
}
pub fn command(key: &str, changes: Vec<ReferenceChange>) -> ReferenceCommand {
    ReferenceCommand {
        master_changes: Vec::new(),
        idempotency_key: RequestId::new(key).unwrap(),
        changes,
    }
}
pub fn create(bond: BondReferenceData) -> ReferenceChange {
    ReferenceChange::Create {
        bond: Box::new(bond),
    }
}
pub fn update(bond: BondReferenceData, expected: u64) -> ReferenceChange {
    ReferenceChange::Update {
        bond: Box::new(bond),
        expected_version: version(expected),
    }
}

pub async fn versions_history_and_mapping(repository: Arc<dyn ReferenceRepository>) {
    seed(&repository).await;
    let initial = bond("HISTORY");
    let created = repository
        .commit(
            &command("create-history", vec![create(initial.clone())]),
            time(),
        )
        .await
        .unwrap();
    assert_eq!(created.records[0].version, version(1));
    let mut revised = initial.clone();
    revised.isin = Some("remapped-isin".into());
    revised.cusip = None;
    revised.coupon_rate = Some(ExactDecimal::parse("-0.0025").unwrap().decimal());
    let updated = repository
        .commit(
            &command("update-history", vec![update(revised.clone(), 1)]),
            time(),
        )
        .await
        .unwrap();
    assert_eq!(
        repository.get(&initial.instrument_id).await.unwrap(),
        Some(updated.records[0].clone())
    );
    assert_eq!(
        repository
            .get_version(&initial.instrument_id, version(1))
            .await
            .unwrap(),
        Some(created.records[0].clone())
    );
    assert!(repository
        .get_version(&initial.instrument_id, version(3))
        .await
        .unwrap()
        .is_none());
    assert!(repository
        .resolve(&ExternalIdentifier::Isin {
            value: initial.isin.unwrap()
        })
        .await
        .unwrap()
        .is_none());
    assert!(repository
        .resolve(&ExternalIdentifier::Cusip {
            value: initial.cusip.unwrap()
        })
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        repository
            .resolve(&ExternalIdentifier::Isin {
                value: "remapped-isin".into()
            })
            .await
            .unwrap(),
        Some(updated.records[0].clone())
    );
    let deleted = repository
        .commit(
            &command(
                "delete-history",
                vec![ReferenceChange::Delete {
                    instrument_id: revised.instrument_id.clone(),
                    expected_version: version(2),
                }],
            ),
            time(),
        )
        .await
        .unwrap();
    assert_eq!(deleted.records[0].version, version(3));
    assert!(deleted.records[0].bond.is_none());
    assert_eq!(
        repository.get(&revised.instrument_id).await.unwrap(),
        Some(deleted.records[0].clone())
    );
    assert_eq!(
        repository
            .get_version(&revised.instrument_id, version(2))
            .await
            .unwrap(),
        Some(updated.records[0].clone())
    );
    assert_eq!(
        repository
            .list(&BondFilter::default(), &Pagination::new(0, 100))
            .await
            .unwrap()
            .total,
        0
    );
    assert!(repository
        .resolve(&ExternalIdentifier::Isin {
            value: "remapped-isin".into()
        })
        .await
        .unwrap()
        .is_none());
    assert!(matches!(
        repository
            .commit(&command("recreate-history", vec![create(revised)]), time())
            .await,
        Err(ReferenceError::Conflict(_))
    ));
}

pub async fn conflict_is_atomic(repository: Arc<dyn ReferenceRepository>) {
    seed(&repository).await;
    let existing = bond("EXISTING");
    repository
        .commit(&command("seed", vec![create(existing.clone())]), time())
        .await
        .unwrap();
    let new_bond = bond("NEW");
    let conflicting = command(
        "mixed",
        vec![create(new_bond.clone()), update(existing.clone(), 9)],
    );
    assert!(matches!(
        repository.commit(&conflicting, time()).await,
        Err(ReferenceError::Conflict(_))
    ));
    assert!(repository
        .get(&new_bond.instrument_id)
        .await
        .unwrap()
        .is_none());
    assert!(repository
        .get_version(&new_bond.instrument_id, version(1))
        .await
        .unwrap()
        .is_none());
    assert!(repository
        .get_commit(&conflicting.idempotency_key)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        repository
            .get(&existing.instrument_id)
            .await
            .unwrap()
            .unwrap()
            .version,
        version(1)
    );
    let corrected = command("mixed", vec![create(new_bond), update(existing, 1)]);
    let receipt = repository.commit(&corrected, time()).await.unwrap();
    assert_eq!(receipt.records.len(), 2);
    assert_eq!(receipt.records[1].version, version(2));
}

pub async fn retry_preserves_original_result(repository: Arc<dyn ReferenceRepository>) {
    seed(&repository).await;
    let original = bond("RETRY");
    let first = command("original", vec![create(original.clone())]);
    let result = repository.commit(&first, time()).await.unwrap();
    // The caller may lose/drop its response. Lookup recovers the entire committed result.
    assert_eq!(
        repository.get_commit(&first.idempotency_key).await.unwrap(),
        Some(result.clone())
    );
    let mut revised = original;
    revised.description = "corrected description".into();
    repository
        .commit(
            &command("revision", vec![update(revised.clone(), 1)]),
            time(),
        )
        .await
        .unwrap();
    let later = UtcTimestamp::parse("2026-10-04T12:00:00Z").unwrap();
    assert_eq!(repository.commit(&first, later).await.unwrap(), result);
    assert_eq!(
        repository
            .get(&revised.instrument_id)
            .await
            .unwrap()
            .unwrap()
            .version,
        version(2)
    );
    assert!(matches!(
        repository
            .commit(&command("original", vec![create(revised)]), later)
            .await,
        Err(ReferenceError::Conflict(_))
    ));
    assert_eq!(
        repository.get_commit(&first.idempotency_key).await.unwrap(),
        Some(result)
    );
}

pub async fn expected_version_race(repository: Arc<dyn ReferenceRepository>) {
    seed(&repository).await;
    let initial = bond("RACE");
    repository
        .commit(&command("race-seed", vec![create(initial.clone())]), time())
        .await
        .unwrap();
    let barrier = Arc::new(tokio::sync::Barrier::new(16));
    let mut writers = Vec::new();
    for writer in 0..16 {
        let repository = repository.clone();
        let barrier = barrier.clone();
        let mut bond = initial.clone();
        bond.description = format!("writer-{writer}");
        writers.push(tokio::spawn(async move {
            let command = command(&format!("race-{writer}"), vec![update(bond, 1)]);
            barrier.wait().await;
            (
                command.idempotency_key.clone(),
                repository.commit(&command, time()).await,
            )
        }));
    }
    let mut winners = 0;
    for writer in writers {
        let (key, result) = writer.await.unwrap();
        match result {
            Ok(receipt) => {
                winners += 1;
                assert_eq!(receipt.records[0].version, version(2));
            }
            Err(ReferenceError::Conflict(_)) => {
                assert!(repository.get_commit(&key).await.unwrap().is_none())
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
    assert_eq!(winners, 1);
    assert_eq!(
        repository
            .get(&initial.instrument_id)
            .await
            .unwrap()
            .unwrap()
            .version,
        version(2)
    );
    assert!(repository
        .get_version(&initial.instrument_id, version(3))
        .await
        .unwrap()
        .is_none());
}

pub async fn identical_retry_race(repository: Arc<dyn ReferenceRepository>) {
    seed(&repository).await;
    let mutation = command("same-key", vec![create(bond("SAME"))]);
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let repository = repository.clone();
        let mutation = mutation.clone();
        tasks.push(tokio::spawn(async move {
            repository.commit(&mutation, time()).await.unwrap()
        }));
    }
    let original = tasks.remove(0).await.unwrap();
    for task in tasks {
        assert_eq!(task.await.unwrap(), original);
    }
    assert_eq!(
        repository
            .list(&BondFilter::default(), &Pagination::new(0, 100))
            .await
            .unwrap()
            .total,
        1
    );
    assert!(repository
        .get_version(&original.records[0].instrument_id, version(2))
        .await
        .unwrap()
        .is_none());
}

pub async fn filters_and_ambiguity(repository: Arc<dyn ReferenceRepository>) {
    seed(&repository).await;
    let mut first = bond("A");
    first.description = "Alpha Technology".into();
    let mut second = bond("B");
    second.currency = Currency::EUR;
    second.issuer_id = "OTHER".into();
    second.contractual_terms.as_mut().unwrap().issuer.entity_id =
        convex_core::contracts::EntityId::new("OTHER").unwrap();
    second.sector = "Financials".into();
    let mut third = bond("C");
    third.isin = Some("unique-third".into());
    third.cusip = None;
    repository
        .commit(
            &command(
                "query-seed",
                vec![create(third), create(second.clone()), create(first.clone())],
            ),
            time(),
        )
        .await
        .unwrap();
    let page = repository
        .list(&BondFilter::default(), &Pagination::new(1, 1))
        .await
        .unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.items[0].instrument_id.as_str(), "B");
    for filter in [
        BondFilter::by_currency(Currency::EUR),
        BondFilter {
            sector: Some("Financials".into()),
            ..BondFilter::default()
        },
        BondFilter {
            issuer_id: Some("OTHER".into()),
            ..BondFilter::default()
        },
        BondFilter {
            text_search: Some("other".into()),
            ..BondFilter::default()
        },
        BondFilter::by_instrument_id(second.instrument_id.clone()),
    ] {
        let page = repository
            .list(&filter, &Pagination::new(0, 100))
            .await
            .unwrap();
        // Text searches use description/issuer name/identity/ISIN/CUSIP, not issuer ID.
        if filter.text_search.is_some() {
            assert_eq!(page.total, 0);
        } else {
            assert_eq!(page.total, 1);
            assert_eq!(page.items[0].bond.as_ref(), Some(&second));
        }
    }
    let page = repository
        .list(
            &BondFilter {
                text_search: Some("aLpHa".into()),
                ..BondFilter::default()
            },
            &Pagination::new(0, 100),
        )
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].bond.as_ref(), Some(&first));
    assert_eq!(
        repository
            .resolve(&ExternalIdentifier::Isin {
                value: first.isin.unwrap()
            })
            .await,
        Err(ReferenceError::AmbiguousIdentifier)
    );
    assert!(matches!(
        repository
            .list(&BondFilter::default(), &Pagination::new(0, 0))
            .await,
        Err(ReferenceError::InvalidInput(_))
    ));
    assert!(matches!(
        repository
            .list(&BondFilter::default(), &Pagination::new(0, 1001))
            .await,
        Err(ReferenceError::InvalidInput(_))
    ));
    assert!(repository.get(&InstrumentId::new("")).await.is_err());
}

pub async fn exact_precision_and_failed_validation(repository: Arc<dyn ReferenceRepository>) {
    seed(&repository).await;
    let mut exact = bond("EXACT");
    exact.coupon_rate = Some(
        ExactDecimal::parse("-0.1234567890123456789012345678")
            .unwrap()
            .decimal(),
    );
    exact.face_value = ExactDecimal::parse("79228162514264337593543950335")
        .unwrap()
        .decimal();
    let mutation = command("precision", vec![create(exact.clone())]);
    let receipt = repository.commit(&mutation, time()).await.unwrap();
    assert_eq!(
        repository
            .get(&exact.instrument_id)
            .await
            .unwrap()
            .unwrap()
            .bond,
        Some(exact.clone())
    );
    assert_eq!(repository.commit(&mutation, time()).await.unwrap(), receipt);
    let original_scale = bond("SCALE");
    let mut different_scale = original_scale.clone();
    different_scale.coupon_rate = Some(ExactDecimal::parse("0.053750").unwrap().decimal());
    assert_eq!(different_scale.coupon_rate, original_scale.coupon_rate);
    repository
        .commit(&command("scale", vec![create(original_scale)]), time())
        .await
        .unwrap();
    assert!(matches!(
        repository
            .commit(&command("scale", vec![create(different_scale)]), time())
            .await,
        Err(ReferenceError::Conflict(_))
    ));
    let mut invalid = bond("INVALID");
    invalid.face_value = ExactDecimal::parse("0").unwrap().decimal();
    let invalid_command = command(
        "invalid",
        vec![create(bond("VALID-PREFIX")), create(invalid.clone())],
    );
    assert!(matches!(
        repository.commit(&invalid_command, time()).await,
        Err(ReferenceError::InvalidInput(_))
    ));
    assert!(repository
        .get(&InstrumentId::new("VALID-PREFIX"))
        .await
        .unwrap()
        .is_none());
    assert!(repository
        .get_commit(&invalid_command.idempotency_key)
        .await
        .unwrap()
        .is_none());
    let duplicate = command("duplicate", vec![create(exact.clone()), create(exact)]);
    assert!(matches!(
        repository.commit(&duplicate, time()).await,
        Err(ReferenceError::InvalidInput(_))
    ));
    let oversized = command(
        "oversized",
        (0..257)
            .map(|id| create(bond(&format!("BOUNDS-{id}"))))
            .collect(),
    );
    assert!(matches!(
        repository.commit(&oversized, time()).await,
        Err(ReferenceError::InvalidInput(_))
    ));
}
