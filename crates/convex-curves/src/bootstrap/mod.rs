//! Curve bootstrap algorithms.
//!
//! This module provides algorithms for constructing yield curves from
//! market instruments.
//!
//! # Bootstrap Methods
//!
//! - **Sequential Bootstrap**: Solves for each instrument's discount factor
//!   sequentially, using previously solved values. Fast and simple.
//!
//! - **Global Bootstrap**: Fits all parameters simultaneously by minimizing
//!   the sum of squared pricing errors. Better for noisy data or when
//!   smoothness is important.
//!
//! - **Iterative Multi-Curve**: Bootstraps coupled discount and projection
//!   curves iteratively until convergence. Required for multi-curve frameworks.
//!
//! # Example: Sequential Bootstrap
//!
//! ```rust,ignore
//! use convex_curves::bootstrap::SequentialBootstrapper;
//! use convex_curves::instruments::{Deposit, Swap};
//!
//! let curve = SequentialBootstrapper::new(reference_date)
//!     .add_instrument(Deposit::new(spot, end_3m, 0.05))
//!     .add_instrument(Deposit::new(spot, end_6m, 0.052))
//!     .add_instrument(Swap::new(spot, end_2y, 0.045, Frequency::SemiAnnual))
//!     .bootstrap()?;
//!
//! let df = curve.discount_factor(1.5)?;
//! ```
//!
//! # Example: Multi-Curve Bootstrap
//!
//! ```rust,ignore
//! use convex_curves::bootstrap::IterativeMultiCurveBootstrapper;
//!
//! let result = IterativeMultiCurveBootstrapper::new(reference_date)
//!     .add_ois_instrument(ois_1y)
//!     .add_ois_instrument(ois_5y)
//!     .add_projection_instrument(swap_3m_2y)
//!     .bootstrap()?;
//!
//! let discount_curve = result.discount_curve;
//! let projection_curve = result.projection_curve;
//! ```

mod global;
mod iterative;
mod sequential;

pub use global::{
    GlobalBootstrapConfig, GlobalBootstrapDiagnostics, GlobalBootstrapper, GlobalCurveType,
};
pub use iterative::{IterativeBootstrapConfig, IterativeMultiCurveBootstrapper, MultiCurveResult};
pub use sequential::{
    bootstrap_discount_curve, SequentialBootstrapConfig, SequentialBootstrapper,
};
