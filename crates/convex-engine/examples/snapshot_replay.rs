//! Offline immutable snapshot administration; no server, feed session, scheduler or order gateway.
use convex_core::contracts::{ModelId, RequestId, SnapshotId, UtcTimestamp};
use convex_engine::{
    context::ValuationContext,
    snapshot::{SnapshotAssembler, SnapshotAssembly, SnapshotValuator},
};
use convex_ext_file::{FileQuoteReplay, FileSnapshotRepository, JsonFixingRepository};
use convex_ports::{
    market_data::StreamError, snapshot::SnapshotRepository, storage::BondPricingConfig,
};
use std::{io::Read, path::Path, sync::Arc};

fn json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, Box<dyn std::error::Error>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 16 * 1024 * 1024
    {
        return Err("input must be a regular non-symlink JSON file <=16 MiB".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("input grew beyond JSON bound".into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let output = match args.as_slice() {
        [mode, database, configuration] if mode == "store-config" => {
            let config: BondPricingConfig = json(configuration)?;
            let storage = convex_ext_redb::create_redb_storage(database)?;
            storage.configs.save(&config).await?;
            serde_json::json!({"config_id":config.config_id,"version":config.version})
        }
        [mode, database, fixings, replay, assembly, directory] if mode == "assemble" => {
            let request: SnapshotAssembly = json(assembly)?;
            let storage = convex_ext_redb::create_redb_storage(database)?;
            let repository = Arc::new(FileSnapshotRepository::new(Path::new(directory))?);
            let assembler = SnapshotAssembler::new(storage.bonds, storage.configs, Arc::new(JsonFixingRepository::new(fixings)?), repository);
            let mut cursor = FileQuoteReplay::open(replay)?;
            let mut events = Vec::new();
            loop {
                match cursor.read_event() {
                    Ok(event) => {
                        if events.len() == 100_000 { return Err("snapshot trace exceeds 100,000 events".into()); }
                        events.push(event);
                    }
                    Err(StreamError::Closed) => break,
                    Err(error) => return Err(error.into()),
                }
            }
            let snapshot = assembler.assemble(&request, &events).await?;
            serde_json::json!({"snapshot_id":snapshot.id(),"generation":snapshot.content().generation,"quality":snapshot.quality()})
        }
        [mode, directory, id] if mode == "verify" => {
            let repository = FileSnapshotRepository::new(directory)?;
            let snapshot = repository.get(&SnapshotId::new(id)?).await?.ok_or("snapshot absent")?;
            serde_json::json!({"snapshot_id":snapshot.id(),"generation":snapshot.content().generation,"quality":snapshot.quality(),"instruments":snapshot.content().references.len(),"curves":snapshot.content().curves.len(),"fixings":snapshot.content().fixings.len()})
        }
        [mode, directory, context, request_id, calculated_at, build_id] if mode == "value" => {
            let context: ValuationContext = json(context)?;
            let repository = FileSnapshotRepository::new(directory)?;
            let snapshot = repository.get(&context.spec().snapshot_id).await?.ok_or("snapshot absent")?;
            let valuator = SnapshotValuator::new(ModelId::new(build_id)?, 1)?;
            serde_json::to_value(valuator.value(&snapshot, &context, RequestId::new(request_id)?, UtcTimestamp::parse(calculated_at)?)?)?
        }
        _ => return Err("usage: snapshot_replay store-config DATABASE CONFIG_JSON | assemble DATABASE FIXINGS_JSON QUOTES_JSONL ASSEMBLY_JSON DIRECTORY | verify DIRECTORY SNAPSHOT_ID | value DIRECTORY CONTEXT_JSON REQUEST_ID CALCULATED_AT BUILD_ID".into()),
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
