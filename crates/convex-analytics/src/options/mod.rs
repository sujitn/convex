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
//! use convex_analytics::options::{BinomialTree, HullWhite, ShortRateModel};
//!
//! // Create Hull-White model
//! let model = HullWhite::new(0.03, 0.01); // 3% mean reversion, 1% vol
//!
//! // Build tree for 5-year maturity, 100 steps
//! let tree = model.build_tree(&curve, 5.0, 100);
//!
//! // Price with backward induction
//! let price = tree.backward_induction_simple(100.0, oas);
//! ```

pub mod binomial_tree;
pub mod models;

pub use binomial_tree::BinomialTree;
pub use models::{HullWhite, ModelError, ShortRateModel};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Verify all types are accessible
        let model = HullWhite::default_params();
        assert_eq!(model.name(), "Hull-White");
    }

    #[test]
    fn test_full_workflow() {
        // Create model
        let model = HullWhite::new(0.03, 0.01);

        // Build tree with flat curve
        let flat_curve = |_t: f64| 0.05;
        let tree = model.build_tree(&flat_curve, 2.0, 20);

        // Price zero coupon bond
        let pv = tree.backward_induction_simple(100.0, 0.0);

        // Should be approximately 100 * exp(-0.05 * 2) ≈ 90.48
        assert!((pv - 90.48).abs() < 2.0);
    }
}
