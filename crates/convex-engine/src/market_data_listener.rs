//! Market data listener for reactive pricing.
//!
//! The [`MarketDataListener`] subscribes to market data updates and feeds them
//! into the calculation graph, triggering reactive repricing.

use std::sync::Arc;

use rust_decimal::Decimal;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use convex_core::ids::*;
use convex_core::Date;

use crate::calc_graph::{CalculationGraph, NodeId, NodeValue};
use crate::curve_builder::BuiltCurve;
use crate::market_ingestion::{DataState, IngestionOutcome, IngestionResult, MarketDataAssembler};
use crate::scheduler::{NodeUpdate, ThrottleManager, UpdateSource};
use convex_ports::market_data::{QuoteEvent, QuoteField, StreamReceiver};

// =============================================================================
// UPDATE EVENTS
// =============================================================================

/// Market data update event.
#[derive(Debug, Clone)]
pub enum MarketDataUpdate {
    /// Quote update for a bond
    Quote(QuoteEvent),

    /// Curve input update
    CurveInput(CurveInputUpdate),

    /// Built curve update
    Curve(CurveUpdate),

    /// Index fixing update (for FRNs)
    IndexFixing(IndexFixingUpdate),

    /// Inflation fixing update (for ILBs)
    InflationFixing(InflationFixingUpdate),

    /// FX rate update
    FxRate(FxRateUpdate),

    /// Volatility surface update
    VolSurface(VolSurfaceUpdate),
}

/// Curve input update.
#[derive(Debug, Clone)]
pub struct CurveInputUpdate {
    /// Curve ID
    pub curve_id: CurveId,
    /// Instrument (e.g., "2Y", "5Y")
    pub instrument: String,
    /// Rate value
    pub rate: Decimal,
    /// Update timestamp
    pub timestamp: i64,
}

/// Built curve update.
#[derive(Debug, Clone)]
pub struct CurveUpdate {
    /// Curve ID
    pub curve_id: CurveId,
    /// Built curve data
    pub curve: BuiltCurve,
    /// Update timestamp
    pub timestamp: i64,
}

/// Index fixing update.
#[derive(Debug, Clone)]
pub struct IndexFixingUpdate {
    /// Rate index (e.g., SOFR, EURIBOR)
    pub index: FloatingRateIndex,
    /// Fixing date
    pub date: Date,
    /// Rate value
    pub rate: Decimal,
    /// Update timestamp
    pub timestamp: i64,
}

/// Inflation fixing update.
#[derive(Debug, Clone)]
pub struct InflationFixingUpdate {
    /// Inflation index
    pub index: InflationIndex,
    /// Reference month
    pub month: YearMonth,
    /// Index value
    pub value: Decimal,
    /// Update timestamp
    pub timestamp: i64,
}

/// FX rate update.
#[derive(Debug, Clone)]
pub struct FxRateUpdate {
    /// Currency pair
    pub pair: CurrencyPair,
    /// Mid rate
    pub mid: Decimal,
    /// Update timestamp
    pub timestamp: i64,
}

/// Volatility surface update.
#[derive(Debug, Clone)]
pub struct VolSurfaceUpdate {
    /// Surface ID
    pub surface_id: VolSurfaceId,
    /// ATM vols by expiry
    pub atm_vols: Vec<(f64, Decimal)>,
    /// Update timestamp
    pub timestamp: i64,
}

/// Observable ingestion decisions. These do not imply that downstream pricing is certified.
#[derive(Debug, Clone)]
pub enum IngestionStatus {
    /// One normalized event decision.
    Event(IngestionResult),
    /// Shared channel loss; every input is suspended.
    Lagged {
        /// Discarded messages.
        missed: u64,
    },
    /// End of delivery.
    Closed,
    /// Multiple suppliers require an explicit selection policy before graph use.
    SourceSelectionRequired {
        /// Ambiguous instrument.
        instrument_id: InstrumentId,
    },
}

// =============================================================================
// MARKET DATA LISTENER
// =============================================================================

/// Listens to market data updates and feeds them into the calculation graph.
pub struct MarketDataListener {
    /// Calculation graph
    calc_graph: Arc<CalculationGraph>,

    assembler: MarketDataAssembler,
    status_tx: broadcast::Sender<IngestionStatus>,
    // Unsequenced auxiliary messages cannot recover from shared-channel loss.
    auxiliary_suspended: bool,

    /// Throttle manager for debounced updates
    throttle_manager: Arc<ThrottleManager>,

    /// Market data update receiver
    update_rx: broadcast::Receiver<MarketDataUpdate>,

    /// Node update sender (for broadcasting processed updates)
    node_update_tx: broadcast::Sender<NodeUpdate>,

    /// Shutdown signal receiver
    shutdown_rx: broadcast::Receiver<()>,
}

impl MarketDataListener {
    /// Create a new market data listener.
    pub fn new(
        calc_graph: Arc<CalculationGraph>,
        throttle_manager: Arc<ThrottleManager>,
        update_rx: broadcast::Receiver<MarketDataUpdate>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        let (node_update_tx, _) = broadcast::channel(1000);
        Self {
            calc_graph,
            assembler: MarketDataAssembler::default(),
            status_tx: broadcast::channel(128).0,
            auxiliary_suspended: false,
            throttle_manager,
            update_rx,
            node_update_tx,
            shutdown_rx,
        }
    }

    /// Subscribe to processed node updates.
    pub fn subscribe(&self) -> broadcast::Receiver<NodeUpdate> {
        self.node_update_tx.subscribe()
    }

    /// Configure ingestion before receiving any event. Runtime policy activation belongs to the configuration use case.
    pub fn with_policy(
        mut self,
        policy: crate::market_ingestion::IngestionPolicy,
    ) -> Result<Self, convex_ports::TraitError> {
        if !self.assembler.is_empty() {
            return Err(convex_ports::TraitError::Conflict(
                "ingestion policy requires a new listener",
            ));
        }
        self.assembler = MarketDataAssembler::new(policy)?;
        Ok(self)
    }

    /// Run the market data listener.
    ///
    /// This method runs until a shutdown signal is received.
    pub async fn run(&mut self) {
        info!("Market data listener started");

        let mut freshness = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            tokio::select! {
                result = self.update_rx.recv() => {
                    match result {
                        Ok(update) => self.process_update(update, chrono::Utc::now().timestamp_millis()).await,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("Market data listener lagged by {} messages; inputs suspended", n);
                            self.assembler.transport_gap();
                            self.auxiliary_suspended = true;
                            self.calc_graph.suspend_market_delivery();
                            self.publish_status(IngestionStatus::Lagged { missed: n });
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("Market data channel closed");
                            break;
                        }
                    }
                }
                _ = freshness.tick() => self.expire_quotes(chrono::Utc::now().timestamp_millis()),
                _ = self.shutdown_rx.recv() => {
                    info!("Market data listener shutting down");
                    break;
                }
            }
        }
        self.assembler.close();
        self.calc_graph.suspend_market_delivery();
        self.publish_status(IngestionStatus::Closed);
    }

    /// Process a market data update.
    async fn process_update(&mut self, update: MarketDataUpdate, now_ms: i64) {
        if self.auxiliary_suspended && !matches!(update, MarketDataUpdate::Quote(_)) {
            warn!("Auxiliary input suspended after channel loss; restart/reload required");
            return;
        }
        match update {
            MarketDataUpdate::Quote(quote) => self.on_quote_update(quote, now_ms),
            MarketDataUpdate::CurveInput(input) => self.on_curve_input_update(input).await,
            MarketDataUpdate::Curve(curve) => self.on_curve_update(curve).await,
            MarketDataUpdate::IndexFixing(fixing) => self.on_index_fixing(fixing).await,
            MarketDataUpdate::InflationFixing(fixing) => self.on_inflation_fixing(fixing).await,
            MarketDataUpdate::FxRate(rate) => self.on_fx_rate_update(rate).await,
            MarketDataUpdate::VolSurface(surface) => self.on_vol_surface_update(surface).await,
        }
    }

    /// Observe event decisions and shared transport loss. Receiver lag remains explicit.
    pub fn subscribe_status(&self) -> StreamReceiver<IngestionStatus> {
        StreamReceiver::new(self.status_tx.subscribe())
    }

    fn publish_status(&self, status: IngestionStatus) {
        if self.status_tx.send(status).is_err() {
            debug!("No ingestion status subscribers");
        }
    }

    fn on_quote_update(&mut self, event: QuoteEvent, now_ms: i64) {
        let key = event.key.clone();
        let decision = self.assembler.ingest(event, now_ms);
        self.publish_status(IngestionStatus::Event(decision.clone()));
        let node_id = NodeId::Quote {
            instrument_id: key.instrument_id.clone(),
        };
        if decision.outcome == IngestionOutcome::CapacityExceeded {
            self.calc_graph.suspend_market_delivery();
            return;
        }
        if decision.state != DataState::Live {
            self.calc_graph.evict_cached(&node_id);
            return;
        }
        if decision.outcome != IngestionOutcome::Applied {
            return;
        }
        let Some(snapshot) = self.assembler.snapshot(&key, now_ms) else {
            return;
        };
        // Existing graph has no source-selection policy. Refuse a second source for the same instrument.
        if self.assembler.has_other_source(&key) {
            self.calc_graph.evict_cached(&node_id);
            self.publish_status(IngestionStatus::SourceSelectionRequired {
                instrument_id: key.instrument_id,
            });
            return;
        }
        let price = |field| {
            snapshot
                .fields
                .get(&field)
                .map(|observation| observation.value.value)
        };
        if price(QuoteField::BidPrice).is_none()
            && price(QuoteField::AskPrice).is_none()
            && price(QuoteField::MidPrice).is_none()
        {
            self.calc_graph.evict_cached(&node_id);
            return;
        }
        let Some(valid_for_ms) = self.assembler.fresh_for_ms(&key, now_ms) else {
            return;
        };
        let Ok(valid_for_ms) = u64::try_from(valid_for_ms) else {
            return;
        };
        let Some(deadline) =
            std::time::Instant::now().checked_add(std::time::Duration::from_millis(valid_for_ms))
        else {
            return;
        };
        self.calc_graph.update_cache_until(
            &node_id,
            NodeValue::Quote {
                bid: price(QuoteField::BidPrice),
                ask: price(QuoteField::AskPrice),
                mid: price(QuoteField::MidPrice),
            },
            deadline,
        );
        self.calc_graph.invalidate(&node_id);
        let bond_node = NodeId::BondPrice {
            instrument_id: key.instrument_id,
        };
        if self.throttle_manager.should_calculate(&bond_node) {
            self.throttle_manager.mark_calculated(&bond_node);
            self.notify_update(bond_node, UpdateSource::Immediate);
        } else {
            self.throttle_manager.schedule(bond_node);
        }
    }

    fn expire_quotes(&self, now_ms: i64) {
        for key in self.assembler.keys() {
            if self
                .assembler
                .state(key, now_ms)
                .is_some_and(|state| state != DataState::Live)
            {
                self.calc_graph.evict_cached(&NodeId::Quote {
                    instrument_id: key.instrument_id.clone(),
                });
            }
        }
    }

    /// Handle a curve input update.
    async fn on_curve_input_update(&self, update: CurveInputUpdate) {
        debug!(
            "Curve input update: {}.{} = {}",
            update.curve_id, update.instrument, update.rate
        );

        let node_id = NodeId::CurveInput {
            curve_id: update.curve_id.clone(),
            instrument: update.instrument.clone(),
        };

        // Invalidate curve input (propagates to curve, then to bonds)
        self.calc_graph.invalidate(&node_id);

        // Trigger curve rebuild
        let curve_node = NodeId::Curve {
            curve_id: update.curve_id.clone(),
        };
        self.calc_graph.invalidate(&curve_node);

        self.notify_update(curve_node, UpdateSource::Immediate);
    }

    /// Handle a built curve update.
    async fn on_curve_update(&self, update: CurveUpdate) {
        debug!("Curve update: {}", update.curve_id);

        let node_id = NodeId::Curve {
            curve_id: update.curve_id.clone(),
        };

        // Update cache with curve data
        self.calc_graph.update_cache(
            &node_id,
            NodeValue::Curve {
                points: update.curve.to_points(),
            },
        );

        // Invalidate to propagate to dependent bonds
        self.calc_graph.invalidate(&node_id);

        self.notify_update(node_id, UpdateSource::Immediate);
    }

    /// Handle an index fixing update.
    async fn on_index_fixing(&self, update: IndexFixingUpdate) {
        debug!(
            "Index fixing: {} {} = {}",
            update.index, update.date, update.rate
        );

        let node_id = NodeId::IndexFixing {
            index: update.index.clone(),
            date: update.date,
        };

        // Update cache
        self.calc_graph
            .update_cache(&node_id, NodeValue::IndexFixing { rate: update.rate });

        // Invalidate to propagate to FRNs
        self.calc_graph.invalidate(&node_id);

        self.notify_update(node_id, UpdateSource::Immediate);
    }

    /// Handle an inflation fixing update.
    async fn on_inflation_fixing(&self, update: InflationFixingUpdate) {
        debug!(
            "Inflation fixing: {} {} = {}",
            update.index, update.month, update.value
        );

        let node_id = NodeId::InflationFixing {
            index: update.index.clone(),
            month: update.month,
        };

        // Update cache
        self.calc_graph.update_cache(
            &node_id,
            NodeValue::InflationFixing {
                value: update.value,
            },
        );

        // Invalidate to propagate to ILBs
        self.calc_graph.invalidate(&node_id);

        self.notify_update(node_id, UpdateSource::Immediate);
    }

    /// Handle an FX rate update.
    async fn on_fx_rate_update(&self, update: FxRateUpdate) {
        debug!("FX rate update: {} = {}", update.pair, update.mid);

        let node_id = NodeId::FxRate {
            pair: update.pair.clone(),
        };

        // Update cache
        self.calc_graph
            .update_cache(&node_id, NodeValue::FxRate { mid: update.mid });

        // Invalidate to propagate to cross-currency holdings
        self.calc_graph.invalidate(&node_id);

        self.notify_update(node_id, UpdateSource::Immediate);
    }

    /// Handle a volatility surface update.
    async fn on_vol_surface_update(&self, update: VolSurfaceUpdate) {
        debug!("Vol surface update: {}", update.surface_id);

        let node_id = NodeId::VolSurface {
            surface_id: update.surface_id.clone(),
        };

        // Update cache
        self.calc_graph.update_cache(
            &node_id,
            NodeValue::VolSurface {
                atm_vols: update.atm_vols,
            },
        );

        // Invalidate to propagate to option-embedded bonds
        self.calc_graph.invalidate(&node_id);

        self.notify_update(node_id, UpdateSource::Immediate);
    }

    /// Notify subscribers of a node update.
    fn notify_update(&self, node_id: NodeId, source: UpdateSource) {
        let _ = self.node_update_tx.send(NodeUpdate {
            node_id,
            timestamp: chrono::Utc::now().timestamp(),
            source,
        });
    }
}

// =============================================================================
// MARKET DATA PUBLISHER
// =============================================================================

/// Result of publishing a market data update.
///
/// The send error carries the un-delivered [`MarketDataUpdate`], which is large,
/// so it is boxed to keep the success path cheap (and satisfy `result_large_err`).
pub type PublishResult = Result<(), PublishError>;

/// Observable publisher failure; rejected input is different from absent subscribers.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    /// Event validation failed before entering the bounded channel.
    #[error("invalid market event: {0}")]
    Invalid(#[from] convex_ports::TraitError),
    /// No subscriber received this update.
    #[error("no market data subscribers")]
    NoSubscribers(Box<MarketDataUpdate>),
}

/// Publisher for market data updates.
///
/// Use this to inject market data updates into the system.
#[derive(Clone)]
pub struct MarketDataPublisher {
    update_tx: broadcast::Sender<MarketDataUpdate>,
}

impl MarketDataPublisher {
    /// Create a new market data publisher.
    pub fn new() -> (Self, broadcast::Receiver<MarketDataUpdate>) {
        let (update_tx, update_rx) = broadcast::channel(10000);
        (Self { update_tx }, update_rx)
    }

    /// Subscribe to market data updates.
    pub fn subscribe(&self) -> broadcast::Receiver<MarketDataUpdate> {
        self.update_tx.subscribe()
    }

    /// Publish a quote update.
    pub fn publish_quote(&self, update: QuoteEvent) -> PublishResult {
        update.validate()?;
        self.publish(MarketDataUpdate::Quote(update))
    }

    /// Publish a curve update.
    pub fn publish_curve(&self, update: CurveUpdate) -> PublishResult {
        self.publish(MarketDataUpdate::Curve(update))
    }

    /// Publish a curve input update.
    pub fn publish_curve_input(&self, update: CurveInputUpdate) -> PublishResult {
        self.publish(MarketDataUpdate::CurveInput(update))
    }

    /// Publish an index fixing.
    pub fn publish_index_fixing(&self, update: IndexFixingUpdate) -> PublishResult {
        self.publish(MarketDataUpdate::IndexFixing(update))
    }

    /// Publish an inflation fixing.
    pub fn publish_inflation_fixing(&self, update: InflationFixingUpdate) -> PublishResult {
        self.publish(MarketDataUpdate::InflationFixing(update))
    }

    /// Publish an FX rate update.
    pub fn publish_fx_rate(&self, update: FxRateUpdate) -> PublishResult {
        self.publish(MarketDataUpdate::FxRate(update))
    }

    /// Publish a volatility surface update.
    pub fn publish_vol_surface(&self, update: VolSurfaceUpdate) -> PublishResult {
        self.publish(MarketDataUpdate::VolSurface(update))
    }

    /// Broadcasts a market data update, boxing the (large) send error.
    fn publish(&self, update: MarketDataUpdate) -> PublishResult {
        self.update_tx
            .send(update)
            .map(|_| ())
            .map_err(|error| PublishError::NoSubscribers(Box::new(error.0)))
    }
}

impl Default for MarketDataPublisher {
    fn default() -> Self {
        Self::new().0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_ports::market_data::QuoteEventKind;

    fn event() -> QuoteEvent {
        serde_json::from_str(
            include_str!("../../convex-ext-file/tests/fixtures/sequenced-quotes.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap()
    }

    fn make_listener() -> (MarketDataListener, Arc<CalculationGraph>) {
        let graph = Arc::new(CalculationGraph::new());
        let throttle = Arc::new(ThrottleManager::new(graph.clone()));
        let (_, update_rx) = broadcast::channel(1);
        let (_, shutdown_rx) = broadcast::channel(1);
        (
            MarketDataListener::new(graph.clone(), throttle, update_rx, shutdown_rx),
            graph,
        )
    }

    #[test]
    fn test_market_data_publisher() {
        let (publisher, _rx) = MarketDataPublisher::new();
        assert!(publisher.publish_quote(event()).is_ok());
        let mut invalid = event();
        invalid.schema_version = 2;
        assert!(matches!(
            publisher.publish_quote(invalid),
            Err(PublishError::Invalid(_))
        ));
        let publisher = MarketDataPublisher::default();
        assert!(matches!(
            publisher.publish_quote(event()),
            Err(PublishError::NoSubscribers(_))
        ));
    }

    #[test]
    fn test_quote_event_creation() {
        let quote = event();
        assert_eq!(quote.key.instrument_id.as_str(), "BOND-001");
        assert_eq!((quote.session.as_str(), quote.sequence), ("A", 1));
    }

    #[test]
    fn cache_expiry_is_enforced_without_a_maintenance_tick() {
        let graph = CalculationGraph::new();
        let quote = NodeId::Quote {
            instrument_id: "EXPIRING".into(),
        };
        graph.update_cache_until(
            &quote,
            NodeValue::Quote {
                bid: None,
                ask: None,
                mid: Some(Decimal::from(100)),
            },
            std::time::Instant::now() - std::time::Duration::from_millis(1),
        );
        assert!(graph.get_cached(&quote).is_none());
        graph.update_cache(
            &quote,
            NodeValue::Quote {
                bid: None,
                ask: None,
                mid: Some(Decimal::from(100)),
            },
        );
        assert!(graph.get_cached(&quote).is_some());
        let bond = NodeId::BondPrice {
            instrument_id: "EXPIRING".into(),
        };
        graph.update_cache(&bond, NodeValue::Empty);
        graph.evict_cached(&quote);
        assert!(graph.get_cached(&quote).is_none());
        assert!(graph.get_cached(&bond).is_none());
    }

    #[test]
    fn listener_uses_only_live_assembled_fields_and_evicts_unusable_inputs() {
        let (mut listener, graph) = make_listener();
        let node = NodeId::Quote {
            instrument_id: event().key.instrument_id,
        };
        listener.on_quote_update(event(), 1000);
        assert!(matches!(
            graph.get_cached(&node).unwrap().value,
            NodeValue::Quote {
                bid: Some(_),
                ask: Some(_),
                mid: Some(_)
            }
        ));
        let mut delta = event();
        delta.sequence = 3;
        delta.event = QuoteEventKind::Delta { fields: vec![] };
        listener.on_quote_update(delta, 1000);
        assert!(graph.get_cached(&node).is_none());
        let mut repaired = event();
        repaired.sequence = 3;
        listener.on_quote_update(repaired.clone(), 1000);
        assert!(graph.get_cached(&node).is_some());
        listener.expire_quotes(6001);
        assert!(graph.get_cached(&node).is_none());
        repaired.sequence = 4;
        repaired.source_time_ms = 7000;
        repaired.receive_time_ms = 7000;
        listener.on_quote_update(repaired.clone(), 7000);
        assert!(graph.get_cached(&node).is_some());
        repaired.sequence = 5;
        repaired.entitled = false;
        listener.on_quote_update(repaired, 7000);
        assert!(graph.get_cached(&node).is_none());
    }

    #[test]
    fn listener_refuses_supplier_composites_and_non_live_delivery() {
        let (mut listener, graph) = make_listener();
        let first = event();
        let node = NodeId::Quote {
            instrument_id: first.key.instrument_id.clone(),
        };
        listener.on_quote_update(first.clone(), 1000);
        let mut second = first.clone();
        second.key.source = "another-provider".into();
        listener.on_quote_update(second, 1000);
        assert!(graph.get_cached(&node).is_none());
        let (mut listener, graph) = make_listener();
        let mut delayed = first;
        delayed.quality = convex_ports::market_data::QuoteQuality::Delayed;
        listener.on_quote_update(delayed, 1000);
        assert!(graph.get_cached(&node).is_none());
    }

    #[tokio::test]
    async fn actual_broadcast_lag_suspends_until_image_and_closed_is_distinct() {
        let graph = Arc::new(CalculationGraph::new());
        let throttle = Arc::new(ThrottleManager::new(graph.clone()));
        let (tx, update_rx) = broadcast::channel(1);
        let (_shutdown_tx, shutdown_rx) = broadcast::channel(1);
        let mut listener = MarketDataListener::new(graph.clone(), throttle, update_rx, shutdown_rx);
        let mut image = event();
        let now = chrono::Utc::now().timestamp_millis() - 100;
        image.source_time_ms = now;
        image.receive_time_ms = now;
        listener.on_quote_update(image.clone(), now);
        let mut status = listener.subscribe_status();
        let mut delta = image.clone();
        delta.sequence = 2;
        delta.event = QuoteEventKind::Delta { fields: vec![] };
        tx.send(MarketDataUpdate::Quote(delta.clone())).unwrap();
        delta.sequence = 3;
        tx.send(MarketDataUpdate::Quote(delta)).unwrap();
        let handle = tokio::spawn(async move {
            listener.run().await;
        });
        let recv = |mut status: StreamReceiver<IngestionStatus>| async move {
            let value = tokio::time::timeout(std::time::Duration::from_secs(2), status.recv())
                .await
                .unwrap()
                .unwrap();
            (value, status)
        };
        let (value, returned) = recv(status).await;
        status = returned;
        assert!(matches!(value, IngestionStatus::Lagged { missed: 1 }));
        let (value, returned) = recv(status).await;
        status = returned;
        assert!(matches!(
            value,
            IngestionStatus::Event(IngestionResult {
                state: DataState::Gap,
                ..
            })
        ));
        let node = NodeId::Quote {
            instrument_id: image.key.instrument_id.clone(),
        };
        assert!(graph.get_cached(&node).is_none());
        image.sequence = 3;
        tx.send(MarketDataUpdate::Quote(image)).unwrap();
        let (value, returned) = recv(status).await;
        status = returned;
        assert!(matches!(
            value,
            IngestionStatus::Event(IngestionResult {
                state: DataState::Live,
                ..
            })
        ));
        assert!(graph.get_cached(&node).is_some());
        drop(tx);
        assert!(matches!(recv(status).await.0, IngestionStatus::Closed));
        handle.await.unwrap();
        assert!(graph.get_cached(&node).is_none());
    }
}
