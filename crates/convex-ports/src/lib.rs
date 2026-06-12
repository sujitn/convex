//! # Convex Ports
//!
//! Hexagonal *port* traits for the Convex pricing engine.
//!
//! This crate contains ONLY trait definitions and their plain-data types, with
//! no runtime dependencies. Adapters (e.g. `convex-ext-file`, `convex-ext-redb`)
//! and the engine both depend *down* on this crate, so adapters never have to
//! pull in the engine to implement a port.
//!
//! ## Modules
//!
//! - [`market_data`]: market data sources (quotes, curves, vol, FX, inflation)
//! - [`reference_data`]: reference data sources (bonds, issuers, ratings, ETFs)
//! - [`storage`]: persistence (bonds, curves, configs, overrides, audit)
//! - [`output`]: output publishing
//! - [`config`]: engine / node configuration value types
//! - [`error`]: the shared [`error::TraitError`]

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod config;
pub mod error;
pub mod market_data;
pub mod output;
pub mod reference_data;
pub mod storage;

pub use error::TraitError;
