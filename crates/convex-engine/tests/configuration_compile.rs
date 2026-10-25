//! `assemble_from_repository` over real redb storage loaded from the showcase seed:
//! the compiled document pins what the repository held at the cutoff, and every
//! input it cannot pin is refused rather than defaulted.
use convex_core::contracts::*;
use convex_engine::configuration::{
    assemble_from_repository, ConfigurationCapabilities, ConfigurationError, PlanSelection,
};
use convex_ports::{
    configuration::*,
    reference_master::{BookDefinition, MasterChange, MasterDefinition},
    reference_repository::ReferenceCommand,
    snapshot::SnapshotCurveRecipe,
    storage::{BondPricingConfig, StorageAdapter},
};
use serde_json::{json, Value};

/// Capture time of the seed: references, curves and pricing are recorded then.
const CAPTURE: &str = "2026-10-07T12:00:00Z";

fn seed() -> Value {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/showcase/seed.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn at(text: &str) -> UtcTimestamp {
    UtcTimestamp::parse(text).unwrap()
}

/// Seed references, curve recipes and the pricing configuration, plus one book.
async fn storage(dir: &tempfile::TempDir) -> (StorageAdapter, String) {
    let storage = convex_ext_redb::create_redb_storage(dir.path().join("inputs.redb")).unwrap();
    let seed = seed();
    for reference in seed["references"].as_array().unwrap() {
        let command: ReferenceCommand =
            serde_json::from_value(reference["command"].clone()).unwrap();
        storage
            .bonds
            .commit(&command, at(reference["recorded_at"].as_str().unwrap()))
            .await
            .unwrap();
    }
    for curve in seed["curves"].as_array().unwrap() {
        let recipe: SnapshotCurveRecipe = serde_json::from_value(curve.clone()).unwrap();
        storage.curves.save_recipe(&recipe).await.unwrap();
    }
    let pricing: BondPricingConfig =
        serde_json::from_value(seed["pricing_configs"][0].clone()).unwrap();
    storage.configs.save(&pricing).await.unwrap();
    let instrument = seed["references"][0]["command"]["changes"][0]["bond"]["instrument_id"]
        .as_str()
        .unwrap()
        .to_owned();
    book(
        &storage,
        "SHOWCASE-BOOK",
        &instrument,
        "2026-10-07T13:00:00Z",
    )
    .await;
    (storage, instrument)
}

async fn book(storage: &StorageAdapter, id: &str, instrument: &str, from: &str) {
    let definition: BookDefinition = serde_json::from_value(json!({
        "book_id": id,
        "owning_system": "test-bms",
        "name": null,
        "currency": null,
        "description": null,
        "effective": {"from": from, "until": null},
        "positions": [{
            "instrument_id": instrument,
            "quantity": "1000000",
            "unit": "face",
            "reference_version": null,
            "source_identifier": null,
            "sector": null,
            "rating": null
        }],
        "provenance": {"source": "test-bms", "evidence_reference": format!("test:{id}")}
    }))
    .unwrap();
    storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new(format!("book-{id}")).unwrap(),
                changes: vec![],
                master_changes: vec![MasterChange {
                    expected_version: None,
                    definition: MasterDefinition::Book(Box::new(definition)),
                }],
            },
            at(from),
        )
        .await
        .unwrap();
}

fn selection() -> PlanSelection {
    serde_json::from_value(json!({
        "schema_version": 1,
        "pricing_config": "SHOWCASE-USD",
        "books": ["SHOWCASE-BOOK"],
        "source": "showcase-feed",
        "fields": ["mid_price"],
        "quote_max_age_ms": 600_000,
        "max_source_skew_ms": 600_000,
        "as_of": "2026-10-07T14:00:00Z",
        "known_at": "2026-10-07T14:00:00Z",
        "mode": "live",
        "settlement_date": "2026-10-08",
        "measures": ["clean_price_mid"],
        "numerical_settings": {"policy_id": "convex-bond-router-v1", "bump_bps": 1.0},
        "max_concurrent_calcs": 4
    }))
    .unwrap()
}

fn capabilities(source: &str) -> ConfigurationCapabilities {
    ConfigurationCapabilities {
        build_id: ModelId::new("compile-test-build").unwrap(),
        storage: SourceId::new("redb-local").unwrap(),
        durable: true,
        atomic_activation: true,
        shared_writers: false,
        result_transport: SourceId::new("display-only").unwrap(),
        providers: vec![ProviderBinding {
            source: SourceId::new(source).unwrap(),
            capabilities: vec![
                ProviderCapability::CanonicalQuotes,
                ProviderCapability::LiveImages,
            ],
            credential: None,
        }],
        demo: false,
    }
}

async fn compile(
    storage: &StorageAdapter,
    selection: &PlanSelection,
    source: &str,
) -> Result<ConfigurationDocument, ConfigurationError> {
    assemble_from_repository(
        selection,
        storage.bonds.as_ref(),
        storage.configs.as_ref(),
        storage.curves.as_ref(),
        &capabilities(source),
    )
    .await
}

#[tokio::test]
async fn compiles_the_book_universe_with_exact_pins() {
    let dir = tempfile::tempdir().unwrap();
    let (storage, instrument) = storage(&dir).await;
    let document = compile(&storage, &selection(), "showcase-feed")
        .await
        .unwrap();
    assert_eq!(document.runtime.runtime_id.as_str(), "SHOWCASE-USD-v1");
    assert_eq!(document.runtime.mode, RuntimeMode::Live);
    assert_eq!(document.runtime.build_id.as_str(), "compile-test-build");
    assert_eq!(document.process.result_transport.as_str(), "display-only");
    assert_eq!(document.assembly.configuration.id.as_str(), "SHOWCASE-USD");
    assert_eq!(document.assembly.configuration.version.get(), 1);
    assert_eq!(document.assembly.instruments.len(), 1);
    assert_eq!(
        document.assembly.instruments[0].instrument_id.as_str(),
        instrument
    );
    assert_eq!(document.assembly.instruments[0].version.get(), 1);
    assert_eq!(document.assembly.quotes[0].key.source, "showcase-feed");
    let required: Vec<_> = document
        .assembly
        .required_curves
        .iter()
        .map(|id| id.as_str())
        .collect();
    assert_eq!(required, ["D.OIS", "P.SOFR"]);
    // Live names its curves and reads their revisions at each snapshot (D50).
    assert!(document.assembly.curve_recipes.is_empty());
    // Replay pins the revisions current at the cutoff.
    let mut replay = selection();
    replay.mode = RuntimeMode::Replay;
    let mut replay_capabilities = capabilities("showcase-feed");
    replay_capabilities.providers[0]
        .capabilities
        .push(ProviderCapability::Replay);
    let pinned = assemble_from_repository(
        &replay,
        storage.bonds.as_ref(),
        storage.configs.as_ref(),
        storage.curves.as_ref(),
        &replay_capabilities,
    )
    .await
    .unwrap();
    let pins: Vec<_> = pinned
        .assembly
        .curve_recipes
        .iter()
        .map(|r| (r.curve_id.as_str(), r.version.get()))
        .collect();
    assert_eq!(pins, [("D.OIS", 1), ("P.SOFR", 1)]);
    // The same cutoff compiles the same document.
    let again = compile(&storage, &selection(), "showcase-feed")
        .await
        .unwrap();
    assert_eq!(again.digest().unwrap(), document.digest().unwrap());
}

#[tokio::test]
async fn refuses_inputs_it_cannot_pin() {
    let dir = tempfile::tempdir().unwrap();
    let (storage, instrument) = storage(&dir).await;
    let invalid = |result: Result<ConfigurationDocument, ConfigurationError>| match result {
        Err(ConfigurationError::Invalid(reason)) => reason,
        other => panic!("expected an invalid selection, got {other:?}"),
    };

    // The book was recorded after this cutoff.
    let mut early = selection();
    early.as_of = at(CAPTURE);
    early.known_at = at(CAPTURE);
    assert_eq!(
        invalid(compile(&storage, &early, "showcase-feed").await),
        "book not found or not effective at the cutoff"
    );

    // A book that starts later is not effective at as_of.
    book(&storage, "LATER-BOOK", &instrument, "2026-10-07T13:30:00Z").await;
    let mut later = selection();
    later.books = vec![ReferenceDataId::new("LATER-BOOK").unwrap()];
    later.as_of = at("2026-10-07T13:15:00Z");
    assert_eq!(
        invalid(compile(&storage, &later, "showcase-feed").await),
        "book not found or not effective at the cutoff"
    );

    // Curves recorded at capture are past their one-day budget two days later.
    let mut stale = selection();
    stale.as_of = at("2026-10-09T14:00:00Z");
    stale.known_at = at("2026-10-09T14:00:00Z");
    assert_eq!(
        invalid(compile(&storage, &stale, "showcase-feed").await),
        "a configured curve has no fresh recipe of its role at the cutoff"
    );

    let mut unknown = selection();
    unknown.pricing_config = SourceId::new("NO-SUCH-CONFIG").unwrap();
    assert_eq!(
        invalid(compile(&storage, &unknown, "showcase-feed").await),
        "pricing configuration not found"
    );

    assert!(matches!(
        compile(&storage, &selection(), "other-feed").await,
        Err(ConfigurationError::Unsupported(_))
    ));

    let mut none = selection();
    none.books.clear();
    assert!(matches!(
        compile(&storage, &none, "showcase-feed").await,
        Err(ConfigurationError::Invalid(_))
    ));
}
