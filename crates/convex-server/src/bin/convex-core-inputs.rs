//! Offline captured core-input import. Stop the server before opening its local redb file.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
use convex_core::contracts::UtcTimestamp;
use convex_engine::ports::{
    fixing::{FixingRepository, PublishedFixing},
    reference_repository::ReferenceCommand,
    snapshot::SnapshotCurveRecipe,
};
use serde::Deserialize;
use std::{io::Read, process::ExitCode};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceImport {
    recorded_at: UtcTimestamp,
    command: ReferenceCommand,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Import {
    schema_version: u32,
    references: Vec<ReferenceImport>,
    curves: Vec<SnapshotCurveRecipe>,
    fixings: Vec<PublishedFixing>,
}
#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Core input import failed: {error}");
            ExitCode::FAILURE
        }
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "usage: convex-core-inputs DATABASE CAPTURED_INPUTS.json (server stopped)".into(),
        );
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&args[1])?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("core import exceeds 2 MiB".into());
    }
    let input: Import = serde_json::from_slice(&bytes)?;
    if input.schema_version != 1
        || input.references.len() > 32
        || input.curves.len() > 256
        || input.fixings.len() > 100_000
    {
        return Err("invalid core import schema/bounds".into());
    }
    for r in &input.references {
        r.command.validate()?;
    }
    for c in &input.curves {
        c.validate()?;
    }
    for f in &input.fixings {
        f.validate()?;
    }
    let (storage, _, fixings) = convex_ext_redb::create_redb_storage_with_core(&args[0])?;
    for r in &input.references {
        storage.bonds.commit(&r.command, r.recorded_at).await?;
    }
    for c in &input.curves {
        storage.curves.save_recipe(c).await?;
    }
    if !input.fixings.is_empty() {
        fixings.publish(&input.fixings).await?;
    }
    println!(
        "{}",
        serde_json::json!({"schema_version":1,"reference_commands":input.references.len(),"curve_recipes":input.curves.len(),"fixing_revisions":input.fixings.len()})
    );
    Ok(())
}
