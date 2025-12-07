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
//! - **Extrapolation**: Curve extrapolation (Flat, Linear, Smith-Wilson)
//!
//! ## Design Philosophy
//!
//! - **Performance First**: Optimized for financial calculations
//! - **Numerical Stability**: Careful handling of edge cases
//! - **Generic**: Works with `f64` and `Decimal` where appropriate

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::similar_names)]
#![allow(clippy::many_single_char_names)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::if_not_else)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::single_match_else)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::derivable_impls)]

pub mod error;
pub mod extrapolation;
pub mod interpolation;
pub mod linear_algebra;
pub mod optimization;
pub mod solvers;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::error::{MathError, MathResult};
    pub use crate::extrapolation::{
        ExtrapolationMethod, Extrapolator, FlatExtrapolator, LinearExtrapolator, SmithWilson,
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
