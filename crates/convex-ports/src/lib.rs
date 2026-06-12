//! # Convex Ports
//!
//! Hexagonal *port* definitions for the Convex pricing engine: the source,
//! storage, and output traits, plus the value types and lightweight helpers they
//! exchange (configs and their constructors, bond filters and their matching
//! logic, and the channel-receiver wrappers used by the streaming ports).
//!
//! The crate stays deliberately lightweight: beyond `convex-core` its only
//! dependency is `tokio`, used solely for the `sync::broadcast` receiver types in
//! the streaming ports -- there is no async executor, no I/O, and no storage
//! backend here. Adapters (e.g. `convex-ext-file`, `convex-ext-redb`) and the
//! engine both depend *down* on this crate, so an adapter never has to pull in
//! the engine to implement a port.
//!
//! ## Modules
//!
//! - [`market_data`] — market data sources (quotes, curves, vol, FX, inflation)
//! - [`reference_data`] — reference data sources (bonds, issuers, ratings, ETFs)
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
pub mod reference_data;
pub mod storage;

pub use error::TraitError;
