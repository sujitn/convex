//! Offline reference administration example. This starts no server or provider session.
use convex_core::contracts::{UtcTimestamp, Version};
use convex_core::ids::InstrumentId;
use convex_ext_file::{import_reference_file, load_reference_import};
use convex_ext_redb::create_redb_storage;
use convex_ports::reference_master::{ExternalIdentifier, ReferenceLookup};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().take(9).collect();
    match args.get(1).map(String::as_str) {
        Some("validate") if args.len() == 3 => {
            match load_reference_import(&args[2]) {
                Ok(document) => println!("{}", serde_json::json!({"status":"valid", "bond_changes":document.command.changes.len(),"master_changes":document.command.master_changes.len()})),
                Err(error) => { eprintln!("{}", serde_json::to_string(&error.diagnostics)?); return Err(error.into()); }
            }
        }
        Some("import") if args.len() == 5 => {
            let document = load_reference_import(&args[3]);
            if let Err(error) = document { eprintln!("{}", serde_json::to_string(&error.diagnostics)?); return Err(error.into()); }
            let storage = create_redb_storage(&args[2])?;
            match import_reference_file(storage.bonds.as_ref(), &args[3], UtcTimestamp::parse(&args[4])?).await {
                Ok(receipt) => println!("{}", serde_json::to_string(&receipt)?),
                Err(error) => { eprintln!("{}", serde_json::to_string(&error.diagnostics)?); return Err(error.into()); }
            }
        }
        Some("resolve") if args.len() == 6 => {
            let identifier: ExternalIdentifier = serde_json::from_str(&args[3])?;
            let lookup = ReferenceLookup { as_of: UtcTimestamp::parse(&args[4])?, known_at: UtcTimestamp::parse(&args[5])? };
            let storage = create_redb_storage(&args[2])?;
            println!("{}", serde_json::to_string(&storage.bonds.resolve_at(&identifier, lookup).await?)?);
        }
        Some("version") if args.len() == 5 => {
            let id = InstrumentId::new(&args[3]);
            let version = Version::new(args[4].parse()?)?;
            let storage = create_redb_storage(&args[2])?;
            println!("{}", serde_json::to_string(&storage.bonds.get_version(&id, version).await?)?);
        }
        _ => return Err("usage: reference_master validate FILE | import DB FILE RECORDED_AT | resolve DB IDENTIFIER_JSON AS_OF KNOWN_AT | version DB INSTRUMENT_ID VERSION".into()),
    }
    Ok(())
}
