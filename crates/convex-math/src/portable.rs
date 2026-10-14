//! Shared binary64 transcendental kernels for native and WebAssembly valuation.
//!
//! The locked pure Rust `libm` implementation uses `force-soft-floats` on every
//! target. Standard floating-point methods can use different platform libraries.
//! These IEEE kernels do not admit financial inputs: calculation owners retain
//! their domain, finiteness, representability, residual and work-budget checks.
//! No result rounding, output filtering or host-specific numerical policy occurs.

pub use libm::{exp, expm1, hypot, log, log1p, pow};
