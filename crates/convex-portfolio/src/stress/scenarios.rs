//! Stress scenario definitions.
//!
//! Provides types for defining various stress scenarios including:
//! - Parallel rate shifts
//! - Key rate shifts (twist, steepening, flattening)
//! - Credit spread shocks
//! - Combined scenarios

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A rate shift at a specific tenor (in basis points).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TenorShift {
    /// Tenor in years.
    pub tenor: f64,
    /// Shift in basis points (positive = rates up).
    pub shift_bps: f64,
}

impl TenorShift {
    /// Creates a new tenor shift.
    #[must_use]
    pub fn new(tenor: f64, shift_bps: f64) -> Self {
        Self { tenor, shift_bps }
    }

    /// Shift as a decimal (e.g., 100 bps = 0.01).
    #[must_use]
    pub fn shift_decimal(&self) -> f64 {
        self.shift_bps / 10000.0
    }
}

/// Type of rate scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RateScenario {
    /// Parallel shift across all tenors (bps).
    ParallelShift(f64),

    /// Specific shifts at each tenor.
    KeyRateShifts(Vec<TenorShift>),

    /// Steepening: short rates down, long rates up.
    Steepening {
        /// Shift for short end (bps, typically negative).
        short_shift: f64,
        /// Shift for long end (bps, typically positive).
        long_shift: f64,
        /// Pivot point in years (typically 5 or 7).
        pivot_tenor: f64,
    },

    /// Flattening: short rates up, long rates down.
    Flattening {
        /// Shift for short end (bps, typically positive).
        short_shift: f64,
        /// Shift for long end (bps, typically negative).
        long_shift: f64,
        /// Pivot point in years.
        pivot_tenor: f64,
    },

    /// Butterfly: short and long up, intermediate down (or vice versa).
    Butterfly {
        /// Shift for wings (short and long end, bps).
        wing_shift: f64,
        /// Shift for belly (intermediate, bps).
        belly_shift: f64,
        /// Short end of belly in years.
        belly_start: f64,
        /// Long end of belly in years.
        belly_end: f64,
    },
}

impl RateScenario {
    /// Creates a parallel shift scenario.
    #[must_use]
    pub fn parallel(shift_bps: f64) -> Self {
        Self::ParallelShift(shift_bps)
    }

    /// Creates a key rate shift scenario from tenor/shift pairs.
    #[must_use]
    pub fn key_rates(shifts: &[(f64, f64)]) -> Self {
        Self::KeyRateShifts(
            shifts
                .iter()
                .map(|(t, s)| TenorShift::new(*t, *s))
                .collect(),
        )
    }

    /// Creates a steepening scenario (2s10s).
    #[must_use]
    pub fn steepening_2s10s(short_shift: f64, long_shift: f64) -> Self {
        Self::Steepening {
            short_shift,
            long_shift,
            pivot_tenor: 5.0,
        }
    }

    /// Creates a flattening scenario (2s10s).
    #[must_use]
    pub fn flattening_2s10s(short_shift: f64, long_shift: f64) -> Self {
        Self::Flattening {
            short_shift,
            long_shift,
            pivot_tenor: 5.0,
        }
    }

    /// Creates a butterfly scenario.
    #[must_use]
    pub fn butterfly(wing_shift: f64, belly_shift: f64) -> Self {
        Self::Butterfly {
            wing_shift,
            belly_shift,
            belly_start: 3.0,
            belly_end: 7.0,
        }
    }

    /// Gets the shift at a specific tenor.
    #[must_use]
    pub fn shift_at_tenor(&self, tenor: f64) -> f64 {
        match self {
            Self::ParallelShift(shift) => *shift,

            Self::KeyRateShifts(shifts) => {
                // Find exact match or interpolate
                if let Some(ts) = shifts.iter().find(|ts| (ts.tenor - tenor).abs() < 0.001) {
                    return ts.shift_bps;
                }
                // Linear interpolation between surrounding tenors
                let below = shifts.iter().filter(|ts| ts.tenor < tenor).max_by(|a, b| {
                    a.tenor
                        .partial_cmp(&b.tenor)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                let above = shifts.iter().filter(|ts| ts.tenor > tenor).min_by(|a, b| {
                    a.tenor
                        .partial_cmp(&b.tenor)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                match (below, above) {
                    (Some(b), Some(a)) => {
                        let weight = (tenor - b.tenor) / (a.tenor - b.tenor);
                        b.shift_bps + weight * (a.shift_bps - b.shift_bps)
                    }
                    (Some(b), None) => b.shift_bps,
                    (None, Some(a)) => a.shift_bps,
                    (None, None) => 0.0,
                }
            }

            Self::Steepening {
                short_shift,
                long_shift,
                pivot_tenor,
            } => {
                if tenor <= *pivot_tenor {
                    // Linear interpolation from short_shift at 0 to 0 at pivot
                    short_shift * (1.0 - tenor / pivot_tenor)
                } else {
                    // Linear interpolation from 0 at pivot to long_shift at 30Y
                    long_shift * (tenor - pivot_tenor) / (30.0 - pivot_tenor)
                }
            }

            Self::Flattening {
                short_shift,
                long_shift,
                pivot_tenor,
            } => {
                if tenor <= *pivot_tenor {
                    short_shift * (1.0 - tenor / pivot_tenor)
                } else {
                    long_shift * (tenor - pivot_tenor) / (30.0 - pivot_tenor)
                }
            }

            Self::Butterfly {
                wing_shift,
                belly_shift,
                belly_start,
                belly_end,
            } => {
                if tenor < *belly_start {
                    *wing_shift
                } else if tenor <= *belly_end {
                    *belly_shift
                } else {
                    *wing_shift
                }
            }
        }
    }

    /// Returns the scenario name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::ParallelShift(_) => "Parallel Shift",
            Self::KeyRateShifts(_) => "Key Rate Shift",
            Self::Steepening { .. } => "Steepening",
            Self::Flattening { .. } => "Flattening",
            Self::Butterfly { .. } => "Butterfly",
        }
    }
}

/// Credit spread scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpreadScenario {
    /// Uniform spread widening/tightening (bps).
    Uniform(f64),

    /// Spread shift by rating category.
    ByRating(HashMap<String, f64>),

    /// Spread shift by sector.
    BySector(HashMap<String, f64>),
}

impl SpreadScenario {
    /// Creates a uniform spread shock.
    #[must_use]
    pub fn uniform(shift_bps: f64) -> Self {
        Self::Uniform(shift_bps)
    }

    /// Creates a rating-based spread shock.
    #[must_use]
    pub fn by_rating(shifts: &[(&str, f64)]) -> Self {
        Self::ByRating(shifts.iter().map(|(r, s)| (r.to_string(), *s)).collect())
    }

    /// Creates a sector-based spread shock.
    #[must_use]
    pub fn by_sector(shifts: &[(&str, f64)]) -> Self {
        Self::BySector(shifts.iter().map(|(s, v)| (s.to_string(), *v)).collect())
    }

    /// Returns the scenario name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Uniform(_) => "Uniform Spread Shock",
            Self::ByRating(_) => "Rating-Based Spread Shock",
            Self::BySector(_) => "Sector-Based Spread Shock",
        }
    }
}

/// A complete stress scenario combining rate and spread shocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressScenario {
    /// Scenario name.
    pub name: String,

    /// Description.
    pub description: Option<String>,

    /// Rate scenario (optional).
    pub rate_scenario: Option<RateScenario>,

    /// Spread scenario (optional).
    pub spread_scenario: Option<SpreadScenario>,
}

impl StressScenario {
    /// Creates a new stress scenario.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            rate_scenario: None,
            spread_scenario: None,
        }
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Sets the rate scenario.
    #[must_use]
    pub fn with_rate_scenario(mut self, scenario: RateScenario) -> Self {
        self.rate_scenario = Some(scenario);
        self
    }

    /// Sets the spread scenario.
    #[must_use]
    pub fn with_spread_scenario(mut self, scenario: SpreadScenario) -> Self {
        self.spread_scenario = Some(scenario);
        self
    }

    /// Returns true if this scenario has a rate component.
    #[must_use]
    pub fn has_rate_scenario(&self) -> bool {
        self.rate_scenario.is_some()
    }

    /// Returns true if this scenario has a spread component.
    #[must_use]
    pub fn has_spread_scenario(&self) -> bool {
        self.spread_scenario.is_some()
    }
}

/// Standard stress scenarios commonly used in fixed income.
pub mod standard {
    use super::*;

    /// +100bp parallel shift.
    #[must_use]
    pub fn rates_up_100() -> StressScenario {
        StressScenario::new("Rates +100bp")
            .with_description("Parallel shift up 100 basis points")
            .with_rate_scenario(RateScenario::parallel(100.0))
    }

    /// -100bp parallel shift.
    #[must_use]
    pub fn rates_down_100() -> StressScenario {
        StressScenario::new("Rates -100bp")
            .with_description("Parallel shift down 100 basis points")
            .with_rate_scenario(RateScenario::parallel(-100.0))
    }

    /// +50bp parallel shift.
    #[must_use]
    pub fn rates_up_50() -> StressScenario {
        StressScenario::new("Rates +50bp")
            .with_description("Parallel shift up 50 basis points")
            .with_rate_scenario(RateScenario::parallel(50.0))
    }

    /// -50bp parallel shift.
    #[must_use]
    pub fn rates_down_50() -> StressScenario {
        StressScenario::new("Rates -50bp")
            .with_description("Parallel shift down 50 basis points")
            .with_rate_scenario(RateScenario::parallel(-50.0))
    }

    /// 2s10s steepening: 2Y -25bp, 10Y +25bp.
    #[must_use]
    pub fn steepening_50() -> StressScenario {
        StressScenario::new("Steepening +50bp")
            .with_description("2s10s steepens 50bp: 2Y -25bp, 10Y +25bp")
            .with_rate_scenario(RateScenario::steepening_2s10s(-25.0, 25.0))
    }

    /// 2s10s flattening: 2Y +25bp, 10Y -25bp.
    #[must_use]
    pub fn flattening_50() -> StressScenario {
        StressScenario::new("Flattening 50bp")
            .with_description("2s10s flattens 50bp: 2Y +25bp, 10Y -25bp")
            .with_rate_scenario(RateScenario::flattening_2s10s(25.0, -25.0))
    }

    /// Spread widening +50bp.
    #[must_use]
    pub fn spreads_widen_50() -> StressScenario {
        StressScenario::new("Spreads +50bp")
            .with_description("Uniform spread widening 50 basis points")
            .with_spread_scenario(SpreadScenario::uniform(50.0))
    }

    /// Spread tightening -25bp.
    #[must_use]
    pub fn spreads_tighten_25() -> StressScenario {
        StressScenario::new("Spreads -25bp")
            .with_description("Uniform spread tightening 25 basis points")
            .with_spread_scenario(SpreadScenario::uniform(-25.0))
    }

    /// Risk-off: rates down, spreads widen.
    #[must_use]
    pub fn risk_off() -> StressScenario {
        StressScenario::new("Risk Off")
            .with_description("Flight to quality: rates -50bp, spreads +100bp")
            .with_rate_scenario(RateScenario::parallel(-50.0))
            .with_spread_scenario(SpreadScenario::uniform(100.0))
    }

    /// Risk-on: rates up, spreads tighten.
    #[must_use]
    pub fn risk_on() -> StressScenario {
        StressScenario::new("Risk On")
            .with_description("Risk appetite: rates +50bp, spreads -25bp")
            .with_rate_scenario(RateScenario::parallel(50.0))
            .with_spread_scenario(SpreadScenario::uniform(-25.0))
    }

    /// Returns all standard scenarios.
    #[must_use]
    pub fn all() -> Vec<StressScenario> {
        vec![
            rates_up_100(),
            rates_down_100(),
            rates_up_50(),
            rates_down_50(),
            steepening_50(),
            flattening_50(),
            spreads_widen_50(),
            spreads_tighten_25(),
            risk_off(),
            risk_on(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_shift() {
        let scenario = RateScenario::parallel(100.0);

        assert_eq!(scenario.shift_at_tenor(2.0), 100.0);
        assert_eq!(scenario.shift_at_tenor(10.0), 100.0);
        assert_eq!(scenario.shift_at_tenor(30.0), 100.0);
    }

    #[test]
    fn test_key_rate_shifts() {
        let scenario = RateScenario::key_rates(&[(2.0, 50.0), (10.0, 100.0)]);

        assert!((scenario.shift_at_tenor(2.0) - 50.0).abs() < 0.01);
        assert!((scenario.shift_at_tenor(10.0) - 100.0).abs() < 0.01);

        // Interpolation at 6Y: 50 + (6-2)/(10-2) * (100-50) = 50 + 25 = 75
        assert!((scenario.shift_at_tenor(6.0) - 75.0).abs() < 0.01);
    }

    #[test]
    fn test_steepening() {
        let scenario = RateScenario::steepening_2s10s(-50.0, 50.0);

        // At short end (2Y): mostly short_shift
        let shift_2y = scenario.shift_at_tenor(2.0);
        assert!(shift_2y < 0.0); // Negative (rates down)

        // At pivot (5Y): should be ~0
        let shift_5y = scenario.shift_at_tenor(5.0);
        assert!(shift_5y.abs() < 1.0);

        // At long end (10Y): positive
        let shift_10y = scenario.shift_at_tenor(10.0);
        assert!(shift_10y > 0.0);
    }

    #[test]
    fn test_butterfly() {
        let scenario = RateScenario::butterfly(25.0, -25.0);

        // Wings (short and long)
        assert_eq!(scenario.shift_at_tenor(2.0), 25.0);
        assert_eq!(scenario.shift_at_tenor(20.0), 25.0);

        // Belly
        assert_eq!(scenario.shift_at_tenor(5.0), -25.0);
    }

    #[test]
    fn test_spread_scenario_uniform() {
        let scenario = SpreadScenario::uniform(50.0);
        assert_eq!(scenario.name(), "Uniform Spread Shock");
    }

    #[test]
    fn test_stress_scenario_builder() {
        let scenario = StressScenario::new("Test Scenario")
            .with_description("A test scenario")
            .with_rate_scenario(RateScenario::parallel(100.0))
            .with_spread_scenario(SpreadScenario::uniform(50.0));

        assert_eq!(scenario.name, "Test Scenario");
        assert!(scenario.has_rate_scenario());
        assert!(scenario.has_spread_scenario());
    }

    #[test]
    fn test_standard_scenarios() {
        let scenarios = standard::all();
        assert_eq!(scenarios.len(), 10);

        // Check rates up
        let rates_up = &scenarios[0];
        assert_eq!(rates_up.name, "Rates +100bp");
        assert!(rates_up.has_rate_scenario());
        assert!(!rates_up.has_spread_scenario());

        // Check risk off (has both)
        let risk_off = scenarios.iter().find(|s| s.name == "Risk Off").unwrap();
        assert!(risk_off.has_rate_scenario());
        assert!(risk_off.has_spread_scenario());
    }

    #[test]
    fn test_tenor_shift() {
        let ts = TenorShift::new(5.0, 100.0);
        assert_eq!(ts.tenor, 5.0);
        assert_eq!(ts.shift_bps, 100.0);
        assert!((ts.shift_decimal() - 0.01).abs() < 0.0001);
    }
}
