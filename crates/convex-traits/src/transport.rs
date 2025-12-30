//! Transport abstraction traits.
//!
//! These traits define protocol-agnostic communication:
//! - [`Transport`]: Request/response and streaming communication
//! - [`Codec`]: Serialization/deserialization abstraction
//! - [`StorageTransport`]: Remote storage backend transport
//! - [`CacheTransport`]: Distributed cache transport
//!
//! Transport implementations are EXTENSIONS (e.g., REST, gRPC, WebSocket, Kafka).

use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;
use serde::{de::DeserializeOwned, Serialize};
use std::pin::Pin;
use std::time::Duration;

use crate::error::TraitError;

// =============================================================================
// TRANSPORT TRAIT
// =============================================================================

/// Transport-agnostic communication layer.
///
/// Implementations live in separate extension crates:
/// - `convex-ext-rest` -> reqwest
/// - `convex-ext-grpc` -> tonic
/// - `convex-ext-ws` -> tokio-tungstenite
/// - `convex-ext-kafka` -> rdkafka
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send request, receive response (request-response pattern).
    async fn request<Req, Res>(&self, endpoint: &str, request: &Req) -> Result<Res, TraitError>
    where
        Req: Serialize + Send + Sync,
        Res: DeserializeOwned;

    /// Send request, receive streaming response.
    async fn request_stream<Req, Res>(
        &self,
        endpoint: &str,
        request: &Req,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Res, TraitError>> + Send>>, TraitError>
    where
        Req: Serialize + Send + Sync,
        Res: DeserializeOwned + Send + 'static;

    /// Subscribe to a topic/stream (push-based).
    async fn subscribe<Res>(
        &self,
        topic: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Res, TraitError>> + Send>>, TraitError>
    where
        Res: DeserializeOwned + Send + 'static;

    /// Publish message (fire-and-forget or ack).
    async fn publish<Msg>(&self, topic: &str, message: &Msg) -> Result<(), TraitError>
    where
        Msg: Serialize + Send + Sync;
}

// =============================================================================
// CODEC TRAIT
// =============================================================================

/// Wire format encoding/decoding.
///
/// Implementations live in separate extension crates:
/// - `convex-ext-json` -> serde_json
/// - `convex-ext-msgpack` -> rmp-serde
/// - `convex-ext-protobuf` -> prost
pub trait Codec: Send + Sync {
    /// Encode value to bytes.
    fn encode<T: Serialize>(&self, value: &T) -> Result<Bytes, TraitError>;

    /// Decode bytes to value.
    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, TraitError>;

    /// Get content type header.
    fn content_type(&self) -> &'static str;
}

// =============================================================================
// STORAGE TRANSPORT TRAIT
// =============================================================================

/// Transport for remote storage backends.
///
/// Implementations:
/// - PostgreSQL (sqlx)
/// - Redis (redis-rs)
/// - TiKV (tikv-client)
/// - S3 (aws-sdk-s3)
#[async_trait]
pub trait StorageTransport: Send + Sync {
    /// Get value by key.
    async fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, TraitError>;

    /// Put value.
    async fn put(&self, table: &str, key: &[u8], value: &[u8]) -> Result<(), TraitError>;

    /// Delete key.
    async fn delete(&self, table: &str, key: &[u8]) -> Result<bool, TraitError>;

    /// Scan keys with prefix.
    async fn scan(
        &self,
        table: &str,
        prefix: &[u8],
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, TraitError>;

    /// Batch put.
    async fn batch_put(&self, table: &str, items: &[(&[u8], &[u8])]) -> Result<(), TraitError>;

    /// Check if key exists.
    async fn exists(&self, table: &str, key: &[u8]) -> Result<bool, TraitError>;
}

// =============================================================================
// CACHE TRANSPORT TRAIT
// =============================================================================

/// Transport for distributed cache.
///
/// Implementations:
/// - Redis (redis-rs)
/// - Memcached (memcache)
/// - In-memory (DashMap)
#[async_trait]
pub trait CacheTransport: Send + Sync {
    /// Get cached value.
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, TraitError>;

    /// Set cached value with optional TTL.
    async fn set(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> Result<(), TraitError>;

    /// Delete cached value.
    async fn delete(&self, key: &str) -> Result<bool, TraitError>;

    /// Publish to a channel (pub/sub).
    async fn publish(&self, channel: &str, message: &[u8]) -> Result<(), TraitError>;

    /// Subscribe to a channel.
    async fn subscribe(
        &self,
        channel: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Vec<u8>, TraitError>> + Send>>, TraitError>;
}

// =============================================================================
// API SERVER TRAIT
// =============================================================================

/// Request object for API handlers.
#[derive(Debug, Clone)]
pub struct ApiRequest {
    /// Request path
    pub path: String,
    /// HTTP method (for REST)
    pub method: String,
    /// Request body
    pub body: Option<Bytes>,
    /// Query parameters
    pub query: Vec<(String, String)>,
    /// Headers
    pub headers: Vec<(String, String)>,
}

/// Response object from API handlers.
#[derive(Debug, Clone)]
pub struct ApiResponse {
    /// Status code
    pub status: u16,
    /// Response body
    pub body: Option<Bytes>,
    /// Headers
    pub headers: Vec<(String, String)>,
}

impl ApiResponse {
    /// Create a successful JSON response.
    pub fn json<T: Serialize>(value: &T) -> Result<Self, TraitError> {
        let json = serde_json::to_vec(value)
            .map_err(|e| TraitError::SerializationError(e.to_string()))?;
        Ok(Self {
            status: 200,
            body: Some(Bytes::from(json)),
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
        })
    }

    /// Create an error response.
    pub fn error(status: u16, message: &str) -> Self {
        Self {
            status,
            body: Some(Bytes::from(format!(r#"{{"error": "{}"}}"#, message))),
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
        }
    }
}

/// Request handler trait.
#[async_trait]
pub trait RequestHandler: Send + Sync {
    /// Handle a request.
    async fn handle(&self, request: ApiRequest) -> ApiResponse;
}
