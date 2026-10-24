//! Single-owner configuration staging and active selection in atomic local transactions.
use crate::{config_store::save_config_in_transaction, database::*};
use async_trait::async_trait;
use convex_core::contracts::{RequestId, SourceId, Version};
use convex_ports::{
    configuration::*,
    storage::{Page, Pagination},
    TraitError,
};
use redb::{
    Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition, TableHandle,
};
use std::sync::Arc;

const DOCUMENTS: TableDefinition<(&str, u64), &[u8]> =
    TableDefinition::new("configuration_documents_v1");
const ACTIVE: TableDefinition<&str, u64> = TableDefinition::new("configuration_active_v1");
const ACTIVATIONS: TableDefinition<(&str, u64), &[u8]> =
    TableDefinition::new("configuration_activations_v1");
const REQUESTS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("configuration_activation_requests_v1");
const STAGE_REQUESTS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("configuration_stage_requests_v1");
const MAX_DOCUMENTS: u64 = 4096;
const MAX_IDENTITIES: u64 = 64;
const MAX_ACTIVATIONS: u64 = 100_000;

/// Additive schema-1 application configuration repository sharing the authoritative database.
pub struct RedbConfigurationRepository {
    db: Arc<Database>,
}
impl RedbConfigurationRepository {
    /// Create empty activation tables without promoting any older latest-pricing record.
    pub fn new(db: Arc<Database>) -> Result<Self, TraitError> {
        crate::migrate_database(&db)?;
        let read = db.begin_read().map_err(database_error)?;
        let schema = read.open_table(SCHEMA).map_err(database_error)?;
        let version = schema
            .get("configuration_activation")
            .map_err(database_error)?
            .map(|v| v.value());
        if let Some(version) = version {
            if version != 1 {
                return Err(TraitError::Unsupported(
                    "newer configuration activation schema",
                ));
            }
            // Reopening an adopted schema is read-only and never repairs missing tables.
            let documents = read
                .open_table(DOCUMENTS)
                .map_err(|_| TraitError::Unsupported("missing configuration documents"))?;
            let stages = read
                .open_table(STAGE_REQUESTS)
                .map_err(|_| TraitError::Unsupported("missing configuration staging receipts"))?;
            let active = read
                .open_table(ACTIVE)
                .map_err(|_| TraitError::Unsupported("missing active configuration pointers"))?;
            let activations = read
                .open_table(ACTIVATIONS)
                .map_err(|_| TraitError::Unsupported("missing configuration activation history"))?;
            let requests = read.open_table(REQUESTS).map_err(|_| {
                TraitError::Unsupported("missing configuration activation receipts")
            })?;
            if documents.len().map_err(database_error)? > MAX_DOCUMENTS
                || stages.len().map_err(database_error)? > MAX_DOCUMENTS
                || active.len().map_err(database_error)? > MAX_IDENTITIES
                || activations.len().map_err(database_error)? > MAX_ACTIVATIONS
                || requests.len().map_err(database_error)? > MAX_ACTIVATIONS
            {
                return Err(TraitError::CapacityExceeded);
            }
            if documents.len().map_err(database_error)? != stages.len().map_err(database_error)?
                || activations.len().map_err(database_error)?
                    != requests.len().map_err(database_error)?
            {
                return Err(TraitError::Unsupported(
                    "configuration content and receipts disagree",
                ));
            }
            return Ok(Self { db });
        }
        if read.list_tables().map_err(database_error)?.any(|table| {
            matches!(
                table.name(),
                "configuration_documents_v1"
                    | "configuration_active_v1"
                    | "configuration_activations_v1"
                    | "configuration_activation_requests_v1"
                    | "configuration_stage_requests_v1"
            )
        }) {
            return Err(TraitError::Unsupported(
                "configuration tables without activation schema marker",
            ));
        }
        drop(schema);
        drop(read);
        let write = begin_write(&db)?;
        {
            let mut schema = write.open_table(SCHEMA).map_err(database_error)?;
            let version = schema
                .get("configuration_activation")
                .map_err(database_error)?
                .map(|v| v.value());
            if version.is_some_and(|v| v != 1) {
                return Err(TraitError::Unsupported(
                    "newer configuration activation schema",
                ));
            }
            write.open_table(DOCUMENTS).map_err(database_error)?;
            write.open_table(ACTIVE).map_err(database_error)?;
            write.open_table(ACTIVATIONS).map_err(database_error)?;
            write.open_table(REQUESTS).map_err(database_error)?;
            write.open_table(STAGE_REQUESTS).map_err(database_error)?;
            schema
                .insert("configuration_activation", 1)
                .map_err(database_error)?;
        }
        commit(write)?;
        Ok(Self { db })
    }
}
fn encode_document(value: &StagedConfiguration) -> Result<Vec<u8>, TraitError> {
    value.validate()?;
    let bytes = serde_json::to_vec(value)
        .map_err(|_| TraitError::InvalidInput("configuration encoding failed"))?;
    if bytes.len() > MAX_CONFIGURATION_BYTES + 16 * 1024 {
        return Err(TraitError::CapacityExceeded);
    }
    Ok(bytes)
}
fn decode_document(bytes: &[u8]) -> Result<StagedConfiguration, TraitError> {
    if bytes.len() > MAX_CONFIGURATION_BYTES + 16 * 1024 {
        return Err(TraitError::CapacityExceeded);
    }
    let staged: StagedConfiguration = serde_json::from_slice(bytes)
        .map_err(|_| TraitError::InvalidInput("invalid stored configuration"))?;
    staged.validate()?;
    Ok(staged)
}
fn decode_activation(bytes: &[u8]) -> Result<ActivationRecord, TraitError> {
    let value: ActivationRecord =
        decode(bytes).map_err(|_| TraitError::InvalidInput("invalid stored activation"))?;
    value.validate()?;
    Ok(value)
}
fn staged_at(
    table: &impl ReadableTable<(&'static str, u64), &'static [u8]>,
    id: &str,
    version: u64,
) -> Result<StagedConfiguration, TraitError> {
    let staged = table
        .get((id, version))
        .map_err(database_error)?
        .map(|v| decode_document(v.value()))
        .transpose()?
        .ok_or(TraitError::InvalidInput("missing staged configuration"))?;
    if staged.document.pricing.config_id != id || staged.document.pricing.version != version {
        return Err(TraitError::Conflict("stored configuration key mismatch"));
    }
    Ok(staged)
}
fn activation_at(
    table: &impl ReadableTable<(&'static str, u64), &'static [u8]>,
    id: &str,
    revision: u64,
) -> Result<ActivationRecord, TraitError> {
    let record = table
        .get((id, revision))
        .map_err(database_error)?
        .map(|v| decode_activation(v.value()))
        .transpose()?
        .ok_or(TraitError::InvalidInput("missing activation history"))?;
    if record.command.id.as_str() != id || record.revision.get() != revision {
        return Err(TraitError::Conflict("stored activation key mismatch"));
    }
    Ok(record)
}

#[async_trait]
impl ConfigurationRepository for RedbConfigurationRepository {
    async fn stage(&self, staged: &StagedConfiguration) -> Result<(), TraitError> {
        let bytes = encode_document(staged)?;
        let doc = &staged.document;
        let key = (doc.pricing.config_id.as_str(), doc.pricing.version);
        let write = begin_write(&self.db)?;
        {
            let mut documents = write.open_table(DOCUMENTS).map_err(database_error)?;
            let mut requests = write.open_table(STAGE_REQUESTS).map_err(database_error)?;
            if let Some(previous) = requests
                .get(staged.idempotency_key.as_str())
                .map_err(database_error)?
            {
                let retained: (String, u64) = decode(previous.value())?;
                if retained.0 != key.0 || retained.1 != key.1 {
                    return Err(TraitError::Conflict("staging request key reused"));
                }
            }
            if let Some(previous) = documents.get(key).map_err(database_error)? {
                return if encode_document(&decode_document(previous.value())?)? == bytes {
                    Ok(())
                } else {
                    Err(TraitError::Conflict(
                        "staged version has different content or metadata",
                    ))
                };
            }
            if documents.len().map_err(database_error)? >= MAX_DOCUMENTS
                || requests.len().map_err(database_error)? >= MAX_DOCUMENTS
            {
                return Err(TraitError::CapacityExceeded);
            }
            save_config_in_transaction(&write, &doc.pricing)?;
            documents
                .insert(key, bytes.as_slice())
                .map_err(database_error)?;
            let request = encode(&(key.0, key.1))?;
            requests
                .insert(staged.idempotency_key.as_str(), request.as_slice())
                .map_err(database_error)?;
        }
        commit(write)
    }
    async fn get_staged(
        &self,
        id: &SourceId,
        version: Version,
    ) -> Result<Option<StagedConfiguration>, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        let documents = read.open_table(DOCUMENTS).map_err(database_error)?;
        if documents
            .get((id.as_str(), version.get()))
            .map_err(database_error)?
            .is_none()
        {
            return Ok(None);
        }
        staged_at(&documents, id.as_str(), version.get()).map(Some)
    }
    async fn active(&self, id: &SourceId) -> Result<Option<ActiveConfiguration>, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        let pointers = read.open_table(ACTIVE).map_err(database_error)?;
        let Some(revision) = pointers
            .get(id.as_str())
            .map_err(database_error)?
            .map(|v| v.value())
        else {
            return Ok(None);
        };
        let activations = read.open_table(ACTIVATIONS).map_err(database_error)?;
        let documents = read.open_table(DOCUMENTS).map_err(database_error)?;
        let activation = activation_at(&activations, id.as_str(), revision)?;
        let staged = staged_at(
            &documents,
            id.as_str(),
            activation.command.configuration_version.get(),
        )?;
        let active = ActiveConfiguration { activation, staged };
        active.validate()?;
        Ok(Some(active))
    }
    async fn activate(&self, command: &ActivationCommand) -> Result<ActivationRecord, TraitError> {
        command.validate()?;
        let write = begin_write(&self.db)?;
        let record;
        {
            let mut requests = write.open_table(REQUESTS).map_err(database_error)?;
            if let Some(previous) = requests
                .get(command.idempotency_key.as_str())
                .map_err(database_error)?
            {
                let previous = decode_activation(previous.value())?;
                return if previous.command == *command {
                    Ok(previous)
                } else {
                    Err(TraitError::Conflict("activation request key reused"))
                };
            }
            let mut pointers = write.open_table(ACTIVE).map_err(database_error)?;
            let current = pointers
                .get(command.id.as_str())
                .map_err(database_error)?
                .map_or(0, |v| v.value());
            if current != command.expected_revision {
                return Err(TraitError::Conflict(
                    "active configuration revision changed",
                ));
            }
            let documents = write.open_table(DOCUMENTS).map_err(database_error)?;
            let staged = staged_at(
                &documents,
                command.id.as_str(),
                command.configuration_version.get(),
            )?;
            if command.activated_at < staged.staged_at {
                return Err(TraitError::InvalidInput("activation precedes staging"));
            }
            let mut activations = write.open_table(ACTIVATIONS).map_err(database_error)?;
            let previous = if current == 0 {
                None
            } else {
                Some(activation_at(&activations, command.id.as_str(), current)?)
            };
            if previous
                .as_ref()
                .is_some_and(|p| command.activated_at < p.command.activated_at)
            {
                return Err(TraitError::InvalidInput("activation time regressed"));
            }
            if command.mode == ReloadClass::Hot {
                let previous = previous.as_ref().ok_or(TraitError::Unsupported(
                    "initial activation requires restart mode",
                ))?;
                let prior = staged_at(
                    &documents,
                    command.id.as_str(),
                    previous.command.configuration_version.get(),
                )?;
                if configuration_diff(&prior.document, &staged.document)
                    .iter()
                    .any(|c| c.reload == ReloadClass::RestartRequired)
                {
                    return Err(TraitError::Unsupported(
                        "configuration changes require restart",
                    ));
                }
            }
            if let ActivationKind::Rollback { to_revision } = command.action {
                let target = activation_at(&activations, command.id.as_str(), to_revision.get())?;
                if to_revision.get() > current
                    || target.command.configuration_version != command.configuration_version
                {
                    return Err(TraitError::Conflict(
                        "rollback target differs from retained activation",
                    ));
                }
            }
            if activations.len().map_err(database_error)? >= MAX_ACTIVATIONS
                || requests.len().map_err(database_error)? >= MAX_ACTIVATIONS
                || (current == 0 && pointers.len().map_err(database_error)? >= MAX_IDENTITIES)
            {
                return Err(TraitError::CapacityExceeded);
            }
            record = ActivationRecord {
                revision: Version::new(current.checked_add(1).ok_or(TraitError::CapacityExceeded)?)
                    .map_err(|_| TraitError::CapacityExceeded)?,
                command: command.clone(),
                previous_version: previous.map(|p| p.command.configuration_version),
                configuration_digest: staged.document.digest()?,
            };
            let bytes = encode(&record)?;
            activations
                .insert(
                    (command.id.as_str(), record.revision.get()),
                    bytes.as_slice(),
                )
                .map_err(database_error)?;
            requests
                .insert(command.idempotency_key.as_str(), bytes.as_slice())
                .map_err(database_error)?;
            pointers
                .insert(command.id.as_str(), record.revision.get())
                .map_err(database_error)?;
        }
        commit(write)?;
        Ok(record)
    }
    async fn activation_for_request(
        &self,
        key: &RequestId,
    ) -> Result<Option<ActivationRecord>, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        read.open_table(REQUESTS)
            .map_err(database_error)?
            .get(key.as_str())
            .map_err(database_error)?
            .map(|v| decode_activation(v.value()))
            .transpose()
    }
    async fn get_activation(
        &self,
        id: &SourceId,
        revision: Version,
    ) -> Result<Option<ActivationRecord>, TraitError> {
        let read = self.db.begin_read().map_err(database_error)?;
        let table = read.open_table(ACTIVATIONS).map_err(database_error)?;
        if table
            .get((id.as_str(), revision.get()))
            .map_err(database_error)?
            .is_none()
        {
            return Ok(None);
        }
        activation_at(&table, id.as_str(), revision.get()).map(Some)
    }
    async fn history(
        &self,
        id: &SourceId,
        pagination: &Pagination,
    ) -> Result<Page<ActivationRecord>, TraitError> {
        pagination.validate()?;
        let read = self.db.begin_read().map_err(database_error)?;
        let table = read.open_table(ACTIVATIONS).map_err(database_error)?;
        if table.len().map_err(database_error)? > MAX_ACTIVATIONS {
            return Err(TraitError::CapacityExceeded);
        }
        let mut items = Vec::with_capacity(pagination.limit);
        let mut total = 0u64;
        for entry in table
            .range((id.as_str(), 1)..=(id.as_str(), u64::MAX))
            .map_err(database_error)?
        {
            let (key, bytes) = entry.map_err(database_error)?;
            let record = decode_activation(bytes.value())?;
            if record.command.id != *id || record.revision.get() != key.value().1 {
                return Err(TraitError::Conflict("activation history key mismatch"));
            }
            if total >= pagination.offset as u64 && items.len() < pagination.limit {
                items.push(record);
            }
            total += 1;
        }
        Ok(Page {
            items,
            total,
            offset: pagination.offset,
            limit: pagination.limit,
        })
    }
}
