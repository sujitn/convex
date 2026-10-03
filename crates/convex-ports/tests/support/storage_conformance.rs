//! Portable configuration/audit/curve storage scenarios for local and future adapters.

use convex_core::ids::{CurveId, InstrumentId};
use convex_ports::error::TraitError;
use convex_ports::storage::*;
use std::sync::Arc;

pub fn config() -> BondPricingConfig {
    serde_json::from_str(include_str!("../fixtures/pricing_config.json")).unwrap()
}

pub async fn exact_configuration_history(store: Arc<dyn ConfigStore>) {
    let first = config();
    let mut second = first.clone();
    second.version = 2;
    second.updated_at = 200;
    second.description = "second exact content".into();
    second.active = false;
    second.pricing_spec = PricingSpec::BenchmarkSpread {
        benchmark: BenchmarkReference::ExplicitYield { yield_pct: -0.15 },
        spread_bps: -12.5,
    };
    store.save(&first).await.unwrap();
    store.save(&second).await.unwrap();
    assert_eq!(
        store.get_version(&first.config_id, 1).await.unwrap(),
        Some(first.clone())
    );
    assert_eq!(
        store.get_version(&first.config_id, 2).await.unwrap(),
        Some(second.clone())
    );
    assert_eq!(
        store.get(&first.config_id).await.unwrap(),
        Some(second.clone())
    );
    assert!(store
        .get_version(&first.config_id, 3)
        .await
        .unwrap()
        .is_none());
    assert!(store.get_version("MISSING", 1).await.unwrap().is_none());
    let versions = store.list_versions(&first.config_id).await.unwrap();
    assert_eq!(
        versions
            .iter()
            .map(|v| (v.version, v.timestamp))
            .collect::<Vec<_>>(),
        [(1, 100), (2, 200)]
    );
    assert!(versions
        .iter()
        .all(|v| v.changed_by.is_none() && v.change_description.is_none()));
    assert_eq!(store.list().await.unwrap(), [second.clone()]);
    // Retrying an old committed version must not reactivate it.
    store.save(&first).await.unwrap();
    assert_eq!(store.get(&first.config_id).await.unwrap(), Some(second));
    assert_eq!(
        store.list_versions(&first.config_id).await.unwrap(),
        versions
    );
}

pub async fn configuration_conflicts_and_validation(store: Arc<dyn ConfigStore>) {
    let first = config();
    store.save(&first).await.unwrap();
    let mut same_version = first.clone();
    same_version.description = "conflict".into();
    assert!(matches!(
        store.save(&same_version).await,
        Err(TraitError::Conflict(_))
    ));
    let mut skipped = first.clone();
    skipped.version = 3;
    assert!(matches!(
        store.save(&skipped).await,
        Err(TraitError::Conflict(_))
    ));
    let mut invalid = first.clone();
    invalid.version = 2;
    invalid.pricing_spec = PricingSpec::ZSpread {
        discount_curve: CurveId::new("USD-FIXTURE"),
        z_spread_bps: f64::NAN,
    };
    assert!(matches!(
        store.save(&invalid).await,
        Err(TraitError::InvalidInput(_))
    ));
    invalid.pricing_spec = PricingSpec::Auto;
    invalid.created_at = 101;
    assert!(matches!(
        store.save(&invalid).await,
        Err(TraitError::InvalidInput(_))
    ));
    invalid = first.clone();
    invalid.version = 0;
    assert!(matches!(
        store.save(&invalid).await,
        Err(TraitError::InvalidInput(_))
    ));
    assert!(matches!(
        store.get_version(&first.config_id, 0).await,
        Err(TraitError::InvalidInput(_))
    ));
    assert_eq!(
        store.get(&first.config_id).await.unwrap(),
        Some(first.clone())
    );
    assert_eq!(
        store.list_versions(&first.config_id).await.unwrap().len(),
        1
    );
    assert!(matches!(
        store.delete(&first.config_id).await,
        Err(TraitError::Unsupported(_))
    ));
    assert_eq!(
        store.get_version(&first.config_id, 1).await.unwrap(),
        Some(first)
    );
}

pub async fn concurrent_configuration_writers(store: Arc<dyn ConfigStore>) {
    let first = config();
    store.save(&first).await.unwrap();
    let mut tasks = Vec::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(16));
    for n in 0..16 {
        let mut candidate = first.clone();
        candidate.version = 2;
        candidate.updated_at = 200;
        candidate.description = format!("candidate {n}");
        let store = store.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            (candidate.clone(), store.save(&candidate).await)
        }));
    }
    let mut winners = Vec::new();
    for task in tasks {
        let (candidate, result) = task.await.unwrap();
        match result {
            Ok(()) => winners.push(candidate),
            Err(TraitError::Conflict(_)) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
    assert_eq!(winners.len(), 1);
    assert_eq!(
        store.get(&first.config_id).await.unwrap(),
        Some(winners[0].clone())
    );
    assert_eq!(
        store.list_versions(&first.config_id).await.unwrap().len(),
        2
    );
}

pub async fn concurrent_configuration_retries(store: Arc<dyn ConfigStore>) {
    let first = config();
    let mut tasks = Vec::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(16));
    for _ in 0..16 {
        let store = store.clone();
        let first = first.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            store.save(&first).await
        }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    assert_eq!(
        store.list_versions(&first.config_id).await.unwrap().len(),
        1
    );
    assert_eq!(store.get(&first.config_id).await.unwrap(), Some(first));
}

pub async fn configuration_decimal_precision(store: Arc<dyn ConfigStore>) {
    let mut first = config();
    let ratio = "1.1234567890123456789012345678".parse().unwrap();
    first.pricing_spec = PricingSpec::RealYield {
        real_yield_pct: -0.25,
        inflation_curve: CurveId::new("USD-INFLATION"),
        index_ratio: ratio,
    };
    store.save(&first).await.unwrap();
    let mut second = first.clone();
    second.version = 2;
    second.updated_at = 200;
    second.description = "next version".into();
    store.save(&second).await.unwrap();
    assert_eq!(
        store.get_version(&first.config_id, 1).await.unwrap(),
        Some(first.clone())
    );
    assert_eq!(store.get(&first.config_id).await.unwrap(), Some(second));
    store.save(&first).await.unwrap();
    if let PricingSpec::RealYield { index_ratio, .. } = store
        .get_version(&first.config_id, 1)
        .await
        .unwrap()
        .unwrap()
        .pricing_spec
    {
        assert_eq!(index_ratio.to_string(), "1.1234567890123456789012345678");
    } else {
        panic!("real yield content lost");
    }
}

pub fn audit(
    id: u64,
    time: i64,
    event: &str,
    entity_type: &str,
    entity: &str,
    user: Option<&str>,
) -> AuditEntry {
    AuditEntry {
        id,
        timestamp: time,
        event_type: event.into(),
        entity_type: entity_type.into(),
        entity_id: entity.into(),
        user: user.map(str::to_owned),
        action: "fixture".into(),
        details: None,
    }
}

pub async fn audit_filter_count_and_pagination(store: Arc<dyn AuditStore>) {
    // Insert out of order; timestamps deliberately differ from ID ordering.
    for entry in [
        audit(50, 100, "write", "bond", "B", None),
        audit(30, 300, "write", "bond", "A", Some("alice")),
        audit(10, 200, "write", "bond", "A", Some("alice")),
        audit(20, 150, "read", "bond", "B", Some("bob")),
        audit(40, 400, "write", "book", "A", Some("alice")),
    ] {
        store.append(&entry).await.unwrap();
    }
    let cases = [
        (AuditFilter::default(), vec![10, 20, 30, 40, 50]),
        (
            AuditFilter {
                event_type: Some("write".into()),
                ..Default::default()
            },
            vec![10, 30, 40, 50],
        ),
        (
            AuditFilter {
                entity_type: Some("bond".into()),
                ..Default::default()
            },
            vec![10, 20, 30, 50],
        ),
        (
            AuditFilter {
                entity_id: Some("A".into()),
                ..Default::default()
            },
            vec![10, 30, 40],
        ),
        (
            AuditFilter {
                user: Some("alice".into()),
                ..Default::default()
            },
            vec![10, 30, 40],
        ),
        (
            AuditFilter {
                from: Some(150),
                to: Some(300),
                ..Default::default()
            },
            vec![10, 20, 30],
        ),
        (
            AuditFilter {
                event_type: Some("write".into()),
                entity_type: Some("bond".into()),
                entity_id: Some("A".into()),
                user: Some("alice".into()),
                from: Some(200),
                to: Some(300),
            },
            vec![10, 30],
        ),
        (
            AuditFilter {
                user: Some("missing".into()),
                ..Default::default()
            },
            vec![],
        ),
    ];
    for (filter, expected) in cases {
        let full = store
            .query(&filter, &Pagination::new(0, 1000))
            .await
            .unwrap();
        assert_eq!(
            full.items.iter().map(|v| v.id).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(full.total, expected.len() as u64);
        let page = store.query(&filter, &Pagination::new(1, 1)).await.unwrap();
        assert_eq!(
            page.items.iter().map(|v| v.id).collect::<Vec<_>>(),
            expected.iter().skip(1).take(1).copied().collect::<Vec<_>>()
        );
        assert_eq!(page.total, full.total);
        let count = store.query(&filter, &Pagination::new(0, 0)).await.unwrap();
        assert!(count.items.is_empty());
        assert_eq!(count.total, full.total);
    }
    let beyond = store
        .query(&AuditFilter::default(), &Pagination::new(usize::MAX, 1))
        .await
        .unwrap();
    assert!(beyond.items.is_empty());
    assert!(!beyond.has_more());
    assert_eq!(beyond.total, 5);
    assert!(matches!(
        store
            .query(&AuditFilter::default(), &Pagination::new(0, 1001))
            .await,
        Err(TraitError::InvalidInput(_))
    ));
    assert!(matches!(
        store
            .query(
                &AuditFilter {
                    from: Some(300),
                    to: Some(100),
                    ..Default::default()
                },
                &Pagination::default()
            )
            .await,
        Err(TraitError::InvalidInput(_))
    ));
}

pub async fn audit_is_append_only(store: Arc<dyn AuditStore>) {
    let first = audit(7, 100, "fixture", "bond", "A", Some("alice"));
    store.append(&first).await.unwrap();
    store.append(&first).await.unwrap();
    let mut changed = first.clone();
    changed.user = Some("bob".into());
    assert!(matches!(
        store.append(&changed).await,
        Err(TraitError::Conflict(_))
    ));
    let page = store
        .query(&AuditFilter::default(), &Pagination::new(0, 10))
        .await
        .unwrap();
    assert_eq!(page.items, [first]);
    assert_eq!(page.total, 1);
}

pub async fn curve_snapshots_are_immutable_and_ordered(store: Arc<dyn CurveStore>) {
    let id = CurveId::new("USD");
    let snapshot = |as_of| CurveSnapshot {
        curve_id: id.clone(),
        as_of,
        points: vec![(365, -0.0025), (730, 0.012)],
        inputs_hash: format!("fixture-{as_of}"),
        build_duration_ms: 1,
    };
    for time in [300, 100, 200] {
        store.save_snapshot(&snapshot(time)).await.unwrap();
    }
    store.save_snapshot(&snapshot(100)).await.unwrap();
    let mut changed = snapshot(100);
    changed.points[0].1 = 0.0025;
    assert!(matches!(
        store.save_snapshot(&changed).await,
        Err(TraitError::Conflict(_))
    ));
    let mut other = snapshot(999);
    other.curve_id = CurveId::new("USD-OTHER");
    store.save_snapshot(&other).await.unwrap();
    assert_eq!(
        store.get_latest_snapshot(&id).await.unwrap(),
        Some(snapshot(300))
    );
    assert_eq!(
        store.list_snapshots(&id, 100, 200).await.unwrap(),
        [snapshot(100), snapshot(200)]
    );
    assert!(matches!(
        store.list_snapshots(&id, 200, 100).await,
        Err(TraitError::InvalidInput(_))
    ));
    assert_eq!(store.delete_snapshots_before(&id, 200).await.unwrap(), 1);
    assert_eq!(
        store.get_snapshot(&id, 200).await.unwrap(),
        Some(snapshot(200))
    );
    assert!(store.get_snapshot(&id, 100).await.unwrap().is_none());
    assert_eq!(
        store.delete_snapshots_before(&id, i64::MIN).await.unwrap(),
        0
    );
    assert_eq!(
        store.get_latest_snapshot(&other.curve_id).await.unwrap(),
        Some(other)
    );
    let mut invalid = snapshot(400);
    invalid.points[0].1 = f64::INFINITY;
    assert!(matches!(
        store.save_snapshot(&invalid).await,
        Err(TraitError::InvalidInput(_))
    ));
    invalid = snapshot(400);
    invalid.points.swap(0, 1);
    assert!(matches!(
        store.save_snapshot(&invalid).await,
        Err(TraitError::InvalidInput(_))
    ));
    assert!(store.get_snapshot(&id, 400).await.unwrap().is_none());
}

pub async fn override_history_is_explicitly_unsupported(store: Arc<dyn OverrideStore>) {
    assert!(matches!(
        store.get_history(&InstrumentId::new("A")).await,
        Err(TraitError::Unsupported(_))
    ));
}
