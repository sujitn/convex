//! Generate a new fictional offline workspace for the configuration operator walkthrough.
//! Shared regression data is intentional; no production startup default is created.
#[path = "../../convex-ports/tests/support/configuration_fixtures.rs"]
mod sample;
use convex_core::contracts::*;
use convex_engine::{configuration::*, ports::configuration::*, ports::reference_repository::*};
use convex_server::SERVER_BUILD_ID;
use std::{fs::OpenOptions, io::Write, path::Path};

fn write(path: &Path, value: &impl serde::Serialize) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args_os().skip(1).take(2).collect::<Vec<_>>();
    if args.len() != 1 {
        return Err("usage: configuration_fixture NEW_DIRECTORY".into());
    }
    let directory = Path::new(&args[0]);
    std::fs::create_dir(directory)?;
    let storage = convex_ext_redb::create_redb_storage(directory.join("inputs.redb"))?;
    let bond = sample::snapshot::bond();
    storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("fictional-reference-seed")?,
                changes: vec![ReferenceChange::Create {
                    bond: Box::new(bond.clone()),
                }],
                master_changes: sample::snapshot::reference_terms::resources(&bond),
            },
            UtcTimestamp::parse("2025-01-01T00:00:00Z")?,
        )
        .await?;

    let mut first = sample::staged(1);
    first.document.runtime.mode = RuntimeMode::Replay;
    first.document.runtime.build_id = ModelId::new(SERVER_BUILD_ID)?;
    first.document.providers[0]
        .capabilities
        .push(ProviderCapability::Replay);
    let mut second = first.clone();
    second.idempotency_key = RequestId::new("stage-2")?;
    second.document.pricing.version = 2;
    second.document.pricing.updated_at = 200;
    second.document.assembly.configuration.version = Version::new(2)?;
    second.document.assembly.quotes[0].max_age_ms = 3000;
    let event = sample::snapshot::content().quotes[0]
        .delivery_watermark
        .clone()
        .ok_or("fictional quote image missing")?;
    let fixtures = vec![ConfigurationFixture {
        id: SourceId::new("fictional-bond-mid")?,
        as_of: first.document.assembly.as_of,
        known_at: first.document.assembly.known_at,
        events: vec![event.clone()],
        expected: vec![ExpectedConfigurationMeasure {
            instrument_id: InstrumentId::new("BOND-001")?,
            measure: MeasureId::new("clean_price_mid")?,
            value: FiniteF64::new(99.25)?,
            tolerance: FiniteF64::new(0.0)?,
        }],
    }];
    let mut rollback = sample::activate(1, 2, "rollback-1");
    rollback.action = ActivationKind::Rollback {
        to_revision: Version::new(1)?,
    };
    for (filename, value) in [
        ("document1.json", serde_json::to_value(&first.document)?),
        ("document2.json", serde_json::to_value(&second.document)?),
        ("stage1.json", serde_json::to_value(&first)?),
        ("stage2.json", serde_json::to_value(&second)?),
        (
            "activate1.json",
            serde_json::to_value(sample::activate(1, 0, "activate-1"))?,
        ),
        (
            "activate2.json",
            serde_json::to_value(sample::activate(2, 1, "activate-2"))?,
        ),
        ("rollback1.json", serde_json::to_value(rollback)?),
        ("fixtures.json", serde_json::to_value(fixtures)?),
    ] {
        write(&directory.join(filename), &value)?;
    }
    let mut replay = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("quotes.jsonl"))?;
    writeln!(replay, "{}", serde_json::to_string(&event)?)?;
    replay.sync_all()?;
    println!(
        "{}",
        serde_json::json!({"schema_version":1,"fictional":true,"configuration_id":"CONFIG-RESTORE","numerical_expectation":{"clean_price_mid":99.25,"absolute_tolerance":0.0},"staged":false,"active":false})
    );
    Ok(())
}
