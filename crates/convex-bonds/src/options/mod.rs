//! Option pricing models for callable and puttable bonds.
//!
//! This module provides:
//!
//! - **Binomial Tree**: Interest rate tree for backward induction pricing
//! - **Short Rate Model**: Hull-White (other models are not implemented)
//! - **Checked HW1F**: Explicit parameters, bounded moment lattice and analytic
//!   dated-payment European options in [`hw1f`]
//! - **OAS Support**: Option-adjusted spread calculation infrastructure
//!
//! # Overview
//!
//! Callable/puttable bonds require modeling interest rate uncertainty to value
//! the embedded options. This module provides the lattice-based framework
//! used by the existing compatibility APIs. New supplied optional-bond workflows
//! use the separately checked API and require scoped convergence/model evidence.
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
pub mod hw1f;
pub mod models;
pub mod swaption_hw1f;
pub mod trinomial_tree;

pub use binomial_tree::BinomialTree;
pub use models::{HullWhite, ModelError, ShortRateModel};
pub use swaption_hw1f::{
    bachelier_atm_price, forward_annuity, forward_swap_rate, payer_swaption_hw1f,
};
pub use trinomial_tree::{build_event_grid, TrinomialTree};
