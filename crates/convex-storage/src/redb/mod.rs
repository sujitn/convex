//! Redb storage backend.
//!
//! This module provides the [`RedbStorage`] adapter that uses [redb](https://crates.io/crates/redb),
//! a pure-Rust embedded database with ACID transactions.

mod storage;

pub use storage::RedbStorage;
