//! Coordination traits for distributed deployments.
//!
//! These traits define the interface for service discovery, partition management,
//! and leader election in multi-replica Convex deployments.
//!
//! ## Overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                        COORDINATION LAYER                                   │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                     │
//! │  │   Service    │  │   Partition  │  │    Leader    │                     │
//! │  │  Discovery   │  │   Registry   │  │   Election   │                     │
//! │  │   (etcd)     │  │              │  │              │                     │
//! │  └──────────────┘  └──────────────┘  └──────────────┘                     │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Implementations
//!
//! Implementations live in separate extension crates:
//! - `convex-ext-etcd` -> etcd-based coordination
//! - `convex-ext-consul` -> Consul-based coordination
//! - `convex-ext-k8s` -> Kubernetes-native coordination
//!
//! ## Usage
//!
//! ```ignore
//! let engine = PricingEngineBuilder::new()
//!     .with_service_registry(EtcdServiceRegistry::new(&config)?)
//!     .with_partition_registry(EtcdPartitionRegistry::new(&config)?)
//!     .with_leader_election(EtcdLeaderElection::new(&config)?)
//!     .build()?;
//! ```

use async_trait::async_trait;
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

use crate::ids::InstrumentId;

// =============================================================================
// ERROR TYPES
// =============================================================================

/// Errors that can occur during service registry operations.
#[derive(Debug, Error)]
pub enum RegistryError {
    /// Failed to connect to coordination backend.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Registration failed.
    #[error("Registration failed: {0}")]
    RegistrationFailed(String),

    /// Instance not found.
    #[error("Instance not found: {0}")]
    InstanceNotFound(String),

    /// Watch operation failed.
    #[error("Watch failed: {0}")]
    WatchFailed(String),

    /// Heartbeat failed.
    #[error("Heartbeat failed: {0}")]
    HeartbeatFailed(String),

    /// Session expired.
    #[error("Session expired")]
    SessionExpired,

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Errors that can occur during partition operations.
#[derive(Debug, Error)]
pub enum PartitionError {
    /// Failed to claim partitions.
    #[error("Claim failed: {0}")]
    ClaimFailed(String),

    /// Partition already owned by another replica.
    #[error("Partition {partition} already owned by {owner}")]
    AlreadyOwned {
        /// The partition ID.
        partition: u32,
        /// The current owner.
        owner: String,
    },

    /// No replica found for partition.
    #[error("No replica for partition {0}")]
    NoReplicaForPartition(u32),

    /// Rebalance in progress.
    #[error("Rebalance in progress")]
    RebalanceInProgress,

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Errors that can occur during leader election.
#[derive(Debug, Error)]
pub enum ElectionError {
    /// Campaign failed.
    #[error("Campaign failed: {0}")]
    CampaignFailed(String),

    /// Not the leader.
    #[error("Not the leader")]
    NotLeader,

    /// Leader unknown.
    #[error("Leader unknown")]
    LeaderUnknown,

    /// Lease expired.
    #[error("Lease expired")]
    LeaseExpired,

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

// =============================================================================
// INSTANCE TYPES
// =============================================================================

/// Status of a service instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstanceStatus {
    /// Instance is starting up.
    Starting,
    /// Instance is ready to serve requests.
    Ready,
    /// Instance is draining (shutting down gracefully).
    Draining,
    /// Instance is unhealthy.
    Unhealthy,
}

impl Default for InstanceStatus {
    fn default() -> Self {
        Self::Starting
    }
}

/// Information about a service instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    /// Unique instance identifier.
    pub instance_id: String,
    /// Service name (e.g., "convex-engine").
    pub service_name: String,
    /// Host address.
    pub host: String,
    /// Port number.
    pub port: u16,
    /// Partition ID if applicable.
    pub partition_id: Option<u32>,
    /// Additional metadata.
    pub metadata: HashMap<String, String>,
    /// Current status.
    pub status: InstanceStatus,
    /// Last heartbeat timestamp (Unix epoch seconds).
    pub last_heartbeat: i64,
}

impl InstanceInfo {
    /// Create a new instance info.
    pub fn new(instance_id: &str, service_name: &str, host: &str, port: u16) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        Self {
            instance_id: instance_id.to_string(),
            service_name: service_name.to_string(),
            host: host.to_string(),
            port,
            partition_id: None,
            metadata: HashMap::new(),
            status: InstanceStatus::Starting,
            last_heartbeat: timestamp,
        }
    }

    /// Set the partition ID.
    pub fn with_partition(mut self, partition_id: u32) -> Self {
        self.partition_id = Some(partition_id);
        self
    }

    /// Set the status.
    pub fn with_status(mut self, status: InstanceStatus) -> Self {
        self.status = status;
        self
    }

    /// Add metadata.
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Get the full address (host:port).
    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

// =============================================================================
// PARTITION TYPES
// =============================================================================

/// Strategy for partitioning bonds across replicas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartitionStrategy {
    /// Partition by currency.
    ByCurrency,
    /// Partition by issuer type (govt, corporate, municipal).
    ByIssuerType,
    /// Partition by hash of instrument ID.
    HashBased,
    /// Manual assignment via configuration.
    Manual,
}

impl Default for PartitionStrategy {
    fn default() -> Self {
        Self::HashBased
    }
}

/// Partition assignment for a replica.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PartitionAssignment {
    /// Currencies this partition handles.
    pub currencies: Option<Vec<String>>,
    /// Issuer types this partition handles.
    pub issuer_types: Option<Vec<String>>,
    /// Explicit instrument IDs.
    pub instrument_ids: Option<Vec<String>>,
}

/// Configuration for partitioning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionConfig {
    /// This replica's partition ID.
    pub partition_id: u32,
    /// Total number of partitions.
    pub total_partitions: u32,
    /// Partition strategy.
    pub strategy: PartitionStrategy,
    /// Partition assignment (for manual/by-currency/by-issuer strategies).
    pub assignment: Option<PartitionAssignment>,
}

impl PartitionConfig {
    /// Create a new partition config.
    pub fn new(partition_id: u32, total_partitions: u32) -> Self {
        Self {
            partition_id,
            total_partitions,
            strategy: PartitionStrategy::HashBased,
            assignment: None,
        }
    }

    /// Set the strategy.
    pub fn with_strategy(mut self, strategy: PartitionStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set the assignment.
    pub fn with_assignment(mut self, assignment: PartitionAssignment) -> Self {
        self.assignment = Some(assignment);
        self
    }

    /// Check if this partition owns an instrument (by hash).
    pub fn owns_by_hash(&self, instrument_id: &str) -> bool {
        let hash = Self::hash_string(instrument_id);
        hash % self.total_partitions == self.partition_id
    }

    /// Simple string hash function.
    fn hash_string(s: &str) -> u32 {
        let mut hash: u32 = 0;
        for byte in s.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(byte as u32);
        }
        hash
    }
}

impl Default for PartitionConfig {
    fn default() -> Self {
        Self::new(0, 1)
    }
}

// =============================================================================
// LEADER ELECTION TYPES
// =============================================================================

/// Status of leader election.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaderStatus {
    /// This instance is the leader.
    Leader {
        /// Lease ID for the leadership.
        lease_id: u64,
    },
    /// This instance is a follower.
    Follower {
        /// The current leader's ID.
        leader_id: String,
    },
    /// No leader currently elected.
    NoLeader,
}

impl LeaderStatus {
    /// Returns true if this instance is the leader.
    pub fn is_leader(&self) -> bool {
        matches!(self, LeaderStatus::Leader { .. })
    }
}

// =============================================================================
// GOSSIP TYPES
// =============================================================================

/// Load metrics for gossip protocol.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoadMetrics {
    /// Number of bonds managed.
    pub bonds_count: u64,
    /// Updates processed per second.
    pub updates_per_second: f64,
    /// P99 latency in microseconds.
    pub latency_p99_us: u64,
    /// Memory used in megabytes.
    pub memory_used_mb: u64,
}

/// State shared via gossip protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipState {
    /// Instance ID.
    pub instance_id: String,
    /// Owned partitions.
    pub partitions: Vec<u32>,
    /// Health status.
    pub status: InstanceStatus,
    /// Load metrics.
    pub load: LoadMetrics,
    /// Version vector for conflict resolution.
    pub version: u64,
}

impl GossipState {
    /// Create a new gossip state.
    pub fn new(instance_id: &str) -> Self {
        Self {
            instance_id: instance_id.to_string(),
            partitions: Vec::new(),
            status: InstanceStatus::Starting,
            load: LoadMetrics::default(),
            version: 0,
        }
    }

    /// Increment version.
    pub fn increment_version(&mut self) {
        self.version += 1;
    }
}

// =============================================================================
// REBALANCE TYPES
// =============================================================================

/// Event type for rebalancing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RebalanceEvent {
    /// Partitions assigned to this replica.
    Assigned {
        /// List of partition IDs assigned.
        partitions: Vec<u32>,
    },
    /// Partitions revoked from this replica.
    Revoked {
        /// List of partition IDs revoked.
        partitions: Vec<u32>,
    },
    /// Rebalance started.
    Started,
    /// Rebalance completed.
    Completed,
}

// =============================================================================
// WATCHER TYPES
// =============================================================================

/// Watcher for instance changes.
pub type InstanceWatcher = Pin<Box<dyn Stream<Item = Result<Vec<InstanceInfo>, RegistryError>> + Send>>;

/// Watcher for rebalance events.
pub type RebalanceWatcher = Pin<Box<dyn Stream<Item = Result<RebalanceEvent, PartitionError>> + Send>>;

/// Watcher for leadership changes.
pub type LeaderWatcher = Pin<Box<dyn Stream<Item = Result<LeaderStatus, ElectionError>> + Send>>;

// =============================================================================
// SERVICE REGISTRY TRAIT
// =============================================================================

/// Service discovery and registration.
///
/// This trait defines the interface for service discovery in a distributed
/// Convex deployment. Implementations may use etcd, Consul, Kubernetes, etc.
///
/// # Example
///
/// ```ignore
/// let registry = EtcdServiceRegistry::new(&config)?;
///
/// // Register this instance
/// let info = InstanceInfo::new("engine-0", "convex-engine", "10.0.0.1", 8080);
/// registry.register(&info).await?;
///
/// // Get all healthy instances
/// let instances = registry.get_instances("convex-engine").await?;
///
/// // Send heartbeats
/// loop {
///     registry.heartbeat("engine-0").await?;
///     tokio::time::sleep(Duration::from_secs(5)).await;
/// }
/// ```
#[async_trait]
pub trait ServiceRegistry: Send + Sync {
    /// Register this instance with the service registry.
    async fn register(&self, instance: &InstanceInfo) -> Result<(), RegistryError>;

    /// Deregister on shutdown.
    async fn deregister(&self, instance_id: &str) -> Result<(), RegistryError>;

    /// Get all healthy instances of a service.
    async fn get_instances(&self, service_name: &str) -> Result<Vec<InstanceInfo>, RegistryError>;

    /// Get a specific instance by ID.
    async fn get_instance(&self, instance_id: &str) -> Result<Option<InstanceInfo>, RegistryError>;

    /// Watch for instance changes.
    fn watch(&self, service_name: &str) -> Result<InstanceWatcher, RegistryError>;

    /// Send heartbeat to indicate this instance is healthy.
    async fn heartbeat(&self, instance_id: &str) -> Result<(), RegistryError>;

    /// Update instance status.
    async fn update_status(
        &self,
        instance_id: &str,
        status: InstanceStatus,
    ) -> Result<(), RegistryError>;
}

// =============================================================================
// PARTITION REGISTRY TRAIT
// =============================================================================

/// Tracks which replica owns which partitions.
///
/// This trait manages partition assignment and rebalancing across replicas.
///
/// # Example
///
/// ```ignore
/// let registry = EtcdPartitionRegistry::new(&config)?;
///
/// // Claim partitions for this replica
/// let claimed = registry.claim_partitions(&[0, 1, 2]).await?;
///
/// // Find which replica handles a specific instrument
/// let replica = registry.find_replica(&instrument_id).await?;
/// ```
#[async_trait]
pub trait PartitionRegistry: Send + Sync {
    /// Claim partitions for this replica.
    ///
    /// Returns the list of successfully claimed partitions.
    async fn claim_partitions(&self, partitions: &[u32]) -> Result<Vec<u32>, PartitionError>;

    /// Release partitions (on shutdown or rebalance).
    async fn release_partitions(&self, partitions: &[u32]) -> Result<(), PartitionError>;

    /// Get current partition assignments.
    ///
    /// Returns a map from partition ID to instance ID.
    async fn get_assignments(&self) -> Result<HashMap<u32, String>, PartitionError>;

    /// Watch for rebalance events.
    fn watch_rebalance(&self) -> Result<RebalanceWatcher, PartitionError>;

    /// Find which replica handles a specific instrument.
    async fn find_replica(
        &self,
        instrument_id: &InstrumentId,
    ) -> Result<Option<String>, PartitionError>;

    /// Get all partitions owned by this instance.
    async fn get_owned_partitions(&self, instance_id: &str) -> Result<Vec<u32>, PartitionError>;

    /// Trigger a rebalance (leader only).
    async fn trigger_rebalance(&self) -> Result<(), PartitionError>;
}

// =============================================================================
// LEADER ELECTION TRAIT
// =============================================================================

/// Leader election for coordinated operations.
///
/// This trait provides leader election for operations that require
/// single-writer semantics, such as partition rebalancing.
///
/// # Example
///
/// ```ignore
/// let election = EtcdLeaderElection::new(&config)?;
///
/// // Attempt to become leader
/// let status = election.campaign("engine-0").await?;
///
/// if status.is_leader() {
///     // Perform leader-only operations
///     partition_registry.trigger_rebalance().await?;
/// }
///
/// // Watch for leadership changes
/// let mut watcher = election.watch()?;
/// while let Some(status) = watcher.next().await {
///     match status? {
///         LeaderStatus::Leader { .. } => println!("We are now the leader"),
///         LeaderStatus::Follower { leader_id } => println!("Following {}", leader_id),
///         LeaderStatus::NoLeader => println!("No leader elected"),
///     }
/// }
/// ```
#[async_trait]
pub trait LeaderElection: Send + Sync {
    /// Attempt to become leader.
    async fn campaign(&self, candidate_id: &str) -> Result<LeaderStatus, ElectionError>;

    /// Check if we are currently the leader.
    async fn is_leader(&self) -> bool;

    /// Resign leadership.
    async fn resign(&self) -> Result<(), ElectionError>;

    /// Get current leader.
    async fn get_leader(&self) -> Result<Option<String>, ElectionError>;

    /// Watch for leadership changes.
    fn watch(&self) -> Result<LeaderWatcher, ElectionError>;

    /// Keep the leadership lease alive.
    ///
    /// Call this periodically to maintain leadership.
    async fn keep_alive(&self) -> Result<(), ElectionError>;
}

// =============================================================================
// COORDINATION CONFIGURATION
// =============================================================================

/// Configuration for coordination services.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationConfig {
    /// Coordination backend type.
    pub backend: CoordinationBackend,
    /// Endpoints for the coordination service.
    pub endpoints: Vec<String>,
    /// Session TTL in seconds.
    pub session_ttl_seconds: u64,
    /// Enable TLS.
    pub tls_enabled: bool,
    /// Username for authentication (optional).
    pub username: Option<String>,
    /// Password for authentication (optional).
    pub password: Option<String>,
}

/// Coordination backend type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinationBackend {
    /// etcd backend.
    Etcd,
    /// Consul backend.
    Consul,
    /// Kubernetes backend (uses K8s API).
    Kubernetes,
    /// In-memory backend (for testing).
    InMemory,
}

impl Default for CoordinationConfig {
    fn default() -> Self {
        Self {
            backend: CoordinationBackend::InMemory,
            endpoints: vec!["localhost:2379".to_string()],
            session_ttl_seconds: 30,
            tls_enabled: false,
            username: None,
            password: None,
        }
    }
}

// =============================================================================
// NEVER STREAM (HELPER FOR EMPTY IMPLEMENTATIONS)
// =============================================================================

/// A stream that never yields any items.
///
/// Used by empty implementations to provide a valid watcher that never produces events.
struct NeverStream<T>(std::marker::PhantomData<T>);

impl<T> NeverStream<T> {
    fn new() -> Self {
        NeverStream(std::marker::PhantomData)
    }
}

impl<T> Stream for NeverStream<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Pending
    }
}

// =============================================================================
// EMPTY IMPLEMENTATIONS (FOR TESTING)
// =============================================================================

/// Empty service registry that does nothing.
///
/// Useful for single-process deployments where coordination is not needed.
#[derive(Debug, Default)]
pub struct EmptyServiceRegistry;

#[async_trait]
impl ServiceRegistry for EmptyServiceRegistry {
    async fn register(&self, _instance: &InstanceInfo) -> Result<(), RegistryError> {
        Ok(())
    }

    async fn deregister(&self, _instance_id: &str) -> Result<(), RegistryError> {
        Ok(())
    }

    async fn get_instances(&self, _service_name: &str) -> Result<Vec<InstanceInfo>, RegistryError> {
        Ok(Vec::new())
    }

    async fn get_instance(&self, _instance_id: &str) -> Result<Option<InstanceInfo>, RegistryError> {
        Ok(None)
    }

    fn watch(&self, _service_name: &str) -> Result<InstanceWatcher, RegistryError> {
        Ok(Box::pin(NeverStream::new()))
    }

    async fn heartbeat(&self, _instance_id: &str) -> Result<(), RegistryError> {
        Ok(())
    }

    async fn update_status(
        &self,
        _instance_id: &str,
        _status: InstanceStatus,
    ) -> Result<(), RegistryError> {
        Ok(())
    }
}

/// Empty partition registry that does nothing.
#[derive(Debug, Default)]
pub struct EmptyPartitionRegistry;

#[async_trait]
impl PartitionRegistry for EmptyPartitionRegistry {
    async fn claim_partitions(&self, partitions: &[u32]) -> Result<Vec<u32>, PartitionError> {
        Ok(partitions.to_vec())
    }

    async fn release_partitions(&self, _partitions: &[u32]) -> Result<(), PartitionError> {
        Ok(())
    }

    async fn get_assignments(&self) -> Result<HashMap<u32, String>, PartitionError> {
        Ok(HashMap::new())
    }

    fn watch_rebalance(&self) -> Result<RebalanceWatcher, PartitionError> {
        Ok(Box::pin(NeverStream::new()))
    }

    async fn find_replica(
        &self,
        _instrument_id: &InstrumentId,
    ) -> Result<Option<String>, PartitionError> {
        Ok(None)
    }

    async fn get_owned_partitions(&self, _instance_id: &str) -> Result<Vec<u32>, PartitionError> {
        Ok(Vec::new())
    }

    async fn trigger_rebalance(&self) -> Result<(), PartitionError> {
        Ok(())
    }
}

/// Empty leader election that always returns "not leader".
#[derive(Debug, Default)]
pub struct EmptyLeaderElection;

#[async_trait]
impl LeaderElection for EmptyLeaderElection {
    async fn campaign(&self, _candidate_id: &str) -> Result<LeaderStatus, ElectionError> {
        Ok(LeaderStatus::NoLeader)
    }

    async fn is_leader(&self) -> bool {
        false
    }

    async fn resign(&self) -> Result<(), ElectionError> {
        Ok(())
    }

    async fn get_leader(&self) -> Result<Option<String>, ElectionError> {
        Ok(None)
    }

    fn watch(&self) -> Result<LeaderWatcher, ElectionError> {
        Ok(Box::pin(NeverStream::new()))
    }

    async fn keep_alive(&self) -> Result<(), ElectionError> {
        Ok(())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_info_creation() {
        let info = InstanceInfo::new("engine-0", "convex-engine", "10.0.0.1", 8080)
            .with_partition(0)
            .with_status(InstanceStatus::Ready)
            .with_metadata("version", "1.0.0");

        assert_eq!(info.instance_id, "engine-0");
        assert_eq!(info.partition_id, Some(0));
        assert_eq!(info.status, InstanceStatus::Ready);
        assert_eq!(info.metadata.get("version"), Some(&"1.0.0".to_string()));
        assert_eq!(info.address(), "10.0.0.1:8080");
    }

    #[test]
    fn test_partition_config_hash() {
        let config = PartitionConfig::new(0, 3);

        // Same string should always hash to same partition
        let owns_a = config.owns_by_hash("INSTRUMENT_A");
        let owns_a_again = config.owns_by_hash("INSTRUMENT_A");
        assert_eq!(owns_a, owns_a_again);

        // Different strings might hash to different partitions
        // (but could be same due to hash collision)
        let _owns_b = config.owns_by_hash("INSTRUMENT_B");
    }

    #[test]
    fn test_leader_status() {
        let leader = LeaderStatus::Leader { lease_id: 12345 };
        assert!(leader.is_leader());

        let follower = LeaderStatus::Follower {
            leader_id: "engine-0".to_string(),
        };
        assert!(!follower.is_leader());

        let no_leader = LeaderStatus::NoLeader;
        assert!(!no_leader.is_leader());
    }

    #[test]
    fn test_gossip_state() {
        let mut state = GossipState::new("engine-0");
        assert_eq!(state.version, 0);

        state.increment_version();
        assert_eq!(state.version, 1);
    }

    #[test]
    fn test_partition_assignment() {
        let assignment = PartitionAssignment {
            currencies: Some(vec!["USD".to_string(), "EUR".to_string()]),
            issuer_types: None,
            instrument_ids: None,
        };

        assert!(assignment.currencies.is_some());
        assert!(assignment.issuer_types.is_none());
    }

    #[tokio::test]
    async fn test_empty_service_registry() {
        let registry = EmptyServiceRegistry;

        // Should not fail
        let info = InstanceInfo::new("test", "test-service", "localhost", 8080);
        registry.register(&info).await.unwrap();
        registry.deregister("test").await.unwrap();
        registry.heartbeat("test").await.unwrap();

        let instances = registry.get_instances("test-service").await.unwrap();
        assert!(instances.is_empty());
    }

    #[tokio::test]
    async fn test_empty_partition_registry() {
        let registry = EmptyPartitionRegistry;

        let claimed = registry.claim_partitions(&[0, 1, 2]).await.unwrap();
        assert_eq!(claimed, vec![0, 1, 2]);

        registry.release_partitions(&[0, 1, 2]).await.unwrap();

        let assignments = registry.get_assignments().await.unwrap();
        assert!(assignments.is_empty());
    }

    #[tokio::test]
    async fn test_empty_leader_election() {
        let election = EmptyLeaderElection;

        let status = election.campaign("test").await.unwrap();
        assert!(!status.is_leader());

        assert!(!election.is_leader().await);

        let leader = election.get_leader().await.unwrap();
        assert!(leader.is_none());
    }
}
