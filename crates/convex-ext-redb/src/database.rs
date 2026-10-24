//! Local schema migration and explicitly durable transaction helpers.

use convex_ports::error::TraitError;
use redb::{
    Database, Durability, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition,
    WriteTransaction,
};

use crate::config_store::{decode_config, encode_config, CONFIGS, CONFIG_HISTORY};

/// Current Convex application schema, independent of the redb file format.
pub const APPLICATION_SCHEMA_VERSION: u64 = 1;
pub(crate) const SCHEMA: TableDefinition<&str, u64> = TableDefinition::new("convex_schema");
pub(crate) const MAX_RECORDS: u64 = 100_000;
pub(crate) const MAX_HISTORY: u64 = 1_000_000;
pub(crate) const MAX_RECORD_BYTES: usize = 128 * 1024;

pub(crate) fn database_error(error: impl std::fmt::Display) -> TraitError {
    TraitError::DatabaseError(error.to_string())
}

pub(crate) fn begin_write(db: &Database) -> Result<WriteTransaction, TraitError> {
    let mut write = db.begin_write().map_err(database_error)?;
    write
        .set_durability(Durability::Immediate)
        .map_err(database_error)?;
    Ok(write)
}

pub(crate) fn commit(write: WriteTransaction) -> Result<(), TraitError> {
    write.commit().map_err(|_| TraitError::UnknownOutcome)
}

pub(crate) fn validate_id(value: &str) -> Result<(), TraitError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(TraitError::InvalidInput("invalid storage identity"));
    }
    Ok(())
}

pub(crate) fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, TraitError> {
    let bytes =
        serde_json::to_vec(value).map_err(|e| TraitError::SerializationError(e.to_string()))?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(TraitError::CapacityExceeded);
    }
    Ok(bytes)
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, TraitError> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(TraitError::CapacityExceeded);
    }
    serde_json::from_slice(bytes).map_err(|e| TraitError::ParseError(e.to_string()))
}

pub(crate) fn schema_version(db: &Database) -> Result<u64, TraitError> {
    let read = db.begin_read().map_err(database_error)?;
    match read.open_table(SCHEMA) {
        Ok(table) => table
            .get("version")
            .map_err(database_error)?
            .map(|v| v.value())
            .ok_or(TraitError::Unsupported(
                "missing application schema version",
            )),
        Err(redb::TableError::TableDoesNotExist(_)) => Ok(0),
        Err(e) => Err(database_error(e)),
    }
}

/// Atomically adopt unversioned configuration storage into application schema 1.
///
/// Only the actually retained current version is imported. Older lost content
/// remains absent. Invalid records and unsupported newer schemas abort the entire
/// migration. Back up the closed database before upgrading; rollback restores that
/// backup into isolation rather than inventing a down-migration.
pub fn migrate_database(db: &Database) -> Result<(), TraitError> {
    let read = db.begin_read().map_err(database_error)?;
    match read.open_table(TableDefinition::<&str, &[u8]>::new("bonds")) {
        Ok(table) if !table.is_empty().map_err(database_error)? => {
            return Err(TraitError::Unsupported(
                "unversioned bonds require explicit export and reference import",
            ))
        }
        Ok(_) | Err(redb::TableError::TableDoesNotExist(_)) => {}
        Err(e) => return Err(database_error(e)),
    }
    drop(read);
    let existing_version = schema_version(db)?;
    if existing_version > APPLICATION_SCHEMA_VERSION {
        return Err(TraitError::Unsupported("newer application schema"));
    }
    if existing_version == APPLICATION_SCHEMA_VERSION {
        // Reopen/inspection verifies the current schema without allocating a write transaction.
        return validate_migrated_configurations(db);
    }
    let write = begin_write(db)?;
    {
        let mut schema = write.open_table(SCHEMA).map_err(database_error)?;
        let version = schema
            .get("version")
            .map_err(database_error)?
            .map(|v| v.value())
            .unwrap_or(0);
        if version > APPLICATION_SCHEMA_VERSION {
            return Err(TraitError::Unsupported("newer application schema"));
        }
        if version == APPLICATION_SCHEMA_VERSION {
            // Do not silently recreate missing history in a supposedly migrated database.
            validate_migrated_configurations(db)?;
            return Ok(());
        }
        let configs = write.open_table(CONFIGS).map_err(database_error)?;
        let mut history = write.open_table(CONFIG_HISTORY).map_err(database_error)?;
        if configs.len().map_err(database_error)? > MAX_RECORDS {
            return Err(TraitError::CapacityExceeded);
        }
        if !history.is_empty().map_err(database_error)? {
            return Err(TraitError::Unsupported(
                "configuration history without a schema marker",
            ));
        }
        for entry in configs.iter().map_err(database_error)? {
            let (key, data) = entry.map_err(database_error)?;
            let config = decode_config(data.value())?;
            if config.config_id != key.value() {
                return Err(TraitError::Unsupported(
                    "configuration key/content mismatch",
                ));
            }
            history
                .insert((key.value(), config.version), data.value())
                .map_err(database_error)?;
        }
        schema
            .insert("version", APPLICATION_SCHEMA_VERSION)
            .map_err(database_error)?;
    }
    commit(write)
}

fn validate_migrated_configurations(db: &Database) -> Result<(), TraitError> {
    let read = db.begin_read().map_err(database_error)?;
    let history = read
        .open_table(CONFIG_HISTORY)
        .map_err(|_| TraitError::Unsupported("missing configuration history"))?;
    let current = read
        .open_table(CONFIGS)
        .map_err(|_| TraitError::Unsupported("missing current configuration"))?;
    if current.len().map_err(database_error)? > MAX_RECORDS
        || history.len().map_err(database_error)? > MAX_HISTORY
    {
        return Err(TraitError::CapacityExceeded);
    }
    if current.is_empty().map_err(database_error)? && !history.is_empty().map_err(database_error)? {
        return Err(TraitError::Unsupported(
            "configuration history has no current records",
        ));
    }
    for entry in current.iter().map_err(database_error)? {
        let (key, data) = entry.map_err(database_error)?;
        let config = decode_config(data.value())?;
        let retained = history
            .get((key.value(), config.version))
            .map_err(database_error)?
            .ok_or(TraitError::Unsupported(
                "current configuration is absent from history",
            ))?;
        let latest = history
            .range((key.value(), 1)..=(key.value(), u64::MAX))
            .map_err(database_error)?
            .next_back()
            .transpose()
            .map_err(database_error)?
            .map(|(key, _)| key.value().1);
        if key.value() != config.config_id
            || latest != Some(config.version)
            || encode_config(&config)? != encode_config(&decode_config(retained.value())?)?
        {
            return Err(TraitError::Unsupported(
                "current configuration and history disagree",
            ));
        }
    }
    Ok(())
}
