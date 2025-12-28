//! Curve configuration types.
//!
//! This module defines configuration structures for yield curve construction.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use convex_core::daycounts::DayCountConvention;
use convex_core::types::Compounding;

use crate::error::{Validate, ValidationError};

// =============================================================================
// INTERPOLATION METHOD
// =============================================================================

/// Interpolation method for curve construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum InterpolationMethod {
    /// Linear interpolation on zero rates.
    Linear,

    /// Linear interpolation on log discount factors.
    LogLinear,

    /// Cubic spline interpolation.
    CubicSpline,

    /// Monotone convex spline (preserves shape).
    #[default]
    MonotoneConvex,

    /// Natural cubic spline.
    NaturalCubic,

    /// Flat forward rates (piecewise constant).
    FlatForward,

    /// Hermite spline interpolation.
    Hermite,

    /// Tension spline interpolation.
    TensionSpline,
}

impl InterpolationMethod {
    /// Returns whether this method preserves monotonicity.
    pub fn is_monotone_preserving(&self) -> bool {
        matches!(self, Self::MonotoneConvex | Self::LogLinear | Self::FlatForward)
    }

    /// Returns whether this method produces smooth forward rates.
    pub fn is_smooth_forwards(&self) -> bool {
        matches!(
            self,
            Self::CubicSpline | Self::NaturalCubic | Self::MonotoneConvex | Self::Hermite
        )
    }
}

// =============================================================================
// CALIBRATION METHOD
// =============================================================================

/// Calibration method for curve bootstrapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum CalibrationMethod {
    /// Piecewise bootstrap (iterative).
    #[default]
    Piecewise,

    /// Global fit using Levenberg-Marquardt.
    GlobalFit,

    /// Global fit using gradient descent.
    GradientDescent,

    /// Parametric fit (Nelson-Siegel family).
    Parametric,
}

// =============================================================================
// CURVE CONFIGURATION
// =============================================================================

/// Configuration for yield curve construction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveConfig {
    /// Configuration name/identifier.
    pub name: String,

    /// Description of this configuration.
    pub description: Option<String>,

    /// Interpolation method.
    #[serde(default)]
    pub interpolation: InterpolationMethod,

    /// Extrapolation method for points beyond the curve.
    #[serde(default)]
    pub extrapolation: ExtrapolationMethod,

    /// Calibration method.
    #[serde(default)]
    pub calibration: CalibrationMethod,

    /// Day count convention for the curve.
    #[serde(default = "default_curve_day_count")]
    pub day_count: DayCountConvention,

    /// Compounding convention.
    #[serde(default = "default_curve_compounding")]
    pub compounding: Compounding,

    /// Calibration tolerance.
    #[serde(default = "default_calibration_tolerance")]
    pub calibration_tolerance: f64,

    /// Maximum calibration iterations.
    #[serde(default = "default_calibration_iterations")]
    pub max_iterations: u32,

    /// Whether to use turn-of-year adjustments.
    #[serde(default)]
    pub turn_of_year_adjustment: bool,

    /// Turn-of-year dates for adjustment.
    #[serde(default)]
    pub turn_of_year_dates: Vec<String>,

    /// Settlement lag in days.
    #[serde(default = "default_settlement_lag")]
    pub settlement_lag: u32,

    /// Instrument configurations for bootstrapping.
    #[serde(default)]
    pub instruments: Vec<InstrumentConfig>,

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

fn default_curve_day_count() -> DayCountConvention {
    DayCountConvention::Act360
}

fn default_curve_compounding() -> Compounding {
    Compounding::Continuous
}

fn default_calibration_tolerance() -> f64 {
    1e-10
}

fn default_calibration_iterations() -> u32 {
    100
}

fn default_settlement_lag() -> u32 {
    2
}

/// Extrapolation method for curve ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ExtrapolationMethod {
    /// No extrapolation (error if out of bounds).
    None,

    /// Flat extrapolation (last known value).
    #[default]
    Flat,

    /// Linear extrapolation.
    Linear,

    /// Decay to long-term rate.
    Decay,
}

impl Default for CurveConfig {
    fn default() -> Self {
        Self::sofr_ois()
    }
}

impl CurveConfig {
    /// Creates a new curve configuration.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            name: name.into(),
            description: None,
            interpolation: InterpolationMethod::default(),
            extrapolation: ExtrapolationMethod::default(),
            calibration: CalibrationMethod::default(),
            day_count: default_curve_day_count(),
            compounding: default_curve_compounding(),
            calibration_tolerance: default_calibration_tolerance(),
            max_iterations: default_calibration_iterations(),
            turn_of_year_adjustment: false,
            turn_of_year_dates: Vec::new(),
            settlement_lag: default_settlement_lag(),
            instruments: Vec::new(),
            read_only: false,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates SOFR OIS curve configuration.
    pub fn sofr_ois() -> Self {
        let now = Utc::now();
        Self {
            name: "USD.SOFR.OIS".to_string(),
            description: Some("USD SOFR OIS curve configuration".to_string()),
            interpolation: InterpolationMethod::MonotoneConvex,
            extrapolation: ExtrapolationMethod::Flat,
            calibration: CalibrationMethod::Piecewise,
            day_count: DayCountConvention::Act360,
            compounding: Compounding::Continuous,
            calibration_tolerance: 1e-10,
            max_iterations: 100,
            turn_of_year_adjustment: true,
            turn_of_year_dates: vec!["12-31".to_string(), "01-01".to_string()],
            settlement_lag: 2,
            instruments: vec![
                InstrumentConfig::deposit("1D", 0),
                InstrumentConfig::ois("1W", 2),
                InstrumentConfig::ois("2W", 2),
                InstrumentConfig::ois("1M", 2),
                InstrumentConfig::ois("2M", 2),
                InstrumentConfig::ois("3M", 2),
                InstrumentConfig::ois("6M", 2),
                InstrumentConfig::ois("9M", 2),
                InstrumentConfig::ois("1Y", 2),
                InstrumentConfig::ois("2Y", 2),
                InstrumentConfig::ois("3Y", 2),
                InstrumentConfig::ois("5Y", 2),
                InstrumentConfig::ois("7Y", 2),
                InstrumentConfig::ois("10Y", 2),
                InstrumentConfig::ois("15Y", 2),
                InstrumentConfig::ois("20Y", 2),
                InstrumentConfig::ois("30Y", 2),
            ],
            read_only: true,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates ESTR OIS curve configuration.
    pub fn estr_ois() -> Self {
        let now = Utc::now();
        Self {
            name: "EUR.ESTR.OIS".to_string(),
            description: Some("EUR €STR OIS curve configuration".to_string()),
            interpolation: InterpolationMethod::MonotoneConvex,
            extrapolation: ExtrapolationMethod::Flat,
            calibration: CalibrationMethod::Piecewise,
            day_count: DayCountConvention::Act360,
            compounding: Compounding::Continuous,
            calibration_tolerance: 1e-10,
            max_iterations: 100,
            turn_of_year_adjustment: true,
            turn_of_year_dates: vec!["12-31".to_string(), "01-01".to_string()],
            settlement_lag: 2,
            instruments: vec![
                InstrumentConfig::deposit("1D", 0),
                InstrumentConfig::ois("1W", 2),
                InstrumentConfig::ois("1M", 2),
                InstrumentConfig::ois("3M", 2),
                InstrumentConfig::ois("6M", 2),
                InstrumentConfig::ois("1Y", 2),
                InstrumentConfig::ois("2Y", 2),
                InstrumentConfig::ois("5Y", 2),
                InstrumentConfig::ois("10Y", 2),
                InstrumentConfig::ois("30Y", 2),
            ],
            read_only: true,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates SONIA OIS curve configuration.
    pub fn sonia_ois() -> Self {
        let now = Utc::now();
        Self {
            name: "GBP.SONIA.OIS".to_string(),
            description: Some("GBP SONIA OIS curve configuration".to_string()),
            interpolation: InterpolationMethod::MonotoneConvex,
            extrapolation: ExtrapolationMethod::Flat,
            calibration: CalibrationMethod::Piecewise,
            day_count: DayCountConvention::Act365Fixed,
            compounding: Compounding::Continuous,
            calibration_tolerance: 1e-10,
            max_iterations: 100,
            turn_of_year_adjustment: false,
            turn_of_year_dates: Vec::new(),
            settlement_lag: 0, // SONIA is T+0
            instruments: vec![
                InstrumentConfig::deposit("1D", 0),
                InstrumentConfig::ois("1W", 0),
                InstrumentConfig::ois("1M", 0),
                InstrumentConfig::ois("3M", 0),
                InstrumentConfig::ois("6M", 0),
                InstrumentConfig::ois("1Y", 0),
                InstrumentConfig::ois("2Y", 0),
                InstrumentConfig::ois("5Y", 0),
                InstrumentConfig::ois("10Y", 0),
                InstrumentConfig::ois("30Y", 0),
            ],
            read_only: true,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates government bond curve configuration.
    pub fn govt_bond(currency: &str) -> Self {
        let now = Utc::now();
        let (day_count, settlement_lag) = match currency {
            "USD" => (DayCountConvention::ActActIcma, 1),
            "EUR" => (DayCountConvention::ActActIcma, 2),
            "GBP" => (DayCountConvention::ActActIcma, 1),
            _ => (DayCountConvention::ActActIcma, 2),
        };

        Self {
            name: format!("{}.GOVT", currency),
            description: Some(format!("{} government bond curve configuration", currency)),
            interpolation: InterpolationMethod::MonotoneConvex,
            extrapolation: ExtrapolationMethod::Flat,
            calibration: CalibrationMethod::GlobalFit,
            day_count,
            compounding: Compounding::SemiAnnual,
            calibration_tolerance: 1e-8,
            max_iterations: 200,
            turn_of_year_adjustment: false,
            turn_of_year_dates: Vec::new(),
            settlement_lag,
            instruments: Vec::new(), // Bonds are added dynamically
            read_only: false,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Builder method to set interpolation.
    pub fn with_interpolation(mut self, method: InterpolationMethod) -> Self {
        self.interpolation = method;
        self.updated_at = Utc::now();
        self
    }

    /// Builder method to set calibration method.
    pub fn with_calibration(mut self, method: CalibrationMethod) -> Self {
        self.calibration = method;
        self.updated_at = Utc::now();
        self
    }

    /// Builder method to add an instrument.
    pub fn with_instrument(mut self, instrument: InstrumentConfig) -> Self {
        self.instruments.push(instrument);
        self.updated_at = Utc::now();
        self
    }

    /// Builder method to set day count.
    pub fn with_day_count(mut self, day_count: DayCountConvention) -> Self {
        self.day_count = day_count;
        self.updated_at = Utc::now();
        self
    }
}

impl Validate for CurveConfig {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        if self.name.is_empty() {
            errors.push(ValidationError::new("name", "Name cannot be empty"));
        }

        if self.calibration_tolerance <= 0.0 || self.calibration_tolerance > 1e-4 {
            errors.push(ValidationError::with_rule(
                "calibration_tolerance",
                "Calibration tolerance must be between 0 and 1e-4",
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

        if self.settlement_lag > 5 {
            errors.push(ValidationError::with_rule(
                "settlement_lag",
                "Settlement lag cannot exceed 5 days",
                "max_settlement_lag",
            ));
        }

        // Validate instruments
        for (i, inst) in self.instruments.iter().enumerate() {
            for error in inst.validate() {
                errors.push(ValidationError::new(
                    format!("instruments[{}].{}", i, error.field),
                    error.message,
                ));
            }
        }

        errors
    }
}

// =============================================================================
// INSTRUMENT CONFIGURATION
// =============================================================================

/// Instrument type for curve bootstrapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstrumentType {
    /// Money market deposit.
    Deposit,
    /// Forward rate agreement.
    Fra,
    /// Interest rate swap.
    Swap,
    /// Overnight index swap.
    Ois,
    /// Futures contract.
    Futures,
    /// Bond.
    Bond,
    /// Basis swap.
    BasisSwap,
    /// Cross-currency swap.
    XccySwap,
}

/// Configuration for a single calibration instrument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentConfig {
    /// Instrument type.
    pub instrument_type: InstrumentType,

    /// Tenor (e.g., "3M", "2Y", "10Y").
    pub tenor: String,

    /// Settlement lag in days.
    pub settlement_days: u32,

    /// Whether this instrument is required for calibration.
    #[serde(default = "default_true")]
    pub required: bool,

    /// Weight in global fit optimization.
    #[serde(default = "default_weight")]
    pub weight: f64,

    /// Day count override for this instrument.
    pub day_count_override: Option<DayCountConvention>,

    /// Fixed leg frequency for swaps.
    pub fixed_frequency: Option<u32>,

    /// Floating leg frequency for swaps.
    pub float_frequency: Option<u32>,
}

fn default_true() -> bool {
    true
}

fn default_weight() -> f64 {
    1.0
}

impl InstrumentConfig {
    /// Creates a deposit instrument configuration.
    pub fn deposit(tenor: &str, settlement_days: u32) -> Self {
        Self {
            instrument_type: InstrumentType::Deposit,
            tenor: tenor.to_string(),
            settlement_days,
            required: true,
            weight: 1.0,
            day_count_override: None,
            fixed_frequency: None,
            float_frequency: None,
        }
    }

    /// Creates an OIS instrument configuration.
    pub fn ois(tenor: &str, settlement_days: u32) -> Self {
        Self {
            instrument_type: InstrumentType::Ois,
            tenor: tenor.to_string(),
            settlement_days,
            required: true,
            weight: 1.0,
            day_count_override: None,
            fixed_frequency: Some(1), // Annual
            float_frequency: Some(1),
        }
    }

    /// Creates a swap instrument configuration.
    pub fn swap(tenor: &str, settlement_days: u32) -> Self {
        Self {
            instrument_type: InstrumentType::Swap,
            tenor: tenor.to_string(),
            settlement_days,
            required: true,
            weight: 1.0,
            day_count_override: None,
            fixed_frequency: Some(2), // Semi-annual
            float_frequency: Some(4), // Quarterly
        }
    }

    /// Creates a FRA instrument configuration.
    pub fn fra(tenor: &str, settlement_days: u32) -> Self {
        Self {
            instrument_type: InstrumentType::Fra,
            tenor: tenor.to_string(),
            settlement_days,
            required: true,
            weight: 1.0,
            day_count_override: None,
            fixed_frequency: None,
            float_frequency: None,
        }
    }

    /// Creates a futures instrument configuration.
    pub fn futures(tenor: &str) -> Self {
        Self {
            instrument_type: InstrumentType::Futures,
            tenor: tenor.to_string(),
            settlement_days: 0,
            required: true,
            weight: 1.0,
            day_count_override: None,
            fixed_frequency: None,
            float_frequency: None,
        }
    }

    /// Builder method to set weight.
    pub fn with_weight(mut self, weight: f64) -> Self {
        self.weight = weight;
        self
    }

    /// Builder method to mark as optional.
    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }
}

impl Validate for InstrumentConfig {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        if self.tenor.is_empty() {
            errors.push(ValidationError::new("tenor", "Tenor cannot be empty"));
        }

        if self.weight <= 0.0 {
            errors.push(ValidationError::with_rule(
                "weight",
                "Weight must be positive",
                "positive_weight",
            ));
        }

        if self.settlement_days > 5 {
            errors.push(ValidationError::with_rule(
                "settlement_days",
                "Settlement days cannot exceed 5",
                "max_settlement_days",
            ));
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_curve_config_sofr_ois() {
        let config = CurveConfig::sofr_ois();
        assert_eq!(config.name, "USD.SOFR.OIS");
        assert_eq!(config.interpolation, InterpolationMethod::MonotoneConvex);
        assert!(!config.instruments.is_empty());
        assert!(config.is_valid());
    }

    #[test]
    fn test_curve_config_estr_ois() {
        let config = CurveConfig::estr_ois();
        assert_eq!(config.name, "EUR.ESTR.OIS");
        assert!(config.is_valid());
    }

    #[test]
    fn test_curve_config_validation() {
        let mut config = CurveConfig::new("test");
        assert!(config.is_valid());

        config.name = String::new();
        assert!(!config.is_valid());
    }

    #[test]
    fn test_interpolation_properties() {
        assert!(InterpolationMethod::MonotoneConvex.is_monotone_preserving());
        assert!(InterpolationMethod::MonotoneConvex.is_smooth_forwards());
        assert!(!InterpolationMethod::Linear.is_monotone_preserving());
    }

    #[test]
    fn test_instrument_config() {
        let deposit = InstrumentConfig::deposit("3M", 2);
        assert_eq!(deposit.instrument_type, InstrumentType::Deposit);
        assert_eq!(deposit.tenor, "3M");
        assert!(deposit.validate().is_empty());

        let ois = InstrumentConfig::ois("1Y", 2).with_weight(2.0);
        assert_eq!(ois.weight, 2.0);
    }
}
