//! Pricing configuration types.
//!
//! This module defines configuration structures for bond pricing calculations.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, Frequency};

use crate::error::{Validate, ValidationError};

// =============================================================================
// PRICING CONFIGURATION
// =============================================================================

/// Pricing configuration for bond valuation.
///
/// Controls how bonds are priced, including yield calculation methods,
/// default day count conventions, and settlement rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingConfig {
    /// Configuration name/identifier.
    pub name: String,

    /// Description of this configuration.
    pub description: Option<String>,

    /// Default day count convention for accrued interest.
    #[serde(default = "default_day_count")]
    pub accrued_day_count: DayCountConvention,

    /// Default day count convention for yield calculations.
    #[serde(default = "default_day_count")]
    pub yield_day_count: DayCountConvention,

    /// Default compounding frequency for yield.
    #[serde(default = "default_compounding")]
    pub yield_compounding: Compounding,

    /// Default coupon frequency.
    #[serde(default = "default_frequency")]
    pub default_frequency: Frequency,

    /// Settlement days (T+n).
    #[serde(default = "default_settlement_days")]
    pub settlement_days: u32,

    /// Whether to use ex-dividend rules.
    #[serde(default)]
    pub use_ex_dividend: bool,

    /// Ex-dividend days before coupon date.
    #[serde(default = "default_ex_dividend_days")]
    pub ex_dividend_days: u32,

    /// Whether to use end-of-month convention.
    #[serde(default = "default_eom")]
    pub end_of_month: bool,

    /// Price precision (decimal places).
    #[serde(default = "default_price_precision")]
    pub price_precision: u32,

    /// Yield precision (decimal places).
    #[serde(default = "default_yield_precision")]
    pub yield_precision: u32,

    /// Newton-Raphson tolerance for yield solving.
    #[serde(default = "default_solver_tolerance")]
    pub solver_tolerance: f64,

    /// Maximum iterations for yield solving.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,

    /// Whether this configuration is read-only.
    #[serde(default)]
    pub read_only: bool,

    /// Configuration metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,

    /// Timestamp when configuration was created.
    pub created_at: DateTime<Utc>,

    /// Timestamp when configuration was last updated.
    pub updated_at: DateTime<Utc>,
}

fn default_day_count() -> DayCountConvention {
    DayCountConvention::Thirty360US
}

fn default_compounding() -> Compounding {
    Compounding::SemiAnnual
}

fn default_frequency() -> Frequency {
    Frequency::SemiAnnual
}

fn default_settlement_days() -> u32 {
    2
}

fn default_ex_dividend_days() -> u32 {
    7
}

fn default_eom() -> bool {
    true
}

fn default_price_precision() -> u32 {
    6
}

fn default_yield_precision() -> u32 {
    8
}

fn default_solver_tolerance() -> f64 {
    1e-10
}

fn default_max_iterations() -> u32 {
    100
}

impl Default for PricingConfig {
    fn default() -> Self {
        Self::us_corporate()
    }
}

impl PricingConfig {
    /// Creates a new pricing configuration with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            name: name.into(),
            description: None,
            accrued_day_count: default_day_count(),
            yield_day_count: default_day_count(),
            yield_compounding: default_compounding(),
            default_frequency: default_frequency(),
            settlement_days: default_settlement_days(),
            use_ex_dividend: false,
            ex_dividend_days: default_ex_dividend_days(),
            end_of_month: default_eom(),
            price_precision: default_price_precision(),
            yield_precision: default_yield_precision(),
            solver_tolerance: default_solver_tolerance(),
            max_iterations: default_max_iterations(),
            read_only: false,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates US Corporate bond pricing configuration.
    pub fn us_corporate() -> Self {
        Self {
            name: "US.CORPORATE".to_string(),
            description: Some("US Corporate bond pricing conventions".to_string()),
            accrued_day_count: DayCountConvention::Thirty360US,
            yield_day_count: DayCountConvention::Thirty360US,
            yield_compounding: Compounding::SemiAnnual,
            default_frequency: Frequency::SemiAnnual,
            settlement_days: 2,
            use_ex_dividend: false,
            ex_dividend_days: 0,
            end_of_month: true,
            price_precision: 6,
            yield_precision: 8,
            solver_tolerance: 1e-10,
            max_iterations: 100,
            read_only: true,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Creates US Treasury bond pricing configuration.
    pub fn us_treasury() -> Self {
        Self {
            name: "US.TREASURY".to_string(),
            description: Some("US Treasury bond pricing conventions".to_string()),
            accrued_day_count: DayCountConvention::ActActIcma,
            yield_day_count: DayCountConvention::ActActIcma,
            yield_compounding: Compounding::SemiAnnual,
            default_frequency: Frequency::SemiAnnual,
            settlement_days: 1,
            use_ex_dividend: false,
            ex_dividend_days: 0,
            end_of_month: true,
            price_precision: 6,
            yield_precision: 8,
            solver_tolerance: 1e-10,
            max_iterations: 100,
            read_only: true,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Creates UK Gilt pricing configuration.
    pub fn uk_gilt() -> Self {
        Self {
            name: "UK.GILT".to_string(),
            description: Some("UK Gilt pricing conventions".to_string()),
            accrued_day_count: DayCountConvention::ActActIcma,
            yield_day_count: DayCountConvention::ActActIcma,
            yield_compounding: Compounding::SemiAnnual,
            default_frequency: Frequency::SemiAnnual,
            settlement_days: 1,
            use_ex_dividend: true,
            ex_dividend_days: 7,
            end_of_month: false,
            price_precision: 6,
            yield_precision: 8,
            solver_tolerance: 1e-10,
            max_iterations: 100,
            read_only: true,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Creates Euro government bond pricing configuration.
    pub fn euro_govt() -> Self {
        Self {
            name: "EUR.GOVT".to_string(),
            description: Some("Euro government bond pricing conventions".to_string()),
            accrued_day_count: DayCountConvention::ActActIcma,
            yield_day_count: DayCountConvention::ActActIcma,
            yield_compounding: Compounding::Annual,
            default_frequency: Frequency::Annual,
            settlement_days: 2,
            use_ex_dividend: false,
            ex_dividend_days: 0,
            end_of_month: true,
            price_precision: 6,
            yield_precision: 8,
            solver_tolerance: 1e-10,
            max_iterations: 100,
            read_only: true,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Builder method to set description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Builder method to set settlement days.
    pub fn with_settlement_days(mut self, days: u32) -> Self {
        self.settlement_days = days;
        self.updated_at = Utc::now();
        self
    }

    /// Builder method to set day count.
    pub fn with_day_count(mut self, day_count: DayCountConvention) -> Self {
        self.accrued_day_count = day_count;
        self.yield_day_count = day_count;
        self.updated_at = Utc::now();
        self
    }

    /// Builder method to set compounding.
    pub fn with_compounding(mut self, compounding: Compounding) -> Self {
        self.yield_compounding = compounding;
        self.updated_at = Utc::now();
        self
    }

    /// Adds metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

impl Validate for PricingConfig {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        if self.name.is_empty() {
            errors.push(ValidationError::new("name", "Name cannot be empty"));
        }

        if self.settlement_days > 10 {
            errors.push(ValidationError::with_rule(
                "settlement_days",
                format!("Settlement days {} exceeds maximum of 10", self.settlement_days),
                "max_settlement_days",
            ));
        }

        if self.solver_tolerance <= 0.0 || self.solver_tolerance > 1e-4 {
            errors.push(ValidationError::with_rule(
                "solver_tolerance",
                "Solver tolerance must be between 0 and 1e-4",
                "valid_tolerance",
            ));
        }

        if self.max_iterations == 0 || self.max_iterations > 10000 {
            errors.push(ValidationError::with_rule(
                "max_iterations",
                "Max iterations must be between 1 and 10000",
                "valid_iterations",
            ));
        }

        if self.price_precision > 15 {
            errors.push(ValidationError::with_rule(
                "price_precision",
                "Price precision cannot exceed 15",
                "max_precision",
            ));
        }

        if self.yield_precision > 15 {
            errors.push(ValidationError::with_rule(
                "yield_precision",
                "Yield precision cannot exceed 15",
                "max_precision",
            ));
        }

        errors
    }
}

// =============================================================================
// SPREAD CONFIGURATION
// =============================================================================

/// Spread calculation configuration.
///
/// Controls how various spread measures are calculated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpreadConfig {
    /// Configuration name/identifier.
    pub name: String,

    /// Description of this configuration.
    pub description: Option<String>,

    /// Default benchmark curve for G-spread.
    pub default_govt_curve: Option<String>,

    /// Default swap curve for I-spread.
    pub default_swap_curve: Option<String>,

    /// Default OIS curve for discounting.
    pub default_ois_curve: Option<String>,

    /// Spread precision (decimal places in basis points).
    #[serde(default = "default_spread_precision")]
    pub spread_precision: u32,

    /// Z-spread solver tolerance.
    #[serde(default = "default_spread_tolerance")]
    pub z_spread_tolerance: f64,

    /// OAS solver tolerance.
    #[serde(default = "default_spread_tolerance")]
    pub oas_tolerance: f64,

    /// Maximum iterations for spread solving.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,

    /// Whether to include accrued interest in spread calculations.
    #[serde(default = "default_true")]
    pub include_accrued: bool,

    /// Whether this configuration is read-only.
    #[serde(default)]
    pub read_only: bool,

    /// Configuration metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,

    /// Timestamp when created.
    pub created_at: DateTime<Utc>,

    /// Timestamp when last updated.
    pub updated_at: DateTime<Utc>,
}

fn default_spread_precision() -> u32 {
    4
}

fn default_spread_tolerance() -> f64 {
    1e-8
}

fn default_true() -> bool {
    true
}

impl Default for SpreadConfig {
    fn default() -> Self {
        Self::usd()
    }
}

impl SpreadConfig {
    /// Creates a new spread configuration.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            name: name.into(),
            description: None,
            default_govt_curve: None,
            default_swap_curve: None,
            default_ois_curve: None,
            spread_precision: default_spread_precision(),
            z_spread_tolerance: default_spread_tolerance(),
            oas_tolerance: default_spread_tolerance(),
            max_iterations: default_max_iterations(),
            include_accrued: true,
            read_only: false,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates USD spread configuration.
    pub fn usd() -> Self {
        let now = Utc::now();
        Self {
            name: "USD.SPREAD".to_string(),
            description: Some("USD spread calculation configuration".to_string()),
            default_govt_curve: Some("USD.TREASURY".to_string()),
            default_swap_curve: Some("USD.SOFR.SWAP".to_string()),
            default_ois_curve: Some("USD.SOFR.OIS".to_string()),
            spread_precision: 4,
            z_spread_tolerance: 1e-8,
            oas_tolerance: 1e-8,
            max_iterations: 100,
            include_accrued: true,
            read_only: true,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates EUR spread configuration.
    pub fn eur() -> Self {
        let now = Utc::now();
        Self {
            name: "EUR.SPREAD".to_string(),
            description: Some("EUR spread calculation configuration".to_string()),
            default_govt_curve: Some("EUR.GOVT".to_string()),
            default_swap_curve: Some("EUR.EURIBOR.SWAP".to_string()),
            default_ois_curve: Some("EUR.ESTR.OIS".to_string()),
            spread_precision: 4,
            z_spread_tolerance: 1e-8,
            oas_tolerance: 1e-8,
            max_iterations: 100,
            include_accrued: true,
            read_only: true,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Builder method to set government curve.
    pub fn with_govt_curve(mut self, curve: impl Into<String>) -> Self {
        self.default_govt_curve = Some(curve.into());
        self.updated_at = Utc::now();
        self
    }

    /// Builder method to set swap curve.
    pub fn with_swap_curve(mut self, curve: impl Into<String>) -> Self {
        self.default_swap_curve = Some(curve.into());
        self.updated_at = Utc::now();
        self
    }

    /// Builder method to set OIS curve.
    pub fn with_ois_curve(mut self, curve: impl Into<String>) -> Self {
        self.default_ois_curve = Some(curve.into());
        self.updated_at = Utc::now();
        self
    }
}

impl Validate for SpreadConfig {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        if self.name.is_empty() {
            errors.push(ValidationError::new("name", "Name cannot be empty"));
        }

        if self.z_spread_tolerance <= 0.0 || self.z_spread_tolerance > 1e-4 {
            errors.push(ValidationError::with_rule(
                "z_spread_tolerance",
                "Z-spread tolerance must be between 0 and 1e-4",
                "valid_tolerance",
            ));
        }

        if self.oas_tolerance <= 0.0 || self.oas_tolerance > 1e-4 {
            errors.push(ValidationError::with_rule(
                "oas_tolerance",
                "OAS tolerance must be between 0 and 1e-4",
                "valid_tolerance",
            ));
        }

        if self.max_iterations == 0 || self.max_iterations > 10000 {
            errors.push(ValidationError::with_rule(
                "max_iterations",
                "Max iterations must be between 1 and 10000",
                "valid_iterations",
            ));
        }

        errors
    }
}

// =============================================================================
// RISK CONFIGURATION
// =============================================================================

/// Risk metrics calculation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    /// Configuration name/identifier.
    pub name: String,

    /// Description of this configuration.
    pub description: Option<String>,

    /// Basis point shift for DV01 calculation (in bps).
    #[serde(default = "default_dv01_shift")]
    pub dv01_shift_bps: Decimal,

    /// Whether to use full revaluation for DV01 (vs analytic).
    #[serde(default)]
    pub dv01_full_revaluation: bool,

    /// Key rate tenors for KRD calculation.
    #[serde(default = "default_key_rate_tenors")]
    pub key_rate_tenors: Vec<String>,

    /// Key rate shift in basis points.
    #[serde(default = "default_key_rate_shift")]
    pub key_rate_shift_bps: Decimal,

    /// Convexity shift in basis points.
    #[serde(default = "default_convexity_shift")]
    pub convexity_shift_bps: Decimal,

    /// OAS volatility for callable bond analytics.
    #[serde(default = "default_oas_volatility")]
    pub oas_volatility: f64,

    /// Number of paths for Monte Carlo simulations.
    #[serde(default = "default_mc_paths")]
    pub monte_carlo_paths: u32,

    /// Random seed for reproducibility (None = random).
    pub monte_carlo_seed: Option<u64>,

    /// Risk precision (decimal places).
    #[serde(default = "default_risk_precision")]
    pub risk_precision: u32,

    /// Whether this configuration is read-only.
    #[serde(default)]
    pub read_only: bool,

    /// Configuration metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,

    /// Timestamp when created.
    pub created_at: DateTime<Utc>,

    /// Timestamp when last updated.
    pub updated_at: DateTime<Utc>,
}

fn default_dv01_shift() -> Decimal {
    Decimal::ONE
}

fn default_key_rate_tenors() -> Vec<String> {
    vec![
        "3M".to_string(),
        "6M".to_string(),
        "1Y".to_string(),
        "2Y".to_string(),
        "3Y".to_string(),
        "5Y".to_string(),
        "7Y".to_string(),
        "10Y".to_string(),
        "20Y".to_string(),
        "30Y".to_string(),
    ]
}

fn default_key_rate_shift() -> Decimal {
    Decimal::ONE
}

fn default_convexity_shift() -> Decimal {
    Decimal::ONE
}

fn default_oas_volatility() -> f64 {
    0.15
}

fn default_mc_paths() -> u32 {
    10000
}

fn default_risk_precision() -> u32 {
    6
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self::standard()
    }
}

impl RiskConfig {
    /// Creates a new risk configuration.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            name: name.into(),
            description: None,
            dv01_shift_bps: default_dv01_shift(),
            dv01_full_revaluation: false,
            key_rate_tenors: default_key_rate_tenors(),
            key_rate_shift_bps: default_key_rate_shift(),
            convexity_shift_bps: default_convexity_shift(),
            oas_volatility: default_oas_volatility(),
            monte_carlo_paths: default_mc_paths(),
            monte_carlo_seed: None,
            risk_precision: default_risk_precision(),
            read_only: false,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates standard risk configuration.
    pub fn standard() -> Self {
        let now = Utc::now();
        Self {
            name: "STANDARD".to_string(),
            description: Some("Standard risk calculation configuration".to_string()),
            dv01_shift_bps: Decimal::ONE,
            dv01_full_revaluation: false,
            key_rate_tenors: default_key_rate_tenors(),
            key_rate_shift_bps: Decimal::ONE,
            convexity_shift_bps: Decimal::ONE,
            oas_volatility: 0.15,
            monte_carlo_paths: 10000,
            monte_carlo_seed: None,
            risk_precision: 6,
            read_only: true,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates high-precision risk configuration.
    pub fn high_precision() -> Self {
        let now = Utc::now();
        Self {
            name: "HIGH_PRECISION".to_string(),
            description: Some("High-precision risk calculation configuration".to_string()),
            dv01_shift_bps: Decimal::new(1, 1), // 0.1 bp
            dv01_full_revaluation: true,
            key_rate_tenors: default_key_rate_tenors(),
            key_rate_shift_bps: Decimal::ONE,
            convexity_shift_bps: Decimal::new(1, 1), // 0.1 bp
            oas_volatility: 0.15,
            monte_carlo_paths: 100000,
            monte_carlo_seed: None,
            risk_precision: 8,
            read_only: true,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Builder method to set DV01 shift.
    pub fn with_dv01_shift(mut self, shift_bps: Decimal) -> Self {
        self.dv01_shift_bps = shift_bps;
        self.updated_at = Utc::now();
        self
    }

    /// Builder method to set key rate tenors.
    pub fn with_key_rate_tenors(mut self, tenors: Vec<String>) -> Self {
        self.key_rate_tenors = tenors;
        self.updated_at = Utc::now();
        self
    }

    /// Builder method to set Monte Carlo paths.
    pub fn with_monte_carlo_paths(mut self, paths: u32) -> Self {
        self.monte_carlo_paths = paths;
        self.updated_at = Utc::now();
        self
    }
}

impl Validate for RiskConfig {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        if self.name.is_empty() {
            errors.push(ValidationError::new("name", "Name cannot be empty"));
        }

        if self.dv01_shift_bps <= Decimal::ZERO {
            errors.push(ValidationError::with_rule(
                "dv01_shift_bps",
                "DV01 shift must be positive",
                "positive_shift",
            ));
        }

        if self.key_rate_tenors.is_empty() {
            errors.push(ValidationError::with_rule(
                "key_rate_tenors",
                "At least one key rate tenor is required",
                "non_empty_tenors",
            ));
        }

        if self.oas_volatility <= 0.0 || self.oas_volatility > 1.0 {
            errors.push(ValidationError::with_rule(
                "oas_volatility",
                "OAS volatility must be between 0 and 1",
                "valid_volatility",
            ));
        }

        if self.monte_carlo_paths < 100 || self.monte_carlo_paths > 10_000_000 {
            errors.push(ValidationError::with_rule(
                "monte_carlo_paths",
                "Monte Carlo paths must be between 100 and 10,000,000",
                "valid_mc_paths",
            ));
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pricing_config_us_corporate() {
        let config = PricingConfig::us_corporate();
        assert_eq!(config.name, "US.CORPORATE");
        assert_eq!(config.settlement_days, 2);
        assert_eq!(config.accrued_day_count, DayCountConvention::Thirty360US);
        assert!(config.is_valid());
    }

    #[test]
    fn test_pricing_config_us_treasury() {
        let config = PricingConfig::us_treasury();
        assert_eq!(config.name, "US.TREASURY");
        assert_eq!(config.settlement_days, 1);
        assert_eq!(config.accrued_day_count, DayCountConvention::ActActIcma);
        assert!(config.is_valid());
    }

    #[test]
    fn test_pricing_config_validation() {
        let mut config = PricingConfig::new("test");
        assert!(config.is_valid());

        config.name = String::new();
        assert!(!config.is_valid());

        config.name = "test".to_string();
        config.settlement_days = 100;
        assert!(!config.is_valid());
    }

    #[test]
    fn test_spread_config_usd() {
        let config = SpreadConfig::usd();
        assert_eq!(config.name, "USD.SPREAD");
        assert_eq!(config.default_govt_curve, Some("USD.TREASURY".to_string()));
        assert!(config.is_valid());
    }

    #[test]
    fn test_risk_config_standard() {
        let config = RiskConfig::standard();
        assert_eq!(config.name, "STANDARD");
        assert_eq!(config.dv01_shift_bps, Decimal::ONE);
        assert!(config.is_valid());
    }

    #[test]
    fn test_risk_config_validation() {
        let mut config = RiskConfig::new("test");
        assert!(config.is_valid());

        config.oas_volatility = 2.0;
        assert!(!config.is_valid());
    }
}
