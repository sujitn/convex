//! # Convex Ext File
//!
//! File adapters for the Convex ports: reference import and repository, fixings,
//! sequenced quote replay (`FileQuoteReplay`), and optional snapshot and result
//! files. The server composes replay and fixings; its durable state is in redb.

#![warn(missing_docs)]
#![warn(clippy::all)]

mod fixing_repository;
mod reference_import;
mod reference_repository;
mod replay;
mod result_publisher;
mod snapshot_repository;

pub use fixing_repository::*;
pub use reference_import::*;
pub use reference_repository::*;
pub use replay::*;
pub use result_publisher::*;
pub use snapshot_repository::*;
