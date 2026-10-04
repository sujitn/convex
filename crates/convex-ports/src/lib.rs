//! # Convex Ports
//!
//! Hexagonal *port* definitions for the Convex pricing engine: the source,
//! storage, and output traits, plus the value types and lightweight helpers they
//! exchange (configs and their constructors, bond filters and their matching
//! logic, and the channel-receiver wrappers used by the streaming ports).
//!
//! The crate has value/serialization dependencies and uses `tokio` solely for
//! the `sync::broadcast` receiver types in streaming ports. There is no async
//! executor, I/O implementation or storage backend here. Adapters
//! (e.g. `convex-ext-file`, `convex-ext-redb`) and the
//! engine both depend *down* on this crate, so an adapter never has to pull in
//! the engine to implement a port.
//!
//! ## Modules
//!
//! - [`market_data`] — market data sources (quotes, curves, vol, FX, inflation)
//! - [`reference_data`] — reference terms and issuer, rating and ETF sources
//! - [`reference_repository`] — atomic versioned reference commits
//! - [`storage`] — persistence (bonds, curves, configs, overrides, audit)
//! - [`output`] — output publishing
//! - [`config`] — engine / node configuration value types
//! - [`error`] — the shared [`error::TraitError`]

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod config;
pub mod error;
pub mod market_data;
pub mod output;
pub mod quote_event;
pub mod reference_data;
pub mod reference_repository;
pub mod storage;

pub use error::TraitError;
