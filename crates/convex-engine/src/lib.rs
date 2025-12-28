//! # Convex Engine
//!
//! Stateful pricing engine with calculation graph for the Convex fixed income analytics library.
//!
//! This crate provides the **stateful orchestration layer** that sits on top of the
//! pure calculation libraries (`convex-analytics`, `convex-curves`, `convex-bonds`).
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                           CONVEX ENGINE                                     │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                             │
//! │  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐                     │
//! │  │   Market    │    │   Services  │    │   Caches    │                     │
//! │  │    Data     │    │  (Bond,     │    │  (Curve,    │                     │
//! │  │  Providers  │    │   Curve,    │    │   Calc)     │                     │
//! │  │             │    │   Pricing)  │    │             │                     │
//! │  └──────┬──────┘    └──────┬──────┘    └──────┬──────┘                     │
//! │         │                  │                  │                             │
//! │         ▼                  ▼                  ▼                             │
//! │  ┌─────────────────────────────────────────────────────────────────────┐   │
//! │  │                      CALCULATION GRAPH                              │   │
//! │  │  • Dependency tracking    • Dirty flag propagation                 │   │
//! │  │  • Incremental recalc     • Memoization/caching                    │   │
//! │  └─────────────────────────────────────────────────────────────────────┘   │
//! │         │                                                                   │
//! │         ▼                                                                   │
//! │  ┌─────────────────────────────────────────────────────────────────────┐   │
//! │  │                      STREAMING / PUBLISHING                         │   │
//! │  │  • WebSocket streams      • Real-time BondQuote updates            │   │
//! │  │  • Curve snapshots        • Portfolio analytics                    │   │
//! │  └─────────────────────────────────────────────────────────────────────┘   │
//! │                                                                             │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Components
//!
//! - **[`CalculationGraph`]**: Dependency-driven calculation engine with dirty flag propagation
//! - **[`CurveCache`]**: Atomic curve caching with hot-swap support
//! - **[`PricingEngine`]**: High-level pricing orchestration
//! - **Services**: [`BondService`], [`CurveService`], [`PricingService`] traits
//! - **Enterprise Patterns**: Circuit breakers, health checks, graceful shutdown
//!
//! ## Example
//!
//! ```rust,ignore
//! use convex_engine::prelude::*;
//!
//! // Create the calculation graph
//! let graph = CalculationGraph::new();
//!
//! // Register curve nodes
//! graph.register(CurveNode::new("USD.GOVT"));
//! graph.register(CurveNode::new("USD.SOFR.OIS"));
//!
//! // Register bond pricing nodes (depend on curves)
//! let bond_node = BondPricingNode::new("US912828Z229")
//!     .depends_on("USD.GOVT")
//!     .depends_on("USD.SOFR.OIS");
//! graph.register(bond_node);
//!
//! // When market data changes, invalidate and recalculate
//! graph.invalidate(&NodeId::Curve("USD.GOVT".into()));
//! let recalculated = graph.recalculate();
//! ```
//!
//! ## Separation of Concerns
//!
//! **This crate is STATEFUL.** It manages:
//! - Curve caches and their lifecycle
//! - Calculation state and memoization
//! - Service instances and their dependencies
//! - Real-time subscriptions and streaming
//!
//! The pure calculation logic lives in `convex-analytics`, `convex-curves`, and `convex-bonds`.
//! This crate wraps them with caching, orchestration, and runtime patterns.

#![warn(missing_docs)]
#![warn(clippy::all)]
#![deny(unsafe_code)]

// Core modules
pub mod error;
pub mod graph;
pub mod nodes;
pub mod cache;
pub mod services;
pub mod runtime;
pub mod streaming;
pub mod engine;

// Re-export core types
pub use error::{EngineError, EngineResult};
pub use graph::{CalculationGraph, NodeId, NodeValue, Revision};
pub use cache::{CurveCache, CacheStats};
pub use engine::PricingEngine;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::error::{EngineError, EngineResult};

    // Graph
    pub use crate::graph::{
        CalculationGraph, NodeId, NodeValue, CachedValue, Revision,
    };
    pub use crate::nodes::{
        CalculationNode, NodeType, CurveNode, BondPricingNode, PortfolioNode,
    };

    // Cache
    pub use crate::cache::{CurveCache, CacheStats, CacheEntry};

    // Services
    pub use crate::services::{
        BondService, CurveService, PricingService, OverrideService,
        BondFilter, CurveFilter,
    };

    // Runtime
    pub use crate::runtime::{
        HealthCheck, HealthStatus, ServiceStatus,
        GracefulShutdown,
        CircuitBreaker, CircuitBreakerConfig, CircuitState,
        RetryConfig, RateLimiter,
    };

    // Streaming
    pub use crate::streaming::{
        BondQuote, QuoteSide, QuoteCondition, QuoteSource,
        StreamPublisher, StreamSubscriber,
    };

    // Engine
    pub use crate::engine::{PricingEngine, PricingEngineConfig};
}
