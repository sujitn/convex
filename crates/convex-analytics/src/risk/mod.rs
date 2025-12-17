//! Risk analytics for fixed income instruments.
//!
//! This module provides comprehensive risk calculations including:
//!
//! - **Duration**: Macaulay, Modified, Effective, Key Rate, Spread
//! - **Convexity**: Analytical and Effective
//! - **DV01/PV01**: Dollar value of a basis point
//! - **VaR**: Value at Risk (Historical and Parametric)
//! - **Hedging**: Hedge ratios and portfolio risk
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_analytics::risk::prelude::*;
//!
//! let calc = BondRiskCalculator::from_cash_flows(
//!     times, cash_flows,
//!     0.05,   // YTM
//!     2,      // semi-annual
//!     100.0,  // dirty price
//!     100.0,  // face value
//! )?;
//!
//! let metrics = calc.all_metrics()?;
//! println!("Modified Duration: {}", metrics.modified_duration);
//! println!("Convexity: {}", metrics.convexity);
//! println!("DV01: {}", metrics.dv01);
//! ```

pub mod calculator;
pub mod convexity;
pub mod duration;
pub mod dv01;
pub mod hedging;
pub mod var;

// Re-export main types and functions
pub use calculator::{
    BondRiskCalculator, BondRiskMetrics, EffectiveDurationCalculator, KeyRateDurationCalculator,
};
pub use convexity::{
    analytical_convexity, effective_convexity, price_change_with_convexity, Convexity,
};
pub use duration::{
    effective_duration, key_rate_duration_at_tenor, macaulay_duration, modified_duration,
    modified_from_macaulay, price_change_from_duration, spread_duration, Duration, KeyRateDuration,
    KeyRateDurations, DEFAULT_BUMP_SIZE, SMALL_BUMP_SIZE, STANDARD_KEY_RATE_TENORS,
};
pub use dv01::{dv01_from_duration, dv01_from_prices, dv01_per_100_face, notional_from_dv01, DV01};
pub use hedging::{
    aggregate_portfolio_risk, duration_hedge_ratio, dv01_hedge_ratio, HedgeDirection,
    HedgeRecommendation, PortfolioRisk, Position,
};
pub use var::{historical_var, parametric_var, parametric_var_from_dv01, VaRMethod, VaRResult};

/// Prelude for convenient imports
pub mod prelude {
    pub use super::calculator::*;
    pub use super::convexity::*;
    pub use super::duration::*;
    pub use super::dv01::*;
    pub use super::hedging::*;
    pub use super::var::*;
}
