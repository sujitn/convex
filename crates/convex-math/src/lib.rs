// The crate documentation is the README, so docs.rs and crates.io show the
// same text and its examples run as doctests.
#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![warn(clippy::all)]
#![forbid(unsafe_code)]
#![deny(unused_must_use)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

pub mod bounds;
/// Bounded exact Decimal accumulation and final rounding.
pub mod decimal;
pub mod error;
pub mod extrapolation;
pub mod interpolation;
pub mod linear_algebra;
pub mod optimization;
pub mod portable;
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
