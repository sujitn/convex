//! Curve cache with atomic hot-swap support.
//!
//! The `CurveCache` provides thread-safe curve storage with atomic updates.
//! This enables live curve rebuilding without blocking ongoing calculations.
//!
//! # Features
//!
//! - **Atomic swap**: Replace curves without locking readers
//! - **Version tracking**: Each curve update increments a version counter
//! - **Statistics**: Track hit/miss rates and access patterns
//! - **TTL support**: Optional expiration for stale curves
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_engine::cache::CurveCache;
//!
//! let cache = CurveCache::new();
//!
//! // Store a curve
//! cache.put("USD.GOVT", curve);
//!
//! // Get a curve (returns CurveRef which is Arc<dyn TermStructure>)
//! if let Some(curve) = cache.get("USD.GOVT") {
//!     let df = curve.value_at(5.0);
//! }
//!
//! // Atomic swap (old curve remains valid for in-flight calculations)
//! cache.swap("USD.GOVT", new_curve);
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use convex_curves::{CurveRef, TermStructure};

use crate::error::{EngineError, EngineResult};

// =============================================================================
// CACHE ENTRY
// =============================================================================

/// Metadata for a cached curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveMetadata {
    /// Curve identifier.
    pub curve_id: String,
    /// Version number (incremented on each update).
    pub version: u64,
    /// When the curve was built.
    pub build_time: DateTime<Utc>,
    /// When the curve was cached.
    pub cached_at: DateTime<Utc>,
    /// Reference date of the curve.
    pub reference_date: chrono::NaiveDate,
    /// Tenor bounds (min, max) in years.
    pub tenor_bounds: (f64, f64),
    /// Build duration in microseconds.
    pub build_time_us: u64,
    /// Source identifier (e.g., "BOOTSTRAP", "MANUAL").
    pub source: Option<String>,
}

/// A cached curve with metadata.
pub struct CacheEntry {
    /// The curve itself.
    curve: CurveRef,
    /// Metadata about the curve.
    metadata: CurveMetadata,
    /// Last access time (for LRU eviction).
    last_accessed: RwLock<Instant>,
    /// Access count.
    access_count: AtomicU64,
}

impl CacheEntry {
    /// Creates a new cache entry.
    pub fn new(curve_id: String, curve: CurveRef, build_time_us: u64) -> Self {
        let ref_date = curve.reference_date();
        let (min_tenor, max_tenor) = curve.tenor_bounds();

        let metadata = CurveMetadata {
            curve_id,
            version: 1,
            build_time: Utc::now(),
            cached_at: Utc::now(),
            reference_date: chrono::NaiveDate::from_ymd_opt(
                ref_date.year(),
                ref_date.month(),
                ref_date.day(),
            )
            .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()),
            tenor_bounds: (min_tenor, max_tenor),
            build_time_us,
            source: None,
        };

        Self {
            curve,
            metadata,
            last_accessed: RwLock::new(Instant::now()),
            access_count: AtomicU64::new(0),
        }
    }

    /// Creates a cache entry with source information.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.metadata.source = Some(source.into());
        self
    }

    /// Returns a reference to the curve.
    pub fn curve(&self) -> CurveRef {
        *self.last_accessed.write() = Instant::now();
        self.access_count.fetch_add(1, Ordering::Relaxed);
        self.curve.clone()
    }

    /// Returns the curve without updating access stats (for internal use).
    pub fn curve_untracked(&self) -> CurveRef {
        self.curve.clone()
    }

    /// Returns the metadata.
    pub fn metadata(&self) -> &CurveMetadata {
        &self.metadata
    }

    /// Returns the version.
    pub fn version(&self) -> u64 {
        self.metadata.version
    }

    /// Returns the access count.
    pub fn access_count(&self) -> u64 {
        self.access_count.load(Ordering::Relaxed)
    }

    /// Returns the age of this entry.
    pub fn age(&self) -> Duration {
        self.last_accessed.read().elapsed()
    }

    /// Updates the entry with a new curve (increments version).
    fn update(&mut self, curve: CurveRef, build_time_us: u64) {
        let ref_date = curve.reference_date();
        let (min_tenor, max_tenor) = curve.tenor_bounds();

        self.curve = curve;
        self.metadata.version += 1;
        self.metadata.build_time = Utc::now();
        self.metadata.cached_at = Utc::now();
        self.metadata.reference_date = chrono::NaiveDate::from_ymd_opt(
            ref_date.year(),
            ref_date.month(),
            ref_date.day(),
        )
        .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(2000, 1, 1).unwrap());
        self.metadata.tenor_bounds = (min_tenor, max_tenor);
        self.metadata.build_time_us = build_time_us;
        *self.last_accessed.write() = Instant::now();
    }
}

// =============================================================================
// CACHE STATISTICS
// =============================================================================

/// Statistics for the curve cache.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStats {
    /// Number of curves in cache.
    pub curve_count: usize,
    /// Total cache hits.
    pub hits: u64,
    /// Total cache misses.
    pub misses: u64,
    /// Total curve updates (swaps).
    pub updates: u64,
    /// Total evictions.
    pub evictions: u64,
}

impl CacheStats {
    /// Returns the hit rate (0.0 to 1.0).
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

// =============================================================================
// CURVE CACHE
// =============================================================================

/// Thread-safe curve cache with atomic hot-swap.
///
/// The cache stores `CurveRef` (Arc<dyn TermStructure>) which enables
/// zero-copy sharing across threads. Updates are atomic - old curves
/// remain valid for in-flight calculations.
pub struct CurveCache {
    /// Curve storage.
    curves: DashMap<String, Arc<RwLock<CacheEntry>>>,

    /// Cache statistics.
    hits: AtomicU64,
    misses: AtomicU64,
    updates: AtomicU64,
    evictions: AtomicU64,

    /// Optional TTL for curve expiration.
    ttl: Option<Duration>,

    /// Maximum number of curves to cache.
    max_size: Option<usize>,
}

impl CurveCache {
    /// Creates a new curve cache with default settings.
    pub fn new() -> Self {
        Self {
            curves: DashMap::new(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            updates: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            ttl: None,
            max_size: None,
        }
    }

    /// Creates a cache with a maximum size.
    pub fn with_max_size(max_size: usize) -> Self {
        Self {
            max_size: Some(max_size),
            ..Self::new()
        }
    }

    /// Creates a cache with TTL expiration.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            ttl: Some(ttl),
            ..Self::new()
        }
    }

    /// Sets the TTL.
    pub fn set_ttl(&mut self, ttl: Option<Duration>) {
        self.ttl = ttl;
    }

    /// Sets the maximum size.
    pub fn set_max_size(&mut self, max_size: Option<usize>) {
        self.max_size = max_size;
    }

    /// Returns the number of curves in the cache.
    pub fn len(&self) -> usize {
        self.curves.len()
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.curves.is_empty()
    }

    /// Gets a curve from the cache.
    ///
    /// Returns `None` if the curve is not found or has expired.
    pub fn get(&self, curve_id: &str) -> Option<CurveRef> {
        match self.curves.get(curve_id) {
            Some(entry) => {
                let entry_guard = entry.read();

                // Check TTL expiration
                if let Some(ttl) = self.ttl {
                    if entry_guard.age() > ttl {
                        drop(entry_guard);
                        drop(entry);
                        self.curves.remove(curve_id);
                        self.misses.fetch_add(1, Ordering::Relaxed);
                        self.evictions.fetch_add(1, Ordering::Relaxed);
                        return None;
                    }
                }

                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(entry_guard.curve())
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Gets a curve with its metadata.
    pub fn get_with_metadata(&self, curve_id: &str) -> Option<(CurveRef, CurveMetadata)> {
        self.curves.get(curve_id).map(|entry| {
            let entry_guard = entry.read();
            self.hits.fetch_add(1, Ordering::Relaxed);
            (entry_guard.curve(), entry_guard.metadata().clone())
        })
    }

    /// Checks if a curve exists in the cache.
    pub fn contains(&self, curve_id: &str) -> bool {
        self.curves.contains_key(curve_id)
    }

    /// Stores a curve in the cache.
    ///
    /// If a curve with the same ID already exists, it will be replaced.
    pub fn put(&self, curve_id: impl Into<String>, curve: CurveRef) {
        self.put_with_timing(curve_id, curve, 0);
    }

    /// Stores a curve with build timing information.
    pub fn put_with_timing(
        &self,
        curve_id: impl Into<String>,
        curve: CurveRef,
        build_time_us: u64,
    ) {
        let curve_id = curve_id.into();
        let entry = CacheEntry::new(curve_id.clone(), curve, build_time_us);

        // Check if we need to evict
        if let Some(max_size) = self.max_size {
            if self.curves.len() >= max_size && !self.curves.contains_key(&curve_id) {
                self.evict_oldest();
            }
        }

        self.curves
            .insert(curve_id, Arc::new(RwLock::new(entry)));
    }

    /// Atomically swaps a curve.
    ///
    /// The old curve remains valid for any in-flight calculations that
    /// hold a reference to it.
    pub fn swap(&self, curve_id: impl Into<String>, curve: CurveRef) -> Option<CurveRef> {
        self.swap_with_timing(curve_id, curve, 0)
    }

    /// Atomically swaps a curve with timing information.
    pub fn swap_with_timing(
        &self,
        curve_id: impl Into<String>,
        curve: CurveRef,
        build_time_us: u64,
    ) -> Option<CurveRef> {
        let curve_id = curve_id.into();

        match self.curves.get(&curve_id) {
            Some(entry) => {
                let old_curve = {
                    let mut entry_guard = entry.write();
                    let old = entry_guard.curve_untracked();
                    entry_guard.update(curve, build_time_us);
                    old
                };
                self.updates.fetch_add(1, Ordering::Relaxed);
                Some(old_curve)
            }
            None => {
                // Curve doesn't exist, just put it
                self.put_with_timing(curve_id, curve, build_time_us);
                None
            }
        }
    }

    /// Removes a curve from the cache.
    pub fn remove(&self, curve_id: &str) -> Option<CurveRef> {
        self.curves.remove(curve_id).map(|(_, entry)| {
            self.evictions.fetch_add(1, Ordering::Relaxed);
            entry.read().curve_untracked()
        })
    }

    /// Clears all curves from the cache.
    pub fn clear(&self) {
        let count = self.curves.len();
        self.curves.clear();
        self.evictions.fetch_add(count as u64, Ordering::Relaxed);
    }

    /// Returns all curve IDs in the cache.
    pub fn curve_ids(&self) -> Vec<String> {
        self.curves.iter().map(|r| r.key().clone()).collect()
    }

    /// Returns metadata for all curves.
    pub fn all_metadata(&self) -> Vec<CurveMetadata> {
        self.curves
            .iter()
            .map(|r| r.value().read().metadata().clone())
            .collect()
    }

    /// Returns cache statistics.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            curve_count: self.curves.len(),
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            updates: self.updates.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
        }
    }

    /// Resets cache statistics.
    pub fn reset_stats(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.updates.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
    }

    /// Evicts expired curves (if TTL is set).
    pub fn evict_expired(&self) -> usize {
        let ttl = match self.ttl {
            Some(ttl) => ttl,
            None => return 0,
        };

        let mut evicted = 0;
        let expired: Vec<_> = self
            .curves
            .iter()
            .filter_map(|entry| {
                if entry.read().age() > ttl {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();

        for curve_id in expired {
            if self.curves.remove(&curve_id).is_some() {
                evicted += 1;
            }
        }

        self.evictions.fetch_add(evicted as u64, Ordering::Relaxed);
        evicted
    }

    /// Evicts the oldest (least recently accessed) curve.
    fn evict_oldest(&self) {
        let oldest = self
            .curves
            .iter()
            .map(|r| (r.key().clone(), r.value().read().age()))
            .max_by(|a, b| a.1.cmp(&b.1))
            .map(|(k, _)| k);

        if let Some(curve_id) = oldest {
            self.curves.remove(&curve_id);
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Gets the version of a cached curve.
    pub fn version(&self, curve_id: &str) -> Option<u64> {
        self.curves.get(curve_id).map(|e| e.read().version())
    }

    /// Returns true if the curve has been updated since the given version.
    pub fn is_stale(&self, curve_id: &str, known_version: u64) -> bool {
        match self.curves.get(curve_id) {
            Some(entry) => entry.read().version() != known_version,
            None => true,
        }
    }
}

impl Default for CurveCache {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// CURVE SNAPSHOT
// =============================================================================

/// A snapshot of all curves at a point in time.
///
/// Used for consistent pricing across a portfolio where all bonds
/// should use the same curve versions.
#[derive(Clone)]
pub struct CurveSnapshot {
    /// Snapshot timestamp.
    timestamp: DateTime<Utc>,
    /// Curves in this snapshot.
    curves: DashMap<String, CurveRef>,
    /// Version of each curve when snapshot was taken.
    versions: DashMap<String, u64>,
}

impl CurveSnapshot {
    /// Creates a snapshot from the current cache state.
    pub fn from_cache(cache: &CurveCache) -> Self {
        let curves = DashMap::new();
        let versions = DashMap::new();

        for entry in cache.curves.iter() {
            let curve_id = entry.key().clone();
            let entry_guard = entry.value().read();
            curves.insert(curve_id.clone(), entry_guard.curve_untracked());
            versions.insert(curve_id, entry_guard.version());
        }

        Self {
            timestamp: Utc::now(),
            curves,
            versions,
        }
    }

    /// Creates a snapshot with specific curves.
    pub fn with_curves(curve_ids: &[&str], cache: &CurveCache) -> EngineResult<Self> {
        let curves = DashMap::new();
        let versions = DashMap::new();

        for &curve_id in curve_ids {
            match cache.curves.get(curve_id) {
                Some(entry) => {
                    let entry_guard = entry.read();
                    curves.insert(curve_id.to_string(), entry_guard.curve_untracked());
                    versions.insert(curve_id.to_string(), entry_guard.version());
                }
                None => {
                    return Err(EngineError::CurveNotFound(curve_id.to_string()));
                }
            }
        }

        Ok(Self {
            timestamp: Utc::now(),
            curves,
            versions,
        })
    }

    /// Returns the snapshot timestamp.
    pub fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }

    /// Gets a curve from the snapshot.
    pub fn get(&self, curve_id: &str) -> Option<CurveRef> {
        self.curves.get(curve_id).map(|r| r.clone())
    }

    /// Returns all curve IDs in the snapshot.
    pub fn curve_ids(&self) -> Vec<String> {
        self.curves.iter().map(|r| r.key().clone()).collect()
    }

    /// Returns the version of a curve in this snapshot.
    pub fn version(&self, curve_id: &str) -> Option<u64> {
        self.versions.get(curve_id).map(|r| *r)
    }

    /// Checks if any curves in this snapshot are stale compared to the cache.
    pub fn is_stale(&self, cache: &CurveCache) -> bool {
        self.versions.iter().any(|entry| {
            let curve_id = entry.key();
            let version = *entry.value();
            cache.is_stale(curve_id, version)
        })
    }
}

impl std::fmt::Debug for CurveSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CurveSnapshot")
            .field("timestamp", &self.timestamp)
            .field("curve_count", &self.curves.len())
            .finish()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::types::Date;
    use convex_curves::ValueType;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::Compounding;
    use std::sync::Arc;

    /// A simple flat curve for testing.
    struct FlatCurve {
        reference_date: Date,
        value: f64,
        max_tenor: f64,
    }

    impl FlatCurve {
        fn new(value: f64) -> Self {
            Self {
                reference_date: Date::from_ymd(2024, 1, 1).unwrap(),
                value,
                max_tenor: 30.0,
            }
        }
    }

    impl TermStructure for FlatCurve {
        fn reference_date(&self) -> Date {
            self.reference_date
        }

        fn value_at(&self, _t: f64) -> f64 {
            self.value
        }

        fn tenor_bounds(&self) -> (f64, f64) {
            (0.0, self.max_tenor)
        }

        fn value_type(&self) -> ValueType {
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            }
        }

        fn max_date(&self) -> Date {
            self.reference_date.add_days((self.max_tenor * 365.0) as i64)
        }
    }

    #[test]
    fn test_cache_put_get() {
        let cache = CurveCache::new();
        let curve: CurveRef = Arc::new(FlatCurve::new(0.05));

        cache.put("USD.GOVT", curve);
        assert!(cache.contains("USD.GOVT"));

        let retrieved = cache.get("USD.GOVT");
        assert!(retrieved.is_some());
        assert!((retrieved.unwrap().value_at(5.0) - 0.05).abs() < 1e-10);
    }

    #[test]
    fn test_cache_swap() {
        let cache = CurveCache::new();
        let curve1: CurveRef = Arc::new(FlatCurve::new(0.04));
        let curve2: CurveRef = Arc::new(FlatCurve::new(0.05));

        cache.put("USD.GOVT", curve1);
        assert_eq!(cache.version("USD.GOVT"), Some(1));

        let old = cache.swap("USD.GOVT", curve2);
        assert!(old.is_some());
        assert!((old.unwrap().value_at(5.0) - 0.04).abs() < 1e-10);
        assert_eq!(cache.version("USD.GOVT"), Some(2));

        let current = cache.get("USD.GOVT").unwrap();
        assert!((current.value_at(5.0) - 0.05).abs() < 1e-10);
    }

    #[test]
    fn test_cache_stats() {
        let cache = CurveCache::new();
        let curve: CurveRef = Arc::new(FlatCurve::new(0.05));

        cache.put("USD.GOVT", curve);

        // Hit
        let _ = cache.get("USD.GOVT");
        let _ = cache.get("USD.GOVT");

        // Miss
        let _ = cache.get("EUR.GOVT");

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate() - 0.666666).abs() < 0.01);
    }

    #[test]
    fn test_cache_max_size() {
        let cache = CurveCache::with_max_size(2);

        cache.put("CURVE1", Arc::new(FlatCurve::new(0.01)));
        cache.put("CURVE2", Arc::new(FlatCurve::new(0.02)));

        // Should trigger eviction
        cache.put("CURVE3", Arc::new(FlatCurve::new(0.03)));

        assert_eq!(cache.len(), 2);
        // One of CURVE1 or CURVE2 should have been evicted
        assert!(cache.contains("CURVE3"));
    }

    #[test]
    fn test_curve_snapshot() {
        let cache = CurveCache::new();
        cache.put("USD.GOVT", Arc::new(FlatCurve::new(0.04)));
        cache.put("EUR.GOVT", Arc::new(FlatCurve::new(0.03)));

        let snapshot = CurveSnapshot::from_cache(&cache);
        assert_eq!(snapshot.curve_ids().len(), 2);
        assert!(snapshot.get("USD.GOVT").is_some());

        // Update cache
        cache.swap("USD.GOVT", Arc::new(FlatCurve::new(0.05)));

        // Snapshot should be stale
        assert!(snapshot.is_stale(&cache));

        // But snapshot still has old value
        assert!((snapshot.get("USD.GOVT").unwrap().value_at(5.0) - 0.04).abs() < 1e-10);
    }

    #[test]
    fn test_is_stale() {
        let cache = CurveCache::new();
        let curve: CurveRef = Arc::new(FlatCurve::new(0.05));

        cache.put("USD.GOVT", curve);
        let v1 = cache.version("USD.GOVT").unwrap();

        assert!(!cache.is_stale("USD.GOVT", v1));

        cache.swap("USD.GOVT", Arc::new(FlatCurve::new(0.06)));

        assert!(cache.is_stale("USD.GOVT", v1));
    }
}
