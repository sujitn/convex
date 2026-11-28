//! `convex-config` validates a floating rate note's document offline against the fixings
//! the database holds.
#[path = "../../convex-engine/tests/support/configuration_setup.rs"]
mod configuration;
#[path = "../../convex-engine/tests/support/products.rs"]
mod products;
#[path = "../../convex-engine/tests/support/curve_replay.rs"]
mod recipes;
use configuration::{fixtures, runtime::*};
use convex_core::contracts::*;
use convex_engine::configuration::ConfigurationFixture;
use convex_engine::ports::{
    configuration::{ConfigurationDocument, ProviderCapability, RuntimeMode},
    fixing::{FixingRepository, PublishedFixing},
    quote_event::{QuoteEvent, QuoteField},
    reference_repository::{ReferenceChange, ReferenceCommand},
};
use convex_server::SERVER_BUILD_ID;
use serde_json::{json, Value};

/// BOND-001 stored as a SOFR note quoted by its discount margin, with `fixings` published,
/// and a document over it whose fixture expects the note at a 45 bp margin.
async fn floating_note(
    setup: &Setup,
    fixings: Vec<PublishedFixing>,
) -> (
    ConfigurationDocument,
    Vec<ConfigurationFixture>,
    Vec<QuoteEvent>,
) {
    let note = products::frn(fixture::bond());
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("frn-terms").unwrap(),
                changes: vec![ReferenceChange::Update {
                    bond: Box::new(note.clone()),
                    expected_version: fixture::version(1),
                }],
                master_changes: vec![
                    products::convention(&note),
                    products::index(&note),
                    products::pricing_rule("BOND-001", json!({"kind": "discount_margin"})),
                ],
            },
            fixture::time("2025-06-01T00:00:00Z"),
        )
        .await
        .unwrap();
    if !fixings.is_empty() {
        setup.fixings.publish(&fixings).await.unwrap();
    }
    let recipes = [recipes::recipe(), products::projection(recipes::recipe())].map(recipes::record);
    for recipe in &recipes {
        setup.storage.curves.save_recipe(recipe).await.unwrap();
    }
    let mut pricing = fixture::config();
    pricing.version = 2;
    pricing.updated_at = 200;
    pricing.analytics_curves.frn_reference_curve =
        Some(convex_core::ids::CurveId::new("USD-SOFR-PROJECTION"));
    setup.storage.configs.save(&pricing).await.unwrap();

    let mut document = configuration_fixture::document(2);
    document.pricing = pricing;
    document.runtime.mode = RuntimeMode::Replay;
    document.runtime.build_id = ModelId::new(SERVER_BUILD_ID).unwrap();
    document
        .runtime
        .measures
        .push(MeasureId::new("discount_margin_mid").unwrap());
    document.providers[0]
        .capabilities
        .push(ProviderCapability::Replay);
    let assembly = &mut document.assembly;
    assembly.instruments[0].version = fixture::version(2);
    assembly.curves.clear();
    assembly.required_curves = recipes
        .iter()
        .map(|recipe| recipe.curve_id.clone())
        .collect();
    assembly.curve_recipes = recipes.to_vec();
    let mut events = setup.plan.seed_events.clone();
    products::quote_in(
        &mut assembly.quotes[0],
        &mut events[0],
        QuoteField::MidSpread,
        rust_decimal::Decimal::from(45),
    );
    let mut cases = fixtures(setup);
    cases[0].events = events.clone();
    cases[0].expected[0].measure = MeasureId::new("discount_margin_mid").unwrap();
    cases[0].expected[0].value = FiniteF64::new(45.0).unwrap();
    (document, cases, events)
}

/// `convex-config validate`, in a separate process that opens the database alone.
fn validate_offline(
    setup: Setup,
    document: &ConfigurationDocument,
    cases: &[ConfigurationFixture],
    events: &[QuoteEvent],
) -> Value {
    let dir = setup.release();
    let path = |name: &str| dir.path().join(name).to_str().unwrap().to_owned();
    std::fs::write(path("document.json"), serde_json::to_vec(document).unwrap()).unwrap();
    std::fs::write(path("fixtures.json"), serde_json::to_vec(cases).unwrap()).unwrap();
    let replay: String = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap() + "\n")
        .collect();
    std::fs::write(path("quotes.jsonl"), replay).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_convex-config"))
        .args([
            path("inputs.redb"),
            "validate".to_owned(),
            path("document.json"),
            path("fixtures.json"),
            path("quotes.jsonl"),
        ])
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}

async fn validate_with(fixings: Vec<PublishedFixing>) -> Value {
    let setup = setup().await;
    let (document, cases, events) = floating_note(&setup, fixings).await;
    validate_offline(setup, &document, &cases, &events)
}

fn issue_codes(validation: &Value) -> Vec<&str> {
    validation["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|issue| issue["code"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn a_floating_note_without_its_fixings_in_the_database_is_refused_its_inputs() {
    let validation = validate_with(Vec::new()).await;
    assert_eq!(validation["valid"], false, "{validation:#}");
    assert_eq!(issue_codes(&validation), ["INVALID_FIXTURE_INPUTS"]);
}

#[tokio::test]
async fn a_floating_note_validates_at_its_margin_against_the_fixings_the_database_holds() {
    let validation = validate_with(products::published()).await;
    assert_eq!(validation["valid"], true, "{validation:#}");
    assert!(issue_codes(&validation).is_empty());
}
