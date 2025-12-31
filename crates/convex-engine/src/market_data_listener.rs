//! Market data listener for reactive pricing.
//!
//! The [`MarketDataListener`] subscribes to market data updates and feeds them
//! into the calculation graph, triggering reactive repricing.

use std::sync::Arc;

use rust_decimal::Decimal;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use convex_core::Date;
use convex_traits::ids::*;

use crate::calc_graph::{CalculationGraph, NodeId, NodeValue};
use crate::curve_builder::{BuiltCurve, CurveBuilder};
use crate::scheduler::{NodeUpdate, ThrottleManager, UpdateSource};

// =============================================================================
// UPDATE EVENTS
// =============================================================================

/// Market data update event.
#[derive(Debug, Clone)]
pub enum MarketDataUpdate {
    /// Quote update for a bond
    Quote(QuoteUpdate),

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

/// Quote update for a bond.
#[derive(Debug, Clone)]
pub struct QuoteUpdate {
    /// Instrument ID
    pub instrument_id: InstrumentId,
    /// Bid price
    pub bid: Option<Decimal>,
    /// Ask price
    pub ask: Option<Decimal>,
    /// Mid price
    pub mid: Option<Decimal>,
    /// Update timestamp
    pub timestamp: i64,
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

// =============================================================================
// MARKET DATA LISTENER
// =============================================================================

/// Listens to market data updates and feeds them into the calculation graph.
pub struct MarketDataListener {
    /// Calculation graph
    calc_graph: Arc<CalculationGraph>,

    /// Curve builder (reserved for future curve updates)
    #[allow(dead_code)]
    curve_builder: Arc<CurveBuilder>,

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
        curve_builder: Arc<CurveBuilder>,
        throttle_manager: Arc<ThrottleManager>,
        update_rx: broadcast::Receiver<MarketDataUpdate>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        let (node_update_tx, _) = broadcast::channel(1000);
        Self {
            calc_graph,
            curve_builder,
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

    /// Run the market data listener.
    ///
    /// This method runs until a shutdown signal is received.
    pub async fn run(&mut self) {
        info!("Market data listener started");

        loop {
            tokio::select! {
                result = self.update_rx.recv() => {
                    match result {
                        Ok(update) => self.process_update(update).await,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("Market data listener lagged by {} messages", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("Market data channel closed");
                            break;
                        }
                    }
                }
                _ = self.shutdown_rx.recv() => {
                    info!("Market data listener shutting down");
                    break;
                }
            }
        }
    }

    /// Process a market data update.
    async fn process_update(&self, update: MarketDataUpdate) {
        match update {
            MarketDataUpdate::Quote(quote) => self.on_quote_update(quote).await,
            MarketDataUpdate::CurveInput(input) => self.on_curve_input_update(input).await,
            MarketDataUpdate::Curve(curve) => self.on_curve_update(curve).await,
            MarketDataUpdate::IndexFixing(fixing) => self.on_index_fixing(fixing).await,
            MarketDataUpdate::InflationFixing(fixing) => self.on_inflation_fixing(fixing).await,
            MarketDataUpdate::FxRate(rate) => self.on_fx_rate_update(rate).await,
            MarketDataUpdate::VolSurface(surface) => self.on_vol_surface_update(surface).await,
        }
    }

    /// Handle a quote update.
    async fn on_quote_update(&self, update: QuoteUpdate) {
        debug!(
            "Quote update: {} bid={:?} ask={:?}",
            update.instrument_id, update.bid, update.ask
        );

        let node_id = NodeId::Quote {
            instrument_id: update.instrument_id.clone(),
        };

        // Update cache
        self.calc_graph.update_cache(
            &node_id,
            NodeValue::Quote {
                bid: update.bid,
                ask: update.ask,
                mid: update.mid,
            },
        );

        // Invalidate the quote node (propagates to dependent bond prices)
        self.calc_graph.invalidate(&node_id);

        // Check if dependent bond price should be calculated (respecting throttle)
        let bond_node = NodeId::BondPrice {
            instrument_id: update.instrument_id.clone(),
        };

        if self.throttle_manager.should_calculate(&bond_node) {
            self.throttle_manager.mark_calculated(&bond_node);
            self.notify_update(bond_node, UpdateSource::Immediate);
        } else {
            self.throttle_manager.schedule(bond_node);
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
    pub fn publish_quote(
        &self,
        update: QuoteUpdate,
    ) -> Result<(), broadcast::error::SendError<MarketDataUpdate>> {
        self.update_tx
            .send(MarketDataUpdate::Quote(update))
            .map(|_| ())
    }

    /// Publish a curve update.
    pub fn publish_curve(
        &self,
        update: CurveUpdate,
    ) -> Result<(), broadcast::error::SendError<MarketDataUpdate>> {
        self.update_tx
            .send(MarketDataUpdate::Curve(update))
            .map(|_| ())
    }

    /// Publish a curve input update.
    pub fn publish_curve_input(
        &self,
        update: CurveInputUpdate,
    ) -> Result<(), broadcast::error::SendError<MarketDataUpdate>> {
        self.update_tx
            .send(MarketDataUpdate::CurveInput(update))
            .map(|_| ())
    }

    /// Publish an index fixing.
    pub fn publish_index_fixing(
        &self,
        update: IndexFixingUpdate,
    ) -> Result<(), broadcast::error::SendError<MarketDataUpdate>> {
        self.update_tx
            .send(MarketDataUpdate::IndexFixing(update))
            .map(|_| ())
    }

    /// Publish an inflation fixing.
    pub fn publish_inflation_fixing(
        &self,
        update: InflationFixingUpdate,
    ) -> Result<(), broadcast::error::SendError<MarketDataUpdate>> {
        self.update_tx
            .send(MarketDataUpdate::InflationFixing(update))
            .map(|_| ())
    }

    /// Publish an FX rate update.
    pub fn publish_fx_rate(
        &self,
        update: FxRateUpdate,
    ) -> Result<(), broadcast::error::SendError<MarketDataUpdate>> {
        self.update_tx
            .send(MarketDataUpdate::FxRate(update))
            .map(|_| ())
    }

    /// Publish a volatility surface update.
    pub fn publish_vol_surface(
        &self,
        update: VolSurfaceUpdate,
    ) -> Result<(), broadcast::error::SendError<MarketDataUpdate>> {
        self.update_tx
            .send(MarketDataUpdate::VolSurface(update))
            .map(|_| ())
    }
}

impl Default for MarketDataPublisher {
    fn default() -> Self {
        Self::new().0
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_data_publisher() {
        let (publisher, _rx) = MarketDataPublisher::new();

        let update = QuoteUpdate {
            instrument_id: InstrumentId::new("TEST"),
            bid: Some(Decimal::from(99)),
            ask: Some(Decimal::from(101)),
            mid: Some(Decimal::from(100)),
            timestamp: 0,
        };

        assert!(publisher.publish_quote(update).is_ok());
    }

    #[test]
    fn test_quote_update_creation() {
        let update = QuoteUpdate {
            instrument_id: InstrumentId::new("US912810TD00"),
            bid: Some(Decimal::from(99)),
            ask: Some(Decimal::from(101)),
            mid: Some(Decimal::from(100)),
            timestamp: chrono::Utc::now().timestamp(),
        };

        assert_eq!(update.instrument_id.as_str(), "US912810TD00");
        assert_eq!(update.bid, Some(Decimal::from(99)));
    }
}
