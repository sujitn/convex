//! Convex Storage Layer
//!
//! This crate provides storage adapters and persistence functionality for the
//! Convex fixed income analytics library. It supports multiple storage backends
//! including embedded databases (redb) and in-memory storage for testing.
//!
//! # Features
//!
//! - **Security Master Storage**: Store and query bond reference data
//! - **Curve Snapshots**: Point-in-time yield curve persistence
//! - **Quote History**: Time-series storage for market quotes
//! - **Versioned Configuration**: Configuration with audit trails
//! - **Multiple Backends**: redb (default) and in-memory adapters
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_storage::{RedbStorage, StorageAdapter, SecurityMaster};
//!
//! // Create or open a database
//! let storage = RedbStorage::open("./convex-data.redb")?;
//!
//! // Store a security
//! let security = SecurityMaster::builder("AAPL-2025", "Apple Inc")
//!     .isin("US0378331005")
//!     .currency(Currency::USD)
//!     .coupon_rate(dec!(0.045))
//!     .build();
//! storage.store_security(&security)?;
//!
//! // Retrieve it later
//! let retrieved = storage.get_security("AAPL-2025")?;
//! ```
//!
//! # Storage Backends
//!
//! ## RedbStorage (Default)
//!
//! Uses [redb](https://crates.io/crates/redb), a pure-Rust embedded database
//! with ACID transactions. Suitable for single-process applications.
//!
//! ## InMemoryStorage
//!
//! A simple in-memory implementation for testing and development.
//! Data is not persisted across restarts.

#![warn(missing_docs)]
#![warn(clippy::all)]
#![deny(unsafe_code)]

mod adapter;
mod error;
mod memory;
mod redb;
mod types;

// Re-export core types
pub use adapter::{SecurityFilter, StorageAdapter, StorageStats};
pub use error::{StorageError, StorageResult};
pub use memory::InMemoryStorage;
pub use redb::RedbStorage;
pub use types::{
    ConfigRecord, CurveInput, CurvePoint, CurveSnapshot, QuoteCondition, QuoteRecord,
    SecurityMaster, SecurityMasterBuilder, SecurityStatus, SecurityType, TimeRange, Versioned,
};

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::adapter::{SecurityFilter, StorageAdapter, StorageStats};
    pub use crate::error::{StorageError, StorageResult};
    pub use crate::memory::InMemoryStorage;
    pub use crate::redb::RedbStorage;
    pub use crate::types::{
        CurveSnapshot, QuoteRecord, SecurityMaster, SecurityStatus, SecurityType, TimeRange,
    };
}
