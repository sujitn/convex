//! Wire-format encoding/decoding abstraction.

use bytes::Bytes;
use serde::{de::DeserializeOwned, Serialize};

use crate::error::TraitError;

/// Wire format encoding/decoding.
///
/// Implementations live in separate extension crates:
/// - `convex-ext-json` -> serde_json
pub trait Codec: Send + Sync {
    /// Encode value to bytes.
    fn encode<T: Serialize>(&self, value: &T) -> Result<Bytes, TraitError>;

    /// Decode bytes to value.
    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, TraitError>;

    /// Get content type header.
    fn content_type(&self) -> &'static str;
}
