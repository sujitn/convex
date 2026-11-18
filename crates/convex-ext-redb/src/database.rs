//! The application schema marker and explicitly durable transaction helpers.

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
/// Bonds stored before versioned reference records; a database holding any is refused.
pub(crate) const UNVERSIONED_BONDS: TableDefinition<&str, &[u8]> = TableDefinition::new("bonds");

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

/// Rows in `table`, or 0 when it does not exist. redb stores the count with the table.
pub(crate) fn table_len<K: redb::Key + 'static, V: redb::Value + 'static>(
    read: &redb::ReadTransaction,
    table: TableDefinition<K, V>,
) -> Result<u64, TraitError> {
    match read.open_table(table) {
        Ok(table) => table.len().map_err(database_error),
        Err(redb::TableError::TableDoesNotExist(_)) => Ok(0),
        Err(error) => Err(database_error(error)),
    }
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

/// Write the application schema marker into a new database, or verify the current one.
///
/// A database without the marker must hold no configurations and no unversioned bonds:
/// stored data without the marker is refused, never adopted. A newer schema is refused.
pub fn migrate_database(db: &Database) -> Result<(), TraitError> {
    let read = db.begin_read().map_err(database_error)?;
    if table_len(&read, UNVERSIONED_BONDS)? > 0 {
        return Err(TraitError::Unsupported("unversioned bonds"));
    }
    drop(read);
    let existing_version = schema_version(db)?;
    if existing_version > APPLICATION_SCHEMA_VERSION {
        return Err(TraitError::Unsupported("newer application schema"));
    }
    if existing_version == APPLICATION_SCHEMA_VERSION {
        // Reopen/inspection verifies the current schema without allocating a write transaction.
        return verify_configuration_tables(db);
    }
    let write = begin_write(db)?;
    {
        let mut schema = write.open_table(SCHEMA).map_err(database_error)?;
        if schema.get("version").map_err(database_error)?.is_some() {
            // Another store on this handle wrote the marker first; never recreate the
            // configuration tables of a marked database.
            return verify_configuration_tables(db);
        }
        // Opening the configuration tables creates them, as a marked database requires.
        let configs = write.open_table(CONFIGS).map_err(database_error)?;
        let history = write.open_table(CONFIG_HISTORY).map_err(database_error)?;
        if !configs.is_empty().map_err(database_error)?
            || !history.is_empty().map_err(database_error)?
        {
            return Err(TraitError::Unsupported(
                "configurations without a schema marker",
            ));
        }
        schema
            .insert("version", APPLICATION_SCHEMA_VERSION)
            .map_err(database_error)?;
    }
    commit(write)
}

fn verify_configuration_tables(db: &Database) -> Result<(), TraitError> {
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
