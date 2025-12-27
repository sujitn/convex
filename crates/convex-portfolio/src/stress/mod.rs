//! Stress testing for portfolios.
//!
//! This module provides stress testing capabilities including:
//! - Scenario definitions (parallel shifts, key rate shocks, spread shocks)
//! - Impact calculations based on duration and convexity
//! - Multi-scenario analysis
//!
//! All calculations are based on pre-calculated analytics from holdings.
//! No curve repricing is performed - impacts are approximated using
//! duration and convexity.

mod impact;
mod scenarios;

pub use impact::*;
pub use scenarios::*;
