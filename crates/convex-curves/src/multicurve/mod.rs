//! Multi-curve pricing environment.
//!
//! This module provides a complete curve environment for multi-curve
//! pricing, managing:
//!
//! - OIS discount curves (SOFR, ESTR, SONIA)
//! - Index projection curves (SOFR, Euribor, etc.)
//! - Credit curves per issuer
//! - Government benchmark curves
//! - FX forward curves
//!
//! # Multi-Curve Framework
//!
//! Post-2008 best practice requires separate curves for:
//! - **Discounting**: OIS-based, considered risk-free
//! - **Projection**: Index-specific for floating leg calculations
//!
//! This environment manages all curves needed for accurate pricing.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_curves::multicurve::{MultiCurveEnvironment, RateIndex, Currency};
//!
//! // Build environment with SOFR for USD
//! let env = MultiCurveEnvironment::builder(today)
//!     .ois_curve(RateIndex::Sofr, sofr_curve)
//!     .projection(RateIndex::Euribor3M, euribor_curve)
//!     .credit("AAPL", apple_credit)
//!     .build()?;
//!
//! // Use for pricing
//! let df = env.discount_factor(Currency::Usd, payment_date)?;
//! let fwd = env.forward_rate(RateIndex::Sofr, start, end)?;
//! ```
//!
//! # Rate Indices
//!
//! The [`RateIndex`] enum defines standard rate indices:
//!
//! - **Overnight rates**: SOFR (USD), ESTR (EUR), SONIA (GBP), TONAR (JPY), SARON (CHF)
//! - **Term rates**: Euribor 1M/3M/6M/12M, TIBOR 3M
//!
//! Each index knows its currency, day count convention, and fixing lag.

mod environment;
mod index;

pub use environment::{MultiCurveEnvironment, MultiCurveEnvironmentBuilder};
pub use index::{Currency, CurrencyPair, RateIndex, Tenor};
