#!/usr/bin/env python3
"""
Generate Rust boilerplate for Convex yield conventions.

Usage:
    python scripts/generate_boilerplate.py --output /path/to/convex/crates/
"""

import argparse
from pathlib import Path

YIELD_METHOD_RS = '''//! Yield calculation method types.

/// Yield calculation methodology.
/// 
/// This only controls HOW to calculate yield - the bond's own conventions
/// (day count, frequency) are used for the actual calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum YieldMethod {
    /// Compound yield with reinvestment assumption.
    /// Formula: Price = Σ CF/(1+y/f)^(f×t)
    /// Uses Newton-Raphson iteration.
    Compounded,
    
    /// Simple yield (linear, no compounding).
    /// Formula: y = (Coupon + (Redemption-Price)/Years) / Price
    /// Used by Japanese government bonds.
    Simple,
    
    /// Discount yield (quoted on face value).
    /// Formula: y = (Face-Price)/Face × (Basis/Days)
    /// Used by T-Bills, Commercial Paper.
    Discount,
    
    /// Add-on yield (quoted on purchase price).
    /// Formula: y = (Face-Price)/Price × (Basis/Days)
    /// Used by CDs, money market instruments.
    AddOn,
}

impl YieldMethod {
    /// Returns true if this method uses iterative solving.
    #[inline]
    pub fn requires_solver(&self) -> bool {
        matches!(self, Self::Compounded)
    }
    
    /// Returns true if this is a money market style calculation.
    #[inline]
    pub fn is_money_market(&self) -> bool {
        matches!(self, Self::Discount | Self::AddOn)
    }
}

impl Default for YieldMethod {
    fn default() -> Self {
        Self::Compounded
    }
}

impl std::fmt::Display for YieldMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Compounded => write!(f, "Compounded"),
            Self::Simple => write!(f, "Simple"),
            Self::Discount => write!(f, "Discount"),
            Self::AddOn => write!(f, "Add-On"),
        }
    }
}
'''

YIELD_CALCULATOR_CONFIG_RS = '''//! Yield calculator configuration.

use super::YieldMethod;

/// Configuration for yield calculations.
/// 
/// Note: Day count and frequency come from the bond itself.
/// This config only controls the calculation method and solver parameters.
#[derive(Debug, Clone)]
pub struct YieldCalculatorConfig {
    /// Primary calculation method.
    pub method: YieldMethod,
    
    /// Threshold (days) for switching to money market methodology.
    /// Typically 182 for US markets, 365 for Canadian.
    pub money_market_threshold: Option<u32>,
    
    /// Newton-Raphson tolerance (default: 1e-10).
    pub tolerance: f64,
    
    /// Maximum iterations for solver (default: 100).
    pub max_iterations: u32,
}

impl Default for YieldCalculatorConfig {
    fn default() -> Self {
        Self {
            method: YieldMethod::Compounded,
            money_market_threshold: None,
            tolerance: 1e-10,
            max_iterations: 100,
        }
    }
}

impl YieldCalculatorConfig {
    /// Create config for compounded yield with money market threshold.
    pub fn with_mm_threshold(threshold: u32) -> Self {
        Self {
            method: YieldMethod::Compounded,
            money_market_threshold: Some(threshold),
            ..Default::default()
        }
    }
    
    /// Check if money market method should be used.
    #[inline]
    pub fn should_use_money_market(&self, days_to_maturity: u32) -> bool {
        self.money_market_threshold
            .map(|t| days_to_maturity <= t)
            .unwrap_or(false)
    }
    
    /// Get effective method for given maturity.
    pub fn effective_method(&self, days_to_maturity: u32) -> YieldMethod {
        if self.should_use_money_market(days_to_maturity) {
            YieldMethod::AddOn
        } else {
            self.method
        }
    }
}

/// Standard money market threshold (182 days = half year).
pub const MM_THRESHOLD_US: u32 = 182;

/// Canadian money market threshold (365 days).
pub const MM_THRESHOLD_CAD: u32 = 365;
'''

MARKET_PRESET_RS = '''//! Market convention presets.

use crate::daycounts::DayCountConvention;
use crate::types::Frequency;
use super::{YieldMethod, YieldCalculatorConfig, MM_THRESHOLD_US, MM_THRESHOLD_CAD};

/// Market convention preset for bond creation and validation.
/// 
/// Presets serve two purposes:
/// 1. Provide defaults when creating bonds for a market
/// 2. Validate that a bond matches expected market conventions
#[derive(Debug, Clone)]
pub struct MarketPreset {
    /// Human-readable name.
    pub name: &'static str,
    /// Expected day count convention.
    pub day_count: DayCountConvention,
    /// Expected coupon frequency.
    pub frequency: Frequency,
    /// Settlement days (T+n).
    pub settlement_days: u32,
    /// Yield calculation method.
    pub yield_method: YieldMethod,
    /// Money market threshold (days).
    pub money_market_threshold: Option<u32>,
    /// Ex-dividend period (business days), if any.
    pub ex_dividend_days: Option<u32>,
}

impl MarketPreset {
    /// Get yield calculator config for this market.
    pub fn yield_config(&self) -> YieldCalculatorConfig {
        YieldCalculatorConfig {
            method: self.yield_method,
            money_market_threshold: self.money_market_threshold,
            ..Default::default()
        }
    }
}

// =============================================================================
// United States
// =============================================================================

pub const US_TREASURY: MarketPreset = MarketPreset {
    name: "US Treasury",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::SemiAnnual,
    settlement_days: 1,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: Some(MM_THRESHOLD_US),
    ex_dividend_days: None,
};

pub const US_CORPORATE: MarketPreset = MarketPreset {
    name: "US Corporate",
    day_count: DayCountConvention::Thirty360Us,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: Some(MM_THRESHOLD_US),
    ex_dividend_days: None,
};

pub const US_TBILL: MarketPreset = MarketPreset {
    name: "US T-Bill",
    day_count: DayCountConvention::Act360,
    frequency: Frequency::None,
    settlement_days: 1,
    yield_method: YieldMethod::Discount,
    money_market_threshold: None,
    ex_dividend_days: None,
};

// =============================================================================
// United Kingdom
// =============================================================================

pub const UK_GILT: MarketPreset = MarketPreset {
    name: "UK Gilt",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::SemiAnnual,
    settlement_days: 1,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: Some(7),
};

// =============================================================================
// Eurozone
// =============================================================================

pub const GERMAN_BUND: MarketPreset = MarketPreset {
    name: "German Bund",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::Annual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};

pub const FRENCH_OAT: MarketPreset = MarketPreset {
    name: "French OAT",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::Annual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};

pub const ITALIAN_BTP: MarketPreset = MarketPreset {
    name: "Italian BTP",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};

// =============================================================================
// Asia-Pacific
// =============================================================================

pub const JAPANESE_JGB: MarketPreset = MarketPreset {
    name: "Japanese JGB",
    day_count: DayCountConvention::Act365Fixed,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};

pub const JAPANESE_JGB_SIMPLE: MarketPreset = MarketPreset {
    name: "Japanese JGB (Simple)",
    day_count: DayCountConvention::Act365Fixed,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Simple,
    money_market_threshold: None,
    ex_dividend_days: None,
};

pub const AUSTRALIAN_GOVT: MarketPreset = MarketPreset {
    name: "Australian Government",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};

// =============================================================================
// Canada
// =============================================================================

pub const CANADIAN_GOVT: MarketPreset = MarketPreset {
    name: "Canadian Government",
    day_count: DayCountConvention::Act365Fixed,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: Some(MM_THRESHOLD_CAD),
    ex_dividend_days: None,
};

/// All available presets.
pub const ALL_PRESETS: &[&MarketPreset] = &[
    &US_TREASURY,
    &US_CORPORATE,
    &US_TBILL,
    &UK_GILT,
    &GERMAN_BUND,
    &FRENCH_OAT,
    &ITALIAN_BTP,
    &JAPANESE_JGB,
    &JAPANESE_JGB_SIMPLE,
    &AUSTRALIAN_GOVT,
    &CANADIAN_GOVT,
];
'''

COMPOUNDING_RS = '''//! Compounding frequency and rate conversion.

use rust_decimal::Decimal;
use rust_decimal::prelude::*;
use thiserror::Error;

/// Compounding frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CompoundingFrequency {
    /// No compounding (simple interest).
    None,
    /// Once per year.
    Annual,
    /// Twice per year.
    SemiAnnual,
    /// Four times per year.
    Quarterly,
    /// Twelve times per year.
    Monthly,
    /// Continuous compounding.
    Continuous,
}

impl CompoundingFrequency {
    /// Periods per year (0 for None and Continuous).
    #[inline]
    pub fn periods_per_year(&self) -> u32 {
        match self {
            Self::None | Self::Continuous => 0,
            Self::Annual => 1,
            Self::SemiAnnual => 2,
            Self::Quarterly => 4,
            Self::Monthly => 12,
        }
    }
    
    /// Is this discrete compounding?
    #[inline]
    pub fn is_discrete(&self) -> bool {
        !matches!(self, Self::None | Self::Continuous)
    }
}

impl Default for CompoundingFrequency {
    fn default() -> Self {
        Self::SemiAnnual
    }
}

/// Convert rate between compounding frequencies.
/// 
/// Formula: (1 + r₁/n₁)^n₁ = (1 + r₂/n₂)^n₂
pub fn convert_rate(
    rate: Decimal,
    from: CompoundingFrequency,
    to: CompoundingFrequency,
) -> Result<Decimal, CompoundingError> {
    use CompoundingFrequency::*;
    
    if from == to {
        return Ok(rate);
    }
    
    let rate_f64 = rate.to_f64()
        .ok_or(CompoundingError::InvalidRate { rate })?;
    
    match (from, to) {
        (Continuous, to_freq) if to_freq.is_discrete() => {
            let n = to_freq.periods_per_year() as f64;
            let result = n * ((rate_f64 / n).exp() - 1.0);
            Ok(Decimal::from_f64(result).unwrap())
        }
        
        (from_freq, Continuous) if from_freq.is_discrete() => {
            let n = from_freq.periods_per_year() as f64;
            let result = n * (1.0 + rate_f64 / n).ln();
            Ok(Decimal::from_f64(result).unwrap())
        }
        
        (from_freq, to_freq) if from_freq.is_discrete() && to_freq.is_discrete() => {
            let n1 = from_freq.periods_per_year() as f64;
            let n2 = to_freq.periods_per_year() as f64;
            let base = 1.0 + rate_f64 / n1;
            let result = n2 * (base.powf(n1 / n2) - 1.0);
            Ok(Decimal::from_f64(result).unwrap())
        }
        
        _ => Err(CompoundingError::InvalidConversion { from, to }),
    }
}

/// Calculate effective annual rate.
pub fn effective_annual_rate(
    rate: Decimal,
    frequency: CompoundingFrequency,
) -> Result<Decimal, CompoundingError> {
    use CompoundingFrequency::*;
    
    let rate_f64 = rate.to_f64()
        .ok_or(CompoundingError::InvalidRate { rate })?;
    
    let result = match frequency {
        None | Annual => rate_f64,
        Continuous => rate_f64.exp() - 1.0,
        freq => {
            let n = freq.periods_per_year() as f64;
            (1.0 + rate_f64 / n).powf(n) - 1.0
        }
    };
    
    Ok(Decimal::from_f64(result).unwrap())
}

#[derive(Debug, Error)]
pub enum CompoundingError {
    #[error("cannot convert from {from:?} to {to:?}")]
    InvalidConversion {
        from: CompoundingFrequency,
        to: CompoundingFrequency,
    },
    #[error("invalid rate: {rate}")]
    InvalidRate { rate: Decimal },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use approx::assert_relative_eq;
    
    #[test]
    fn test_semi_to_annual() {
        let semi = dec!(0.05);
        let annual = convert_rate(semi, CompoundingFrequency::SemiAnnual, CompoundingFrequency::Annual).unwrap();
        assert_relative_eq!(annual.to_f64().unwrap(), 0.050625, epsilon = 1e-10);
    }
    
    #[test]
    fn test_roundtrip() {
        let original = dec!(0.06);
        let annual = convert_rate(original, CompoundingFrequency::SemiAnnual, CompoundingFrequency::Annual).unwrap();
        let back = convert_rate(annual, CompoundingFrequency::Annual, CompoundingFrequency::SemiAnnual).unwrap();
        assert_relative_eq!(original.to_f64().unwrap(), back.to_f64().unwrap(), epsilon = 1e-10);
    }
}
'''


def generate_files(output_dir: Path):
    """Generate all Rust files."""
    
    # convex-core types
    types_dir = output_dir / "convex-core" / "src" / "yields"
    types_dir.mkdir(parents=True, exist_ok=True)
    
    (types_dir / "yield_method.rs").write_text(YIELD_METHOD_RS.strip())
    (types_dir / "config.rs").write_text(YIELD_CALCULATOR_CONFIG_RS.strip())
    (types_dir / "mod.rs").write_text("""//! Yield calculation types.

mod yield_method;
mod config;

pub use yield_method::YieldMethod;
pub use config::{YieldCalculatorConfig, MM_THRESHOLD_US, MM_THRESHOLD_CAD};
""")
    
    print(f"Generated: {types_dir}/yield_method.rs")
    print(f"Generated: {types_dir}/config.rs")
    print(f"Generated: {types_dir}/mod.rs")
    
    # convex-core compounding
    comp_dir = output_dir / "convex-core" / "src" / "compounding"
    comp_dir.mkdir(parents=True, exist_ok=True)
    
    (comp_dir / "mod.rs").write_text(COMPOUNDING_RS.strip())
    print(f"Generated: {comp_dir}/mod.rs")
    
    # convex-yas presets
    yas_dir = output_dir / "convex-yas" / "src" / "presets"
    yas_dir.mkdir(parents=True, exist_ok=True)
    
    (yas_dir / "mod.rs").write_text(MARKET_PRESET_RS.strip())
    print(f"Generated: {yas_dir}/mod.rs")
    
    print("\nDone! Add modules to parent mod.rs files:")
    print("  - convex-core/src/lib.rs: pub mod yields; pub mod compounding;")
    print("  - convex-yas/src/lib.rs: pub mod presets;")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Generate yield convention boilerplate")
    parser.add_argument("--output", "-o", type=Path, required=True, help="Crates directory")
    args = parser.parse_args()
    
    generate_files(args.output)
