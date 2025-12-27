//! Portfolio representation and construction.
//!
//! This module provides the core [`Portfolio`] type and [`PortfolioBuilder`]
//! for constructing portfolios.

mod builder;
#[allow(clippy::module_inception)]
mod portfolio;

pub use builder::PortfolioBuilder;
pub use portfolio::Portfolio;
