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
    capabilities_of(&[source])
}

/// A composition with a Live provider for each of `sources`.
fn capabilities_of(sources: &[&str]) -> ConfigurationCapabilities {
    ConfigurationCapabilities {
        build_id: ModelId::new("compile-test-build").unwrap(),
        storage: SourceId::new("redb-local").unwrap(),
        durable: true,
        atomic_activation: true,
        shared_writers: false,
        result_transport: SourceId::new("display-only").unwrap(),
        providers: sources
            .iter()
            .map(|source| ProviderBinding {
                source: SourceId::new(*source).unwrap(),
                capabilities: vec![
                    ProviderCapability::CanonicalQuotes,
                    ProviderCapability::LiveImages,
                ],
                credential: None,
            })
            .collect(),
        demo: false,
    }
}

async fn compile(
    storage: &StorageAdapter,
    selection: &PlanSelection,
    source: &str,
) -> Result<ConfigurationDocument, ConfigurationError> {
    compile_with(storage, selection, &capabilities(source)).await
}

async fn compile_with(
    storage: &StorageAdapter,
    selection: &PlanSelection,
    capabilities: &ConfigurationCapabilities,
) -> Result<ConfigurationDocument, ConfigurationError> {
    assemble_from_repository(
        selection,
        storage.bonds.as_ref(),
        storage.configs.as_ref(),
        storage.curves.as_ref(),
        capabilities,
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

/// Store `count` copies of the seed's bond, `WIDE-0000`..., and books of at most 1,000
/// positions holding them, `WIDE-BOOK-0`...; returns the books.
async fn wide(storage: &StorageAdapter, count: usize) -> Vec<ReferenceDataId> {
    let seed = seed();
    let template = seed["references"][0]["command"]["changes"][0].clone();
    let ids: Vec<String> = (0..count).map(|i| format!("WIDE-{i:04}")).collect();
    for (chunk, ids) in ids.chunks(256).enumerate() {
        let changes: Vec<Value> = ids
            .iter()
            .map(|id| {
                let mut change = template.clone();
                change["bond"]["instrument_id"] = json!(id);
                change["bond"]["isin"] = json!(format!("ISIN-{id}"));
                change
            })
            .collect();
        let command: ReferenceCommand = serde_json::from_value(json!({
            "idempotency_key": format!("wide-{chunk}"),
            "changes": changes,
            "master_changes": []
        }))
        .unwrap();
        storage.bonds.commit(&command, at(CAPTURE)).await.unwrap();
    }
    let mut books = Vec::new();
    for (n, ids) in ids.chunks(1000).enumerate() {
        let id = format!("WIDE-BOOK-{n}");
        let positions: Vec<Value> = ids
            .iter()
            .map(|instrument| {
                json!({
                    "instrument_id": instrument,
                    "quantity": "1000000",
                    "unit": "face",
                    "reference_version": null,
                    "source_identifier": null,
                    "sector": null,
                    "rating": null
                })
            })
            .collect();
        let definition: BookDefinition = serde_json::from_value(json!({
            "book_id": id,
            "owning_system": "test-bms",
            "name": null,
            "currency": null,
            "description": null,
            "effective": {"from": "2026-10-07T13:00:00Z", "until": null},
            "positions": positions,
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
                at("2026-10-07T13:00:00Z"),
            )
            .await
            .unwrap();
        books.push(ReferenceDataId::new(id).unwrap());
    }
    books
}

/// D101: books holding 4,096 instruments compile to a Live document that validates; the
/// same books are refused as a Replay universe, past 256, and one more instrument is
/// refused as a Live one.
#[tokio::test]
async fn a_live_universe_compiles_up_to_4096_instruments_and_a_replay_one_up_to_256() {
    let dir = tempfile::tempdir().unwrap();
    let (storage, _) = storage(&dir).await;
    let books = wide(&storage, 4096).await;
    let mut live = selection();
    live.books = books.clone();
    let document = compile(&storage, &live, "showcase-feed").await.unwrap();
    assert_eq!(document.assembly.instruments.len(), 4096);
    assert_eq!(document.assembly.quotes.len(), 4096);
    document.validate().unwrap();

    let mut replay = live.clone();
    replay.mode = RuntimeMode::Replay;
    let mut replay_capabilities = capabilities("showcase-feed");
    replay_capabilities.providers[0]
        .capabilities
        .push(ProviderCapability::Replay);
    assert!(matches!(
        assemble_from_repository(
            &replay,
            storage.bonds.as_ref(),
            storage.configs.as_ref(),
            storage.curves.as_ref(),
            &replay_capabilities,
        )
        .await,
        Err(ConfigurationError::CapacityExceeded)
    ));

    let mut over = live;
    over.books
        .push(ReferenceDataId::new("SHOWCASE-BOOK").unwrap());
    assert!(matches!(
        compile(&storage, &over, "showcase-feed").await,
        Err(ConfigurationError::CapacityExceeded)
    ));
}

/// A Treasury: the seed's bond under `id` with a sovereign issuer, its own maturity and a
/// CUSIP.
fn treasury(id: &str, maturity: &str, cusip: &str) -> Value {
    let mut bond = seed()["references"][0]["command"]["changes"][0]["bond"].clone();
    bond["instrument_id"] = id.into();
    bond["issuer_type"] = "Sovereign".into();
    bond["maturity_date"] = maturity.into();
    bond["cusip"] = cusip.into();
    bond["isin"] = Value::Null;
    bond
}

/// Store the 2- and 10-year Treasuries (the seed's bond with a sovereign issuer), a
/// `UST-OTR` set of both, and a rule quoting the seed's bond `method` against it.
async fn level_desk(storage: &StorageAdapter, instrument: &str, method: Value) {
    let notes: Vec<Value> = [
        ("UST-2Y", "2028-01-15", "91282CAA9"),
        ("UST-10Y", "2035-01-15", "91282CBB6"),
    ]
    .into_iter()
    .map(|(id, maturity, cusip)| json!({"kind": "create", "bond": treasury(id, maturity, cusip)}))
    .collect();
    let effective = json!({"from": "2026-01-01T00:00:00Z", "until": null});
    let provenance = json!({"source": "desk", "evidence_reference": "compile-test"});
    let command: ReferenceCommand = serde_json::from_value(json!({
        "idempotency_key": "level-desk",
        "changes": notes,
        "master_changes": [
            {"expected_version": null, "definition": {"kind": "benchmark_set", "definition": {
                "benchmark_set_id": "UST-OTR",
                "currency": "USD",
                "securities": [
                    {"tenor": "2Y", "instrument_id": "UST-2Y", "version": 1},
                    {"tenor": "10Y", "instrument_id": "UST-10Y", "version": 1},
                ],
                "effective": effective,
                "provenance": provenance,
            }}},
            {"expected_version": null, "definition": {"kind": "pricing_rule", "definition": {
                "instrument_id": instrument,
                "method": method,
                "benchmark_set_id": "UST-OTR",
                "effective": effective,
                "provenance": provenance,
            }}},
        ],
    }))
    .unwrap();
    storage
        .bonds
        .commit(&command, at("2026-10-07T12:30:00Z"))
        .await
        .unwrap();
}

/// What a compiled quote reads: its instrument, source, fields and age.
fn quote_reads(document: &ConfigurationDocument) -> Vec<(String, String, Vec<String>, i64)> {
    document
        .assembly
        .quotes
        .iter()
        .map(|quote| {
            (
                quote.key.instrument_id.as_str().to_owned(),
                quote.key.source.clone(),
                quote
                    .fields
                    .iter()
                    .map(|field| {
                        serde_json::to_value(field)
                            .unwrap()
                            .as_str()
                            .unwrap()
                            .to_owned()
                    })
                    .collect(),
                quote.max_age_ms,
            )
        })
        .collect()
}

/// A book bond quoted as a spread over a benchmark set brings the set's Treasuries into
/// the universe at the versions the set pins, quoted by price, and reads its own spread
/// from the levels' source, as old as a trading day; without a levels source it reads its
/// spread from the selection's.
#[tokio::test]
async fn a_level_quoted_bond_brings_its_benchmarks_and_reads_its_level() {
    let dir = tempfile::tempdir().unwrap();
    let (storage, instrument) = storage(&dir).await;
    level_desk(
        &storage,
        &instrument,
        json!({"kind": "benchmark_spread", "tenor": "10Y"}),
    )
    .await;
    let mut desk = selection();
    desk.levels =
        Some(serde_json::from_value(json!({"source": "desk", "max_age_ms": 86_400_000})).unwrap());
    let document = compile_with(
        &storage,
        &desk,
        &capabilities_of(&["showcase-feed", "desk"]),
    )
    .await
    .unwrap();
    let pins: Vec<_> = document
        .assembly
        .instruments
        .iter()
        .map(|pin| (pin.instrument_id.as_str(), pin.version.get()))
        .collect();
    assert_eq!(
        pins,
        [(instrument.as_str(), 1), ("UST-10Y", 1), ("UST-2Y", 1)]
    );
    let feed = |id: &str| {
        (
            id.to_owned(),
            "showcase-feed".to_owned(),
            vec!["mid_price".to_owned()],
            600_000,
        )
    };
    assert_eq!(
        quote_reads(&document),
        [
            (
                instrument.clone(),
                "desk".to_owned(),
                vec!["mid_spread".to_owned()],
                86_400_000
            ),
            feed("UST-10Y"),
            feed("UST-2Y"),
        ]
    );
    assert_eq!(document.providers.len(), 2);

    let document = compile(&storage, &selection(), "showcase-feed")
        .await
        .unwrap();
    assert_eq!(
        quote_reads(&document)[0],
        (
            instrument.clone(),
            "showcase-feed".to_owned(),
            vec!["mid_spread".to_owned()],
            600_000
        )
    );
    assert_eq!(document.providers.len(), 1);
}

/// Benchmarks are quoted by price, at the versions their set pins, from a composed
/// provider; a compile that cannot give them that is refused.
#[tokio::test]
async fn a_level_quoted_universe_is_refused_when_its_benchmarks_cannot_be_priced() {
    let dir = tempfile::tempdir().unwrap();
    let (storage, instrument) = storage(&dir).await;
    level_desk(
        &storage,
        &instrument,
        json!({"kind": "benchmark_spread", "tenor": "10Y"}),
    )
    .await;
    let invalid = |result: Result<ConfigurationDocument, ConfigurationError>| match result {
        Err(ConfigurationError::Invalid(reason)) => reason,
        other => panic!("expected an invalid selection, got {other:?}"),
    };
    let mut yields = selection();
    yields.fields = vec![serde_json::from_value(json!("mid_yield")).unwrap()];
    assert_eq!(
        invalid(compile(&storage, &yields, "showcase-feed").await),
        "benchmark securities are quoted by price: the selection's fields include mid_price"
    );

    let mut desk = selection();
    desk.levels =
        Some(serde_json::from_value(json!({"source": "desk", "max_age_ms": 86_400_000})).unwrap());
    assert!(matches!(
        compile(&storage, &desk, "showcase-feed").await,
        Err(ConfigurationError::Unsupported(_))
    ));

    // A book holds the 10-year at a later version than the set pins.
    let mut reopened = treasury("UST-10Y", "2035-01-15", "91282CBB6");
    reopened["description"] = "reopened".into();
    let command: ReferenceCommand = serde_json::from_value(json!({
        "idempotency_key": "reopen-10y",
        "changes": [{"kind": "update", "bond": reopened, "expected_version": 1}],
        "master_changes": [],
    }))
    .unwrap();
    storage
        .bonds
        .commit(&command, at("2026-10-07T12:45:00Z"))
        .await
        .unwrap();
    book(&storage, "TREASURY-BOOK", "UST-10Y", "2026-10-07T13:00:00Z").await;
    let mut both = selection();
    both.books
        .push(ReferenceDataId::new("TREASURY-BOOK").unwrap());
    assert_eq!(
        invalid(compile(&storage, &both, "showcase-feed").await),
        "a benchmark set pins another version of a book instrument"
    );

    // The rule now names a set that comes into force only after the cutoff.
    let later: ReferenceCommand = serde_json::from_value(json!({
        "idempotency_key": "later-set",
        "changes": [],
        "master_changes": [
            {"expected_version": null, "definition": {"kind": "benchmark_set", "definition": {
                "benchmark_set_id": "UST-LATER",
                "currency": "USD",
                "securities": [{"tenor": "10Y", "instrument_id": "UST-10Y", "version": 1}],
                "effective": {"from": "2026-12-01T00:00:00Z", "until": null},
                "provenance": {"source": "desk", "evidence_reference": "compile-test"},
            }}},
            {"expected_version": 1, "definition": {"kind": "pricing_rule", "definition": {
                "instrument_id": instrument,
                "method": {"kind": "benchmark_spread", "tenor": "10Y"},
                "benchmark_set_id": "UST-LATER",
                "effective": {"from": "2026-01-01T00:00:00Z", "until": null},
                "provenance": {"source": "desk", "evidence_reference": "compile-test"},
            }}},
        ],
    }))
    .unwrap();
    storage
        .bonds
        .commit(&later, at("2026-10-07T12:50:00Z"))
        .await
        .unwrap();
    assert_eq!(
        invalid(compile(&storage, &selection(), "showcase-feed").await),
        "a pricing rule names a benchmark set not in force at the cutoff"
    );
}
