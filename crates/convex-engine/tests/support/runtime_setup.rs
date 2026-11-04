#![allow(dead_code)] // Different lifecycle suites exercise subsets of the shared fictional setup.
#[path = "../../../convex-ports/tests/support/configuration_fixtures.rs"]
pub mod configuration_fixture;
pub use configuration_fixture::snapshot as fixture;
use convex_core::contracts::*;
use convex_engine::{context::*, snapshot::*, *};
use convex_ports::{market_data::*, reference_repository::*, runtime::*, storage::StorageAdapter};
use std::sync::Arc;

pub struct Setup {
    pub dir: tempfile::TempDir,
    pub storage: Arc<StorageAdapter>,
    pub store: Arc<convex_ext_redb::RedbRuntimeStore>,
    pub snapshots: Arc<convex_ext_redb::RedbSnapshotRepository>,
    pub results: Arc<convex_ext_redb::RedbResultPublisher>,
    pub fixings: Arc<convex_ext_redb::RedbFixingRepository>,
    pub plan: RuntimePlan,
}
pub async fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let convex_ext_redb::RedbStorage {
        storage,
        runtime: store,
        snapshots,
        results,
        fixings,
    } = convex_ext_redb::open_redb_storage(dir.path().join("inputs.redb")).unwrap();
    let bond = fixture::bond();
    storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("runtime-seed").unwrap(),
                changes: vec![ReferenceChange::Create {
                    bond: Box::new(bond.clone()),
                }],
                master_changes: fixture::reference_terms::resources(&bond),
            },
            fixture::time("2025-01-01T00:00:00Z"),
        )
        .await
        .unwrap();
    storage.configs.save(&fixture::config()).await.unwrap();
    let content = fixture::content();
    let plan = RuntimePlan {
        schema_version: 1,
        runtime_id: SourceId::new("runtime-test").unwrap(),
        mode: RuntimeMode::Captured,
        assembly: SnapshotAssembly {
            as_of: content.as_of,
            known_at: content.known_at,
            generation: fixture::version(1),
            configuration: ConfigurationPin {
                id: SourceId::new(content.configuration.config_id).unwrap(),
                version: fixture::version(1),
            },
            configuration_capture: None,
            instruments: vec![InstrumentPin {
                instrument_id: InstrumentId::new(bond.instrument_id.as_str()).unwrap(),
                version: fixture::version(1),
            }],
            quotes: vec![fixture::quote_requirement()],
            fixings: vec![],
            required_curves: content.required_curves,
            curves: content.curves,
            curve_recipes: vec![],
            calibration: None,
            max_source_skew_ms: 1000,
        },
        seed_events: vec![content.quotes[0].delivery_watermark.clone().unwrap()],
        settlement_date: BusinessDate::parse("2026-10-06").unwrap(),
        build_id: ModelId::new("runtime-fixture-build").unwrap(),
        measures: [
            "clean_price_mid",
            "dirty_price_mid",
            "accrued_interest",
            "ytm_mid",
        ]
        .into_iter()
        .map(|id| MeasureId::new(id).unwrap())
        .collect(),
        numerical_settings: NumericalSettings {
            policy_id: ModelId::new(BOND_NUMERICAL_POLICY).unwrap(),
            bump_bps: FiniteF64::new(10.0).unwrap(),
        },
    };
    Setup {
        dir,
        storage: Arc::new(storage),
        store,
        snapshots,
        results,
        fixings,
        plan,
    }
}
pub fn make_engine(
    s: &Setup,
    publisher: Arc<dyn ResultPublisher>,
    plan: RuntimePlan,
    store: Arc<dyn RuntimeStore>,
    input: RuntimeInput,
) -> Arc<PricingEngine> {
    Arc::new(
        PricingEngineBuilder::new()
            .with_storage(s.storage.clone())
            .with_runtime(RuntimeServices {
                plan,
                snapshots: s.snapshots.clone(),
                fixings: s.fixings.clone(),
                store,
                publisher,
                input,
            })
            .build()
            .unwrap(),
    )
}
/// Widen `plan` to `width` bonds: copies of the fixture bond under new identities
/// (`BOND-W001`...), stored in `s`, each quoted under its own key.
pub async fn widen(s: &Setup, plan: &mut RuntimePlan, width: usize) {
    let copies: Vec<_> = (1..width)
        .map(|index| {
            let mut bond = fixture::bond();
            bond.instrument_id = convex_core::ids::InstrumentId::new(format!("BOND-W{index:03}"));
            bond.isin = Some(format!("fixture-isin-w{index:03}"));
            bond.cusip = Some(format!("fixture-cusip-w{index:03}"));
            bond
        })
        .collect();
    if copies.is_empty() {
        return;
    }
    s.storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("wide-seed").unwrap(),
                changes: copies
                    .iter()
                    .map(|bond| ReferenceChange::Create {
                        bond: Box::new(bond.clone()),
                    })
                    .collect(),
                master_changes: vec![],
            },
            fixture::time("2025-01-01T00:00:00Z"),
        )
        .await
        .unwrap();
    for bond in &copies {
        plan.assembly.instruments.push(InstrumentPin {
            instrument_id: InstrumentId::new(bond.instrument_id.as_str()).unwrap(),
            version: fixture::version(1),
        });
        let mut quote = plan.assembly.quotes[0].clone();
        quote.key.instrument_id = bond.instrument_id.clone();
        plan.assembly.quotes.push(quote);
    }
}
pub fn archive(s: &Setup) -> Arc<dyn ResultPublisher> {
    s.results.clone()
}
pub fn event(sequence: u64, milliseconds: i64, value: &str) -> QuoteEvent {
    let start = fixture::time("2026-10-02T15:00:00Z")
        .datetime()
        .timestamp_millis();
    fixture::event(sequence, start + milliseconds, start + milliseconds, value)
}
pub fn time(milliseconds: i64) -> UtcTimestamp {
    UtcTimestamp::from_datetime(
        chrono::DateTime::from_timestamp_millis(
            fixture::time("2026-10-02T15:00:00Z")
                .datetime()
                .timestamp_millis()
                + milliseconds,
        )
        .unwrap(),
    )
    .unwrap()
}
