#![allow(dead_code)]
use async_trait::async_trait;
use convex_core::contracts::*;
use convex_ports::{
    configuration::*,
    storage::{Page, Pagination},
    TraitError,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
pub struct FaultRepository {
    pub inner: Arc<dyn ConfigurationRepository>,
    pub unknown_stage: AtomicBool,
    pub unknown_activate: AtomicBool,
    pub fail_activate: AtomicBool,
    pub corrupt_receipt: AtomicBool,
}
impl FaultRepository {
    pub fn new(inner: Arc<dyn ConfigurationRepository>) -> Self {
        Self {
            inner,
            unknown_stage: AtomicBool::new(false),
            unknown_activate: AtomicBool::new(false),
            fail_activate: AtomicBool::new(false),
            corrupt_receipt: AtomicBool::new(false),
        }
    }
}
#[async_trait]
impl ConfigurationRepository for FaultRepository {
    async fn stage(&self, staged: &StagedConfiguration) -> Result<(), TraitError> {
        self.inner.stage(staged).await?;
        if self.unknown_stage.swap(false, Ordering::SeqCst) {
            Err(TraitError::UnknownOutcome)
        } else {
            Ok(())
        }
    }
    async fn get_staged(
        &self,
        id: &SourceId,
        version: Version,
    ) -> Result<Option<StagedConfiguration>, TraitError> {
        self.inner.get_staged(id, version).await
    }
    async fn active(&self, id: &SourceId) -> Result<Option<ActiveConfiguration>, TraitError> {
        self.inner.active(id).await
    }
    async fn activate(&self, command: &ActivationCommand) -> Result<ActivationRecord, TraitError> {
        if self.fail_activate.swap(false, Ordering::SeqCst) {
            return Err(TraitError::SourceNotAvailable(
                "PRIVATE_ADAPTER_MARKER".into(),
            ));
        }
        let record = self.inner.activate(command).await?;
        if self.unknown_activate.swap(false, Ordering::SeqCst) {
            Err(TraitError::UnknownOutcome)
        } else {
            Ok(record)
        }
    }
    async fn activation_for_request(
        &self,
        key: &RequestId,
    ) -> Result<Option<ActivationRecord>, TraitError> {
        let mut record = self.inner.activation_for_request(key).await?;
        if self.corrupt_receipt.load(Ordering::SeqCst) {
            if let Some(record) = &mut record {
                record.configuration_digest = "0".repeat(64);
            }
        }
        Ok(record)
    }
    async fn get_activation(
        &self,
        id: &SourceId,
        version: Version,
    ) -> Result<Option<ActivationRecord>, TraitError> {
        self.inner.get_activation(id, version).await
    }
    async fn history(
        &self,
        id: &SourceId,
        pagination: &Pagination,
    ) -> Result<Page<ActivationRecord>, TraitError> {
        self.inner.history(id, pagination).await
    }
}
