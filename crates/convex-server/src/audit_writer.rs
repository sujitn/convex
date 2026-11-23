//! The audit store's one writer.
//!
//! The limit monitor and the RFQ desk both append to the audit store. A reader following
//! an entity passes the last ID it has read, so entries must commit in ID order: an ID
//! taken and committed after a larger one would be missed. So a writer takes IDs and
//! appends them under one lock, and IDs come from the store's last entry, read once.
//! Every attempt takes new IDs, so a write that failed after it committed never makes a
//! retry conflict.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use convex_engine::ports::storage::{AuditEntry, AuditStore};
use convex_engine::ports::TraitError;
use tokio::sync::{Mutex, MutexGuard};

/// The audit store and the next entry ID, shared by everything that appends.
pub(crate) struct AuditWriter {
    store: Arc<dyn AuditStore>,
    /// None until the store's last entry is read, and again once IDs run out.
    next_id: Mutex<Option<u64>>,
}

/// The writer, held: IDs taken and appends made through it commit in ID order.
pub(crate) struct HeldWriter<'a> {
    store: &'a Arc<dyn AuditStore>,
    next_id: MutexGuard<'a, Option<u64>>,
}

impl AuditWriter {
    pub(crate) fn new(store: Arc<dyn AuditStore>) -> Arc<Self> {
        Arc::new(Self {
            store,
            next_id: Mutex::new(None),
        })
    }

    /// The store, for reads and prunes, which take no ID.
    pub(crate) fn store(&self) -> &Arc<dyn AuditStore> {
        &self.store
    }

    /// Hold the writer, reading the store's last entry first if it has not been read.
    pub(crate) async fn hold(&self) -> Result<HeldWriter<'_>, TraitError> {
        let mut next_id = self.next_id.lock().await;
        if next_id.is_none() {
            let last = self.store.last_id().await?;
            *next_id = Some(last.map_or(1, |id| id.saturating_add(1)));
        }
        Ok(HeldWriter {
            store: &self.store,
            next_id,
        })
    }
}

impl HeldWriter<'_> {
    /// The next entry ID, taken whether or not its entry is written; none once IDs run
    /// out.
    pub(crate) fn take_id(&mut self) -> Option<u64> {
        let id = (*self.next_id)?;
        *self.next_id = id.checked_add(1);
        Some(id)
    }

    /// Append `entries`, whose IDs were taken from this hold, in one batch.
    pub(crate) async fn append_all(&self, entries: &[AuditEntry]) -> Result<(), TraitError> {
        self.store.append_all(entries).await
    }
}
