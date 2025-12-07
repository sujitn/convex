//! Option pricing models for callable and puttable bonds.
//!
//! This module provides:
//!
//! - **Binomial Tree**: Interest rate tree for backward induction pricing
//! - **Short Rate Models**: Hull-White, Black-Derman-Toy for tree construction
//! - **OAS Support**: Option-adjusted spread calculation infrastructure
//!
//! # Overview
//!
//! Callable/puttable bonds require modeling interest rate uncertainty to value
//! the embedded options. This module provides the lattice-based framework
//! commonly used in production systems.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_bonds::options::{BinomialTree, HullWhite, ShortRateModel};
//!
//! // Create Hull-White model
//! let model = HullWhite::new(0.03, 0.01); // 3% mean reversion, 1% vol
//!
//! // Build tree for 5-year maturity, 100 steps
//! let tree = model.build_tree(&curve, 5.0, 100);
//!
//! // Price with backward induction
//! let price = tree.backward_induction(&cash_flows, &call_schedule, oas);
//! ```

pub mod binomial_tree;
pub mod models;

pub use binomial_tree::BinomialTree;
pub use models::{HullWhite, ModelError, ShortRateModel};
