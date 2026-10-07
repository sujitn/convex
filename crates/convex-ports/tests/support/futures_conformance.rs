#![allow(dead_code)] // Individual port/adapter suites execute different contract cases.
use convex_core::contracts::{RequestId, UtcTimestamp, Version};
use convex_ports::reference_master::*;
use convex_ports::reference_repository::*;

pub const INITIAL: &str = include_str!("../../../../tests/fixtures/futures/reference-import.json");
pub fn t(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse(s).unwrap()
}
pub fn first_time() -> UtcTimestamp {
    t("2026-10-07T06:00:00Z")
}
pub fn future_key() -> MasterKey {
    MasterKey::Futures("CME-ZNZ6".to_string().try_into().unwrap())
}
pub fn basket_key() -> MasterKey {
    MasterKey::Basket("CME-ZNZ6-basket".to_string().try_into().unwrap())
}
pub fn command() -> ReferenceCommand {
    let v: serde_json::Value = serde_json::from_str(INITIAL).unwrap();
    serde_json::from_value(v["command"].clone()).unwrap()
}
pub fn correction() -> ReferenceCommand {
    let mut initial = command();
    initial.idempotency_key = RequestId::new("reference-correction-control").unwrap();
    initial.changes.clear();
    initial.master_changes.retain(|c| {
        matches!(
            c.definition,
            MasterDefinition::Futures(_) | MasterDefinition::Basket(_)
        ) && c.definition.key() != MasterKey::Futures("CME-SR3U6".to_string().try_into().unwrap())
    });
    for c in &mut initial.master_changes {
        c.expected_version = Some(Version::new(1).unwrap());
        match &mut c.definition {
            MasterDefinition::Futures(v) => {
                v.contract.stamp.version = Version::new(2).unwrap();
                if let convex_core::futures::FuturesSettlement::Bond { delivery, .. } =
                    &mut v.contract.settlement
                {
                    delivery.basket.version = Version::new(2).unwrap();
                }
            }
            MasterDefinition::Basket(v) => {
                v.basket.stamp.version = Version::new(2).unwrap();
                v.basket.contract.version = Version::new(2).unwrap();
                // Controlled operator correction, not a new published CME market table.
                v.basket.bonds.reverse();
                v.basket.complete = false;
            }
            _ => unreachable!(),
        }
    }
    initial
}

pub async fn exact_history_and_known_at_replay(repo: &dyn ReferenceRepository) {
    let original = repo.commit(&command(), first_time()).await.unwrap();
    let future = original
        .master_records
        .iter()
        .find(|r| r.definition.key() == future_key())
        .unwrap()
        .clone();
    let basket = original
        .master_records
        .iter()
        .find(|r| r.definition.key() == basket_key())
        .unwrap()
        .clone();
    let revised_at = t("2026-10-07T07:00:00Z");
    let updated = repo.commit(&correction(), revised_at).await.unwrap();
    assert_eq!(updated.master_records.len(), 2);
    let lookup = ReferenceLookup {
        as_of: t("2026-10-07T08:00:00Z"),
        known_at: first_time(),
    };
    assert_eq!(
        repo.get_master_at(&future_key(), lookup).await.unwrap(),
        Some(future.clone())
    );
    assert_eq!(
        repo.get_master_at(&basket_key(), lookup).await.unwrap(),
        Some(basket.clone())
    );
    let latest = ReferenceLookup {
        known_at: revised_at,
        ..lookup
    };
    for record in &updated.master_records {
        assert_eq!(record.version.get(), 2);
        assert_eq!(
            repo.get_master_at(&record.definition.key(), latest)
                .await
                .unwrap(),
            Some(record.clone())
        );
    }
    assert_eq!(
        repo.get_master_version(&basket_key(), Version::new(1).unwrap())
            .await
            .unwrap(),
        Some(basket)
    );
    assert_eq!(repo.commit(&command(), revised_at).await.unwrap(), original);
    let symbol = ExternalIdentifier::Provider {
        namespace: "CME".to_string().try_into().unwrap(),
        value: "ZNZ6".into(),
    };
    let before = repo.resolve_at(&symbol, lookup).await.unwrap().unwrap();
    let after = repo.resolve_at(&symbol, latest).await.unwrap().unwrap();
    assert_eq!(before.target, after.target);
    assert_eq!(before.target_version.get(), 1);
    assert_eq!(after.target_version.get(), 2);
    assert!(repo
        .get_master_at(
            &future_key(),
            ReferenceLookup {
                known_at: t("2026-10-07T05:59:59Z"),
                ..lookup
            }
        )
        .await
        .unwrap()
        .is_none());
}

pub async fn missing_pins_and_nonatomic_corrections_are_rejected(repo: &dyn ReferenceRepository) {
    repo.commit(&command(), first_time()).await.unwrap();
    for n in 0..5 {
        let mut bad = correction();
        bad.idempotency_key = RequestId::new(format!("bad-correction-{n}")).unwrap();
        match n {
            0 => {
                if let MasterDefinition::Basket(v) = &mut bad.master_changes[1].definition {
                    v.basket.bonds[0].instrument.version = Version::new(99).unwrap();
                }
            }
            1 => {
                if let MasterDefinition::Basket(v) = &mut bad.master_changes[1].definition {
                    v.basket.contract.version = Version::new(99).unwrap();
                }
            }
            2 => {
                if let MasterDefinition::Futures(v) = &mut bad.master_changes[0].definition {
                    v.contract.stamp.version = Version::new(99).unwrap();
                }
            }
            3 => {
                bad.master_changes.remove(1);
            }
            _ => {
                if let MasterDefinition::Basket(v) = &mut bad.master_changes[1].definition {
                    v.basket.currency = convex_core::Currency::GBP;
                }
            }
        }
        assert!(
            repo.commit(&bad, t("2026-10-07T07:00:00Z")).await.is_err(),
            "case{n}"
        );
        for key in [future_key(), basket_key()] {
            assert_eq!(
                repo.get_master(&key).await.unwrap().unwrap().version.get(),
                1
            );
            assert!(repo
                .get_master_version(&key, Version::new(2).unwrap())
                .await
                .unwrap()
                .is_none());
        }
        assert!(repo
            .get_commit(&bad.idempotency_key)
            .await
            .unwrap()
            .is_none());
    }
}
