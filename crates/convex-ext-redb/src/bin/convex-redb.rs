//! Offline local database maintenance.
use convex_ext_redb::{backup_database, migrate_database, restore_database};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).take(4).collect();
    match args.as_slice() {
        [operation, source, target] if operation == "backup" =>
            println!("{}", serde_json::to_string_pretty(&backup_database(Path::new(source), Path::new(target))?)?),
        [operation, source, target] if operation == "restore" =>
            println!("{}", serde_json::to_string_pretty(&restore_database(Path::new(source), Path::new(target))?)?),
        [operation, source] if operation == "migrate" => {
            let db = redb::Database::open(Path::new(source))?;
            migrate_database(&db)?;
            println!("application schema {}", convex_ext_redb::APPLICATION_SCHEMA_VERSION);
        },
        [operation, source] if operation == "compact" => {
            let mut db = redb::Database::open(Path::new(source))?;
            let changed = db.compact()?;
            println!("{}", serde_json::json!({"compacted":changed, "business_records_reconciled":false}));
        },
        _ => return Err("usage: convex-redb backup DATABASE NEW_DIRECTORY | restore BACKUP_DIRECTORY NEW_DATABASE | migrate DATABASE | compact DATABASE".into()),
    }
    Ok(())
}
