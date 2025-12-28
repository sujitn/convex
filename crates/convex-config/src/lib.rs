//! Convex Configuration Layer
//!
//! This crate provides configuration management for the Convex fixed income
//! analytics library. It supports pricing configurations, curve build settings,
//! spread calculation parameters, and risk calculation options.
//!
//! # Features
//!
//! - **Pricing Configuration**: Day count conventions, settlement rules, solver settings
//! - **Curve Configuration**: Interpolation methods, calibration settings, instrument specs
//! - **Spread Configuration**: Benchmark curves, solver tolerances
//! - **Risk Configuration**: DV01 shifts, key rate tenors, Monte Carlo settings
//! - **Override System**: Runtime configuration overrides with priority and scoping
//! - **Storage Integration**: Persistent configuration storage via convex-storage
//!
//! # Example
//!
//! ```rust
//! use convex_config::{ConfigManager, PricingConfig, CurveConfig};
//!
//! // Create a configuration manager
//! let manager = ConfigManager::new();
//!
//! // Get standard US corporate pricing config
//! let pricing = manager.get_pricing("US.CORPORATE").unwrap();
//! assert_eq!(pricing.settlement_days, 2);
//!
//! // Get SOFR OIS curve config
//! let curve = manager.get_curve("USD.SOFR.OIS").unwrap();
//! assert_eq!(curve.settlement_lag, 2);
//!
//! // Register a custom configuration
//! let custom = PricingConfig::new("MY.CUSTOM")
//!     .with_settlement_days(3)
//!     .with_description("Custom pricing rules");
//! manager.register_pricing(custom).unwrap();
//! ```
//!
//! # Configuration Overrides
//!
//! The override system allows runtime modification of configuration values
//! with priority-based resolution:
//!
//! ```rust
//! use convex_config::{ConfigManager, ConfigOverride, OverridePriority, OverrideScope};
//! use serde_json::json;
//!
//! let manager = ConfigManager::new();
//!
//! // Add a USD-specific override for settlement days
//! let override_item = ConfigOverride::new("pricing", "settlement_days", json!(3))
//!     .with_priority(OverridePriority::User)
//!     .with_scope(OverrideScope::Currency("USD".to_string()))
//!     .with_reason("Holiday adjustment");
//!
//! manager.add_override(override_item).unwrap();
//!
//! // Get config with overrides applied
//! let context = manager.context().with_currency("USD");
//! let config = manager.get_pricing_with_context("US.CORPORATE", &context);
//! ```
//!
//! # Standard Configurations
//!
//! The following standard configurations are loaded by default:
//!
//! ## Pricing Configurations
//! - `US.CORPORATE` - US corporate bond conventions
//! - `US.TREASURY` - US Treasury conventions
//! - `UK.GILT` - UK Gilt conventions (with ex-dividend)
//! - `EUR.GOVT` - Euro government bond conventions
//!
//! ## Curve Configurations
//! - `USD.SOFR.OIS` - USD SOFR OIS curve
//! - `EUR.ESTR.OIS` - EUR €STR OIS curve
//! - `GBP.SONIA.OIS` - GBP SONIA OIS curve
//! - `USD.GOVT`, `EUR.GOVT`, `GBP.GOVT` - Government bond curves
//!
//! ## Spread Configurations
//! - `USD.SPREAD` - USD spread calculation settings
//! - `EUR.SPREAD` - EUR spread calculation settings
//!
//! ## Risk Configurations
//! - `STANDARD` - Standard risk calculation settings
//! - `HIGH_PRECISION` - High-precision risk settings

#![warn(missing_docs)]
#![warn(clippy::all)]
#![deny(unsafe_code)]

mod curve;
mod error;
mod manager;
mod override_system;
mod pricing;

// Re-export core types
pub use curve::{
    CalibrationMethod, CurveConfig, ExtrapolationMethod, InstrumentConfig, InstrumentType,
    InterpolationMethod,
};
pub use error::{ConfigError, ConfigResult, Validate, ValidationError};
pub use manager::{ConfigManager, ConfigType};
pub use override_system::{
    ApplyOverrides, ConfigOverride, OverrideContext, OverridePriority, OverrideScope, OverrideSet,
};
pub use pricing::{PricingConfig, RiskConfig, SpreadConfig};

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::curve::{CurveConfig, InterpolationMethod};
    pub use crate::error::{ConfigError, ConfigResult, Validate};
    pub use crate::manager::ConfigManager;
    pub use crate::override_system::{ConfigOverride, OverrideContext, OverridePriority};
    pub use crate::pricing::{PricingConfig, RiskConfig, SpreadConfig};
}
