//! Generic cash flow pricing framework for fixed income analytics.
//!
//! This crate provides reusable pricing primitives that are independent of
//! specific instrument types. It implements the [`CashFlowPricer`], [`SpreadSolver`],
//! and [`YieldSolver`] traits from `convex-core`.
//!
//! # Design Philosophy
//!
//! The pricing framework follows these principles:
//!
//! 1. **Instrument Agnostic**: Works with any `Vec<CashFlow>`, not specific bond types
//! 2. **Curve Generic**: Works with any `Curve` implementation
//! 3. **Configurable**: Uses `ConfigurableCalculator` for solver settings
//! 4. **Composable**: Components can be combined for complex calculations
//!
//! # Modules
//!
//! - [`pricer`]: Core present value calculation
//! - [`spread`]: Z-spread and generic spread solving
//! - [`yield_calc`]: Yield-to-maturity calculation
//! - [`shifted`]: Curve shifting utilities
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_pricing::{CurvePricer, GenericSpreadSolver};
//! use convex_core::traits::CashFlowPricer;
//!
//! // Create a pricer with a discount curve
//! let pricer = CurvePricer::new(&discount_curve);
//!
//! // Price cash flows
//! let pv = pricer.present_value(&cash_flows, settlement)?;
//!
//! // Calculate Z-spread
//! let solver = GenericSpreadSolver::new(&discount_curve);
//! let z_spread = solver.solve_spread(&cash_flows, dirty_price, settlement)?;
//! ```

pub mod error;
pub mod pricer;
pub mod shifted;
pub mod spread;
pub mod yield_calc;

pub use error::{PricingError, PricingResult};
pub use pricer::CurvePricer;
pub use shifted::{BlendedCurve, ScaledCurve, ShiftedCurve};
pub use spread::GenericSpreadSolver;
pub use yield_calc::GenericYieldSolver;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use super::error::{PricingError, PricingResult};
    pub use super::pricer::CurvePricer;
    pub use super::shifted::{BlendedCurve, ScaledCurve, ShiftedCurve};
    pub use super::spread::GenericSpreadSolver;
    pub use super::yield_calc::GenericYieldSolver;

    // Re-export core traits
    pub use convex_core::traits::{CashFlowPricer, SpreadSolver, YieldSolver};
}
