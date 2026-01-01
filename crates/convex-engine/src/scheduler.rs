//! Schedulers for reactive pricing calculations.
//!
//! This module provides:
//! - [`IntervalScheduler`]: Fixed-interval calculations (e.g., iNAV every 15s)
//! - [`EodScheduler`]: End-of-day calculations (e.g., official NAV)
//! - [`ThrottleManager`]: Debounced calculations for illiquid instruments

use std::sync::Arc;
use std::time::Duration;

use chrono::{Local, NaiveTime, Timelike};
use dashmap::{DashMap, DashSet};
use tokio::sync::broadcast;
use tokio::time::{interval, Instant};
use tracing::{debug, info};

use crate::calc_graph::{CalculationGraph, NodeId};

// =============================================================================
// INTERVAL SCHEDULER
// =============================================================================

/// Manages fixed-interval calculations.
///
/// Nodes registered with the interval scheduler will be recalculated
/// on a fixed schedule, regardless of when their inputs change.
/// This is useful for regulatory requirements like iNAV every 15 seconds.
pub struct IntervalScheduler {
    /// Nodes grouped by interval duration
    interval_groups: DashMap<Duration, Vec<NodeId>>,

    /// Running interval tasks (duration -> shutdown sender)
    tasks: DashMap<Duration, broadcast::Sender<()>>,

    /// Calculation graph reference
    calc_graph: Arc<CalculationGraph>,

    /// Callback for when nodes are calculated
    update_tx: broadcast::Sender<NodeUpdate>,
}

/// Update notification when a node is recalculated.
#[derive(Debug, Clone)]
pub struct NodeUpdate {
    /// The node that was updated
    pub node_id: NodeId,
    /// When the update occurred
    pub timestamp: i64,
    /// Update source
    pub source: UpdateSource,
}

/// Source of the update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateSource {
    /// Immediate recalculation from input change
    Immediate,
    /// Throttled recalculation
    Throttled,
    /// Fixed interval recalculation
    Interval,
    /// End-of-day recalculation
    EndOfDay,
    /// On-demand recalculation
    OnDemand,
}

impl IntervalScheduler {
    /// Create a new interval scheduler.
    pub fn new(calc_graph: Arc<CalculationGraph>) -> Self {
        let (update_tx, _) = broadcast::channel(1000);
        Self {
            interval_groups: DashMap::new(),
            tasks: DashMap::new(),
            calc_graph,
            update_tx,
        }
    }

    /// Subscribe to node updates.
    pub fn subscribe(&self) -> broadcast::Receiver<NodeUpdate> {
        self.update_tx.subscribe()
    }

    /// Register a node for interval updates.
    pub fn register(&self, node_id: NodeId, interval_duration: Duration) {
        // Add to interval group
        self.interval_groups
            .entry(interval_duration)
            .or_default()
            .push(node_id.clone());

        info!(
            "Registered node {} for interval updates every {:?}",
            node_id, interval_duration
        );

        // Start interval task if not running
        if !self.tasks.contains_key(&interval_duration) {
            self.start_interval_task(interval_duration);
        }
    }

    /// Unregister a node from interval updates.
    pub fn unregister(&self, node_id: &NodeId) {
        for mut entry in self.interval_groups.iter_mut() {
            entry.value_mut().retain(|id| id != node_id);
        }
    }

    /// Start an interval task for a specific duration.
    fn start_interval_task(&self, interval_duration: Duration) {
        let calc_graph = self.calc_graph.clone();
        let groups = self.interval_groups.clone();
        let update_tx = self.update_tx.clone();

        let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);
        self.tasks.insert(interval_duration, shutdown_tx);

        tokio::spawn(async move {
            let mut ticker = interval(interval_duration);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        // Get nodes for this interval
                        if let Some(nodes) = groups.get(&interval_duration) {
                            let timestamp = chrono::Utc::now().timestamp();

                            for node_id in nodes.iter() {
                                // Check if node is dirty
                                if calc_graph.is_dirty(node_id) {
                                    debug!("Interval scheduler processing: {}", node_id);

                                    // Notify subscribers
                                    let _ = update_tx.send(NodeUpdate {
                                        node_id: node_id.clone(),
                                        timestamp,
                                        source: UpdateSource::Interval,
                                    });
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Interval scheduler shutting down for {:?}", interval_duration);
                        break;
                    }
                }
            }
        });

        info!("Started interval scheduler for {:?}", interval_duration);
    }

    /// Stop all interval tasks.
    pub fn stop_all(&self) {
        for entry in self.tasks.iter() {
            let _ = entry.value().send(());
        }
        self.tasks.clear();
    }

    /// Get all registered intervals.
    pub fn get_intervals(&self) -> Vec<Duration> {
        self.interval_groups.iter().map(|e| *e.key()).collect()
    }

    /// Get nodes for a specific interval.
    pub fn get_nodes(&self, interval_duration: Duration) -> Vec<NodeId> {
        self.interval_groups
            .get(&interval_duration)
            .map(|v| v.clone())
            .unwrap_or_default()
    }
}

// =============================================================================
// EOD SCHEDULER
// =============================================================================

/// Manages end-of-day calculations.
///
/// Nodes registered with the EOD scheduler will be recalculated
/// once per day at a specified time (e.g., 4:00 PM for official NAV).
pub struct EodScheduler {
    /// Nodes grouped by EOD time
    eod_nodes: DashMap<String, Vec<NodeId>>,

    /// Calculation graph reference
    calc_graph: Arc<CalculationGraph>,

    /// Update notification channel
    update_tx: broadcast::Sender<NodeUpdate>,

    /// Shutdown signal
    shutdown_tx: Option<broadcast::Sender<()>>,
}

impl EodScheduler {
    /// Create a new EOD scheduler.
    pub fn new(calc_graph: Arc<CalculationGraph>) -> Self {
        let (update_tx, _) = broadcast::channel(1000);
        Self {
            eod_nodes: DashMap::new(),
            calc_graph,
            update_tx,
            shutdown_tx: None,
        }
    }

    /// Subscribe to node updates.
    pub fn subscribe(&self) -> broadcast::Receiver<NodeUpdate> {
        self.update_tx.subscribe()
    }

    /// Register a node for EOD updates at specified time.
    ///
    /// # Arguments
    /// * `node_id` - The node to register
    /// * `time` - Time string in "HH:MM:SS" format (local timezone)
    pub fn register(&self, node_id: NodeId, time: &str) {
        self.eod_nodes
            .entry(time.to_string())
            .or_default()
            .push(node_id.clone());

        info!("Registered node {} for EOD at {}", node_id, time);
    }

    /// Unregister a node from EOD updates.
    pub fn unregister(&self, node_id: &NodeId) {
        for mut entry in self.eod_nodes.iter_mut() {
            entry.value_mut().retain(|id| id != node_id);
        }
    }

    /// Start the EOD scheduler.
    pub fn start(&mut self) {
        let calc_graph = self.calc_graph.clone();
        let eod_nodes = self.eod_nodes.clone();
        let update_tx = self.update_tx.clone();

        let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        tokio::spawn(async move {
            // Check every minute for EOD times
            let mut ticker = interval(Duration::from_secs(60));

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let now = Local::now();
                        let current_time = format!("{:02}:{:02}:00", now.hour(), now.minute());

                        // Check if any EOD time matches
                        if let Some(nodes) = eod_nodes.get(&current_time) {
                            info!("EOD trigger at {}", current_time);
                            let timestamp = chrono::Utc::now().timestamp();

                            for node_id in nodes.iter() {
                                debug!("EOD scheduler processing: {}", node_id);

                                // Mark as dirty to trigger recalculation
                                calc_graph.invalidate(node_id);

                                // Notify subscribers
                                let _ = update_tx.send(NodeUpdate {
                                    node_id: node_id.clone(),
                                    timestamp,
                                    source: UpdateSource::EndOfDay,
                                });
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("EOD scheduler shutting down");
                        break;
                    }
                }
            }
        });

        info!("EOD scheduler started");
    }

    /// Stop the EOD scheduler.
    pub fn stop(&self) {
        if let Some(ref tx) = self.shutdown_tx {
            let _ = tx.send(());
        }
    }

    /// Get all registered EOD times.
    pub fn get_times(&self) -> Vec<String> {
        self.eod_nodes.iter().map(|e| e.key().clone()).collect()
    }

    /// Get nodes for a specific EOD time.
    pub fn get_nodes(&self, time: &str) -> Vec<NodeId> {
        self.eod_nodes
            .get(time)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    /// Calculate seconds until next EOD time.
    pub fn seconds_until_next_eod(&self) -> Option<u64> {
        let now = Local::now();
        let current_seconds = now.hour() * 3600 + now.minute() * 60 + now.second();

        let mut min_wait: Option<u64> = None;

        for entry in self.eod_nodes.iter() {
            if let Ok(time) = NaiveTime::parse_from_str(entry.key(), "%H:%M:%S") {
                let target_seconds = time.hour() * 3600 + time.minute() * 60 + time.second();
                let wait = if target_seconds > current_seconds {
                    (target_seconds - current_seconds) as u64
                } else {
                    // Tomorrow
                    (86400 - current_seconds + target_seconds) as u64
                };

                min_wait = Some(min_wait.map_or(wait, |m| m.min(wait)));
            }
        }

        min_wait
    }
}

// =============================================================================
// THROTTLE MANAGER
// =============================================================================

/// Manages throttled (debounced) calculations.
///
/// Nodes with throttled frequency will be recalculated at most once
/// per interval, even if their inputs change more frequently.
pub struct ThrottleManager {
    /// Last calculation time per node
    last_calc_time: DashMap<NodeId, Instant>,

    /// Pending throttled nodes (waiting for interval)
    pending: DashSet<NodeId>,

    /// Throttle intervals per node
    intervals: DashMap<NodeId, Duration>,

    /// Calculation graph reference
    calc_graph: Arc<CalculationGraph>,

    /// Update notification channel
    update_tx: broadcast::Sender<NodeUpdate>,
}

impl ThrottleManager {
    /// Create a new throttle manager.
    pub fn new(calc_graph: Arc<CalculationGraph>) -> Self {
        let (update_tx, _) = broadcast::channel(1000);
        Self {
            last_calc_time: DashMap::new(),
            pending: DashSet::new(),
            intervals: DashMap::new(),
            calc_graph,
            update_tx,
        }
    }

    /// Subscribe to node updates.
    pub fn subscribe(&self) -> broadcast::Receiver<NodeUpdate> {
        self.update_tx.subscribe()
    }

    /// Register a node for throttled updates.
    pub fn register(&self, node_id: NodeId, interval: Duration) {
        self.intervals.insert(node_id.clone(), interval);
        debug!(
            "Registered node {} for throttled updates (interval: {:?})",
            node_id, interval
        );
    }

    /// Unregister a node from throttled updates.
    pub fn unregister(&self, node_id: &NodeId) {
        self.intervals.remove(node_id);
        self.pending.remove(node_id);
        self.last_calc_time.remove(node_id);
    }

    /// Check if a node should be calculated now (respecting throttle).
    pub fn should_calculate(&self, node_id: &NodeId) -> bool {
        if let Some(interval) = self.intervals.get(node_id) {
            let now = Instant::now();
            self.last_calc_time
                .get(node_id)
                .map(|t| now.duration_since(*t) >= *interval)
                .unwrap_or(true)
        } else {
            true // Not throttled
        }
    }

    /// Mark a node as calculated.
    pub fn mark_calculated(&self, node_id: &NodeId) {
        self.last_calc_time.insert(node_id.clone(), Instant::now());
        self.pending.remove(node_id);
    }

    /// Schedule a throttled calculation for a node.
    ///
    /// If the throttle interval hasn't passed, the node is added to pending
    /// and will be calculated when the interval expires.
    pub fn schedule(&self, node_id: NodeId) {
        if !self.should_calculate(&node_id) {
            // Add to pending if not already
            if self.pending.insert(node_id.clone()) {
                // Spawn task to trigger after interval
                let calc_graph = self.calc_graph.clone();
                let update_tx = self.update_tx.clone();
                let pending = self.pending.clone();
                let last_calc_time = self.last_calc_time.clone();

                if let Some(interval) = self.intervals.get(&node_id).map(|i| *i) {
                    let node_id_clone = node_id.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(interval).await;

                        if pending.remove(&node_id_clone).is_some() {
                            debug!("Throttle expired, triggering: {}", node_id_clone);

                            // Mark as dirty
                            calc_graph.invalidate(&node_id_clone);
                            last_calc_time.insert(node_id_clone.clone(), Instant::now());

                            // Notify
                            let _ = update_tx.send(NodeUpdate {
                                node_id: node_id_clone,
                                timestamp: chrono::Utc::now().timestamp(),
                                source: UpdateSource::Throttled,
                            });
                        }
                    });
                }
            }
        } else {
            // Can calculate immediately
            self.mark_calculated(&node_id);

            let _ = self.update_tx.send(NodeUpdate {
                node_id,
                timestamp: chrono::Utc::now().timestamp(),
                source: UpdateSource::Throttled,
            });
        }
    }

    /// Get all pending nodes.
    pub fn get_pending(&self) -> Vec<NodeId> {
        self.pending.iter().map(|r| r.clone()).collect()
    }

    /// Get the throttle interval for a node.
    pub fn get_interval(&self, node_id: &NodeId) -> Option<Duration> {
        self.intervals.get(node_id).map(|i| *i)
    }
}

// =============================================================================
// CRON SCHEDULER
// =============================================================================

/// Manages cron-based scheduled calculations.
///
/// Nodes registered with the cron scheduler will be recalculated
/// according to a cron expression (e.g., "0 */15 * * * *" for every 15 minutes).
pub struct CronScheduler {
    /// Nodes grouped by cron expression
    cron_nodes: DashMap<String, Vec<NodeId>>,

    /// Parsed cron schedules
    schedules: DashMap<String, cron::Schedule>,

    /// Calculation graph reference
    calc_graph: Arc<CalculationGraph>,

    /// Update notification channel
    update_tx: broadcast::Sender<NodeUpdate>,

    /// Shutdown signal
    shutdown_tx: Option<broadcast::Sender<()>>,
}

impl CronScheduler {
    /// Create a new cron scheduler.
    pub fn new(calc_graph: Arc<CalculationGraph>) -> Self {
        let (update_tx, _) = broadcast::channel(1000);
        Self {
            cron_nodes: DashMap::new(),
            schedules: DashMap::new(),
            calc_graph,
            update_tx,
            shutdown_tx: None,
        }
    }

    /// Subscribe to node updates.
    pub fn subscribe(&self) -> broadcast::Receiver<NodeUpdate> {
        self.update_tx.subscribe()
    }

    /// Register a node for cron-based updates.
    ///
    /// # Arguments
    /// * `node_id` - The node to register
    /// * `cron_expr` - Cron expression (e.g., "0 */15 * * * *" for every 15 min)
    ///
    /// # Cron Expression Format
    /// Standard 6-field cron format: second minute hour day-of-month month day-of-week
    /// Examples:
    /// - "0 */15 * * * *" - Every 15 minutes
    /// - "0 0 9 * * MON-FRI" - 9 AM on weekdays
    /// - "0 30 16 * * *" - 4:30 PM daily
    pub fn register(&self, node_id: NodeId, cron_expr: &str) -> Result<(), String> {
        use std::str::FromStr;

        // Parse cron expression
        let schedule = cron::Schedule::from_str(cron_expr)
            .map_err(|e| format!("Invalid cron expression '{}': {}", cron_expr, e))?;

        // Store schedule
        self.schedules.insert(cron_expr.to_string(), schedule);

        // Add node to cron group
        self.cron_nodes
            .entry(cron_expr.to_string())
            .or_default()
            .push(node_id.clone());

        info!(
            "Registered node {} for cron schedule: {}",
            node_id, cron_expr
        );
        Ok(())
    }

    /// Unregister a node from cron updates.
    pub fn unregister(&self, node_id: &NodeId) {
        for mut entry in self.cron_nodes.iter_mut() {
            entry.value_mut().retain(|id| id != node_id);
        }
    }

    /// Start the cron scheduler.
    pub fn start(&mut self) {
        let calc_graph = self.calc_graph.clone();
        let cron_nodes = self.cron_nodes.clone();
        let schedules = self.schedules.clone();
        let update_tx = self.update_tx.clone();

        let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        tokio::spawn(async move {
            // Check every second for cron triggers
            let mut ticker = interval(Duration::from_secs(1));
            let mut last_check = chrono::Utc::now();

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let now = chrono::Utc::now();

                        // Check each cron expression
                        for entry in schedules.iter() {
                            let cron_expr = entry.key();
                            let schedule = entry.value();

                            // Get upcoming times between last check and now
                            let upcoming = schedule.after(&last_check);
                            for next_time in upcoming.take(1) {
                                if next_time <= now {
                                    // Trigger!
                                    if let Some(nodes) = cron_nodes.get(cron_expr) {
                                        info!("Cron trigger for '{}' at {}", cron_expr, now);
                                        let timestamp = now.timestamp();

                                        for node_id in nodes.iter() {
                                            debug!("Cron scheduler processing: {}", node_id);

                                            // Mark as dirty to trigger recalculation
                                            calc_graph.invalidate(node_id);

                                            // Notify subscribers
                                            let _ = update_tx.send(NodeUpdate {
                                                node_id: node_id.clone(),
                                                timestamp,
                                                source: UpdateSource::Interval, // Use Interval as closest match
                                            });
                                        }
                                    }
                                }
                            }
                        }

                        last_check = now;
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Cron scheduler shutting down");
                        break;
                    }
                }
            }
        });

        info!("Cron scheduler started");
    }

    /// Stop the cron scheduler.
    pub fn stop(&self) {
        if let Some(ref tx) = self.shutdown_tx {
            let _ = tx.send(());
        }
    }

    /// Get all registered cron expressions.
    pub fn get_expressions(&self) -> Vec<String> {
        self.cron_nodes.iter().map(|e| e.key().clone()).collect()
    }

    /// Get nodes for a specific cron expression.
    pub fn get_nodes(&self, cron_expr: &str) -> Vec<NodeId> {
        self.cron_nodes
            .get(cron_expr)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    /// Get next trigger time for a cron expression.
    pub fn next_trigger(&self, cron_expr: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        self.schedules
            .get(cron_expr)
            .and_then(|schedule| schedule.upcoming(chrono::Utc).next())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use convex_traits::ids::*;

    #[tokio::test]
    async fn test_interval_scheduler_registration() {
        let calc_graph = Arc::new(CalculationGraph::new());
        let scheduler = IntervalScheduler::new(calc_graph);

        let node_id = NodeId::EtfInav {
            etf_id: EtfId::new("LQD"),
        };
        let interval = Duration::from_secs(15);

        scheduler.register(node_id.clone(), interval);

        let nodes = scheduler.get_nodes(interval);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0], node_id);
    }

    #[test]
    fn test_eod_scheduler_registration() {
        let calc_graph = Arc::new(CalculationGraph::new());
        let scheduler = EodScheduler::new(calc_graph);

        let node_id = NodeId::EtfNav {
            etf_id: EtfId::new("LQD"),
        };

        scheduler.register(node_id.clone(), "16:00:00");

        let nodes = scheduler.get_nodes("16:00:00");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0], node_id);
    }

    #[test]
    fn test_throttle_manager() {
        let calc_graph = Arc::new(CalculationGraph::new());
        let manager = ThrottleManager::new(calc_graph);

        let node_id = NodeId::BondPrice {
            instrument_id: InstrumentId::new("TEST"),
        };
        let interval = Duration::from_secs(1);

        manager.register(node_id.clone(), interval);

        // First check - should calculate
        assert!(manager.should_calculate(&node_id));

        // Mark as calculated
        manager.mark_calculated(&node_id);

        // Immediately after - should not calculate
        assert!(!manager.should_calculate(&node_id));
    }

    #[test]
    fn test_cron_scheduler_registration() {
        let calc_graph = Arc::new(CalculationGraph::new());
        let scheduler = CronScheduler::new(calc_graph);

        let node_id = NodeId::Portfolio {
            portfolio_id: PortfolioId::new("TEST_PORTFOLIO"),
        };

        // Register with valid cron expression (every 15 minutes)
        let result = scheduler.register(node_id.clone(), "0 */15 * * * *");
        assert!(result.is_ok());

        let nodes = scheduler.get_nodes("0 */15 * * * *");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0], node_id);
    }

    #[test]
    fn test_cron_scheduler_invalid_expression() {
        let calc_graph = Arc::new(CalculationGraph::new());
        let scheduler = CronScheduler::new(calc_graph);

        let node_id = NodeId::Portfolio {
            portfolio_id: PortfolioId::new("TEST_PORTFOLIO"),
        };

        // Invalid cron expression
        let result = scheduler.register(node_id, "invalid cron");
        assert!(result.is_err());
    }

    #[test]
    fn test_cron_scheduler_next_trigger() {
        let calc_graph = Arc::new(CalculationGraph::new());
        let scheduler = CronScheduler::new(calc_graph);

        let node_id = NodeId::Portfolio {
            portfolio_id: PortfolioId::new("TEST_PORTFOLIO"),
        };

        // Register with valid cron expression
        scheduler.register(node_id, "0 0 * * * *").unwrap(); // Every hour

        // Should have a next trigger time
        let next = scheduler.next_trigger("0 0 * * * *");
        assert!(next.is_some());

        // Next trigger should be in the future
        let next_time = next.unwrap();
        assert!(next_time > chrono::Utc::now());
    }
}
