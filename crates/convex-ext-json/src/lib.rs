//! # Convex Ext JSON
//!
//! JSON codec implementation for the Convex pricing engine transport layer.
//!
//! This crate provides a JSON implementation of the [`Codec`] trait for
//! REST and WebSocket communication.

#![warn(missing_docs)]
#![warn(clippy::all)]

use bytes::Bytes;
use serde::{de::DeserializeOwned, Serialize};

use convex_traits::error::TraitError;
use convex_traits::transport::Codec;

/// JSON codec using serde_json.
#[derive(Debug, Clone, Default)]
pub struct JsonCodec;

impl JsonCodec {
    /// Create a new JSON codec.
    pub fn new() -> Self {
        Self
    }
}

impl Codec for JsonCodec {
    fn encode<T: Serialize>(&self, value: &T) -> Result<Bytes, TraitError> {
        serde_json::to_vec(value)
            .map(Bytes::from)
            .map_err(|e| TraitError::SerializationError(e.to_string()))
    }

    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, TraitError> {
        serde_json::from_slice(bytes).map_err(|e| TraitError::ParseError(e.to_string()))
    }

    fn content_type(&self) -> &'static str {
        "application/json"
    }
}

/// Pretty-printing JSON codec (for debugging).
#[derive(Debug, Clone, Default)]
pub struct PrettyJsonCodec;

impl PrettyJsonCodec {
    /// Create a new pretty JSON codec.
    pub fn new() -> Self {
        Self
    }
}

impl Codec for PrettyJsonCodec {
    fn encode<T: Serialize>(&self, value: &T) -> Result<Bytes, TraitError> {
        serde_json::to_vec_pretty(value)
            .map(Bytes::from)
            .map_err(|e| TraitError::SerializationError(e.to_string()))
    }

    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, TraitError> {
        serde_json::from_slice(bytes).map_err(|e| TraitError::ParseError(e.to_string()))
    }

    fn content_type(&self) -> &'static str {
        "application/json"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestStruct {
        name: String,
        value: i32,
    }

    #[test]
    fn test_json_codec_roundtrip() {
        let codec = JsonCodec::new();

        let original = TestStruct {
            name: "test".to_string(),
            value: 42,
        };

        let encoded = codec.encode(&original).unwrap();
        let decoded: TestStruct = codec.decode(&encoded).unwrap();

        assert_eq!(original, decoded);
    }

    #[test]
    fn test_content_type() {
        let codec = JsonCodec::new();
        assert_eq!(codec.content_type(), "application/json");
    }
}
