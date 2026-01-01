//! # Convex Traits
//!
//! Trait definitions for the Convex pricing engine.
//!
//! This crate contains ONLY trait definitions with ZERO runtime dependencies.
//! All implementations are in separate extension crates.
//!
//! ## Module Structure
//!
//! - [`market_data`]: Traits for market data sources (quotes, curves, vol, FX, inflation)
//! - [`reference_data`]: Traits for reference data sources (bonds, issuers, ratings)
//! - [`storage`]: Traits for persistence (bonds, curves, configs)
//! - [`transport`]: Traits for communication (REST, gRPC, WebSocket, Kafka)
//! - [`config`]: Traits for configuration sources
//! - [`output`]: Traits for output publishing
//! - [`coordination`]: Traits for distributed coordination (service registry, partitioning, leader election)
//!
//! ## Dependency Injection
//!
//! The pricing engine uses these traits via dependency injection:
//!
//! ```ignore
//! PricingEngineBuilder::new()
//!     .with_market_data(impl MarketDataProvider)
//!     .with_reference_data(impl ReferenceDataProvider)
//!     .with_storage(impl Storage)
//!     .with_config(impl ConfigSource)
//!     .with_output(impl OutputPublisher)
//!     .build()
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod config;
pub mod coordination;
pub mod error;
pub mod ids;
pub mod market_data;
pub mod output;
pub mod reference_data;
pub mod storage;
pub mod transport;

// Re-export commonly used types
pub use error::TraitError;
pub use ids::*;
