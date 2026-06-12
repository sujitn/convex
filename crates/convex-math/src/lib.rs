//! # Convex Math
//!
//! Mathematical utilities for the Convex fixed income analytics library.
//!
//! This crate provides:
//!
//! - **Solvers**: Root-finding algorithms (Newton-Raphson, Brent, Bisection)
//! - **Optimization**: Function optimization (Levenberg-Marquardt, BFGS)
//! - **Linear Algebra**: Matrix operations and decompositions
//! - **Interpolation**: Numerical interpolation methods
//! - **Extrapolation**: Curve extrapolation (Flat, Linear, UFR-convergence)
//!
//! ## Design Philosophy
//!
//! - **Performance First**: Optimized for financial calculations
//! - **Numerical Stability**: Careful handling of edge cases
//! - **Generic**: Works with `f64` and `Decimal` where appropriate

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod error;
pub mod extrapolation;
pub mod interpolation;
pub mod linear_algebra;
pub mod optimization;
pub mod solvers;
pub mod stats;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::error::{MathError, MathResult};
    pub use crate::extrapolation::{
        Extrapolator, FlatExtrapolator, LinearExtrapolator, UfrConvergence,
    };
    pub use crate::interpolation::{
        CubicSpline, Interpolator, LinearInterpolator, LogLinearInterpolator, MonotoneConvex,
        NelsonSiegel, Svensson,
    };
    pub use crate::solvers::{
        bisection, brent, hybrid, hybrid_numerical, newton_raphson, newton_raphson_numerical,
        secant, BisectionSolver, BrentSolver, HybridSolver, NewtonSolver, RootFinder, SecantSolver,
        Solver, SolverConfig, SolverResult,
    };
}

pub use error::{MathError, MathResult};
