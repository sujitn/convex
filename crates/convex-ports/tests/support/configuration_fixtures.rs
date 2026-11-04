#![allow(dead_code)]
#[path = "snapshot_fixtures.rs"]
pub mod snapshot;
use convex_core::contracts::*;
use convex_ports::configuration::*;

pub fn document(version: u64) -> ConfigurationDocument {
    let content = snapshot::content();
    let mut pricing = content.configuration;
    pricing.version = version;
    pricing.updated_at = version as i64 * 100;
    ConfigurationDocument {
        schema_version: 1,
        process: ProcessRequirements {
            storage: SourceId::new("redb-local").unwrap(),
            result_transport: SourceId::new("jsonl-local").unwrap(),
            max_concurrent_calcs: 4,
            shared_writers: false,
        },
        providers: vec![ProviderBinding {
            source: SourceId::new("feed-a").unwrap(),
            capabilities: vec![ProviderCapability::CanonicalQuotes],
            credential: None,
        }],
        runtime: RuntimeSettings {
            runtime_id: SourceId::new("runtime-test").unwrap(),
            mode: RuntimeMode::Captured,
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
                policy_id: ModelId::new("convex-bond-router-v1").unwrap(),
                bump_bps: FiniteF64::new(10.0).unwrap(),
            },
        },
        assembly: SnapshotAssembly {
            as_of: content.as_of,
            known_at: content.known_at,
            generation: snapshot::version(1),
            configuration: ConfigurationPin {
                id: SourceId::new(&pricing.config_id).unwrap(),
                version: snapshot::version(version),
            },
            configuration_capture: None,
            instruments: vec![InstrumentPin {
                instrument_id: InstrumentId::new("BOND-001").unwrap(),
                version: snapshot::version(1),
            }],
            quotes: vec![snapshot::quote_requirement()],
            fixings: vec![],
            required_curves: content.required_curves,
            curves: content.curves,
            curve_recipes: vec![],
            calibration: None,
            max_source_skew_ms: 1000,
        },
        pricing,
        overrides: vec![],
    }
}
pub fn staged(version: u64) -> StagedConfiguration {
    StagedConfiguration {
        document: document(version),
        idempotency_key: RequestId::new(format!("stage-{version}")).unwrap(),
        actor: "fixture-operator".into(),
        reason: "Fictional regression fixture".into(),
        staged_at: snapshot::time("2026-10-02T14:59:59Z"),
        fixture_evidence: FixtureEvidence::ReplayChecked,
    }
}
pub fn activate(version: u64, expected_revision: u64, key: &str) -> ActivationCommand {
    ActivationCommand {
        id: SourceId::new("CONFIG-RESTORE").unwrap(),
        configuration_version: snapshot::version(version),
        expected_revision,
        idempotency_key: RequestId::new(key).unwrap(),
        actor: "fixture-operator".into(),
        reason: "Fictional activation regression".into(),
        activated_at: snapshot::time("2026-10-02T15:00:00Z"),
        mode: if expected_revision == 0 {
            ReloadClass::RestartRequired
        } else {
            ReloadClass::Hot
        },
        action: ActivationKind::Activate,
        fixture_evidence: FixtureEvidence::ReplayChecked,
    }
}
