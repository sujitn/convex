//! Scenario analysis with multiple curve bumps.
//!
//! Scenarios combine multiple bump types (parallel, key-rate, twist, etc.)
//! to model stress tests and market scenarios.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_curves::bumping::{Scenario, ScenarioBump};
//!
//! let scenario = Scenario::new("Rates +100bp, Credit +50bp")
//!     .with_bump(ScenarioBump::parallel(100.0))
//!     .with_bump(ScenarioBump::credit_spread(50.0));
//!
//! let bumped = scenario.apply(&curve);
//! ```

use std::sync::Arc;

use convex_core::types::Date;

use crate::term_structure::TermStructure;
use crate::value_type::ValueType;

/// A single bump component within a scenario.
#[derive(Clone)]
pub enum ScenarioBump {
    /// Parallel (uniform) shift across all tenors.
    Parallel {
        /// Shift in basis points.
        shift_bps: f64,
    },

    /// Steepener: short rates down, long rates up.
    Steepener {
        /// Downward shift at short end (in bps, positive = rates down).
        short_shift_bps: f64,
        /// Upward shift at long end (in bps, positive = rates up).
        long_shift_bps: f64,
        /// Pivot tenor where shift is zero (years).
        pivot_tenor: f64,
    },

    /// Flattener: short rates up, long rates down.
    Flattener {
        /// Upward shift at short end (in bps).
        short_shift_bps: f64,
        /// Downward shift at long end (in bps).
        long_shift_bps: f64,
        /// Pivot tenor where shift is zero (years).
        pivot_tenor: f64,
    },

    /// Key-rate bump at a specific tenor.
    KeyRate {
        /// Key tenor (years).
        tenor: f64,
        /// Shift in basis points.
        shift_bps: f64,
        /// Left neighbor tenor (optional).
        left_tenor: Option<f64>,
        /// Right neighbor tenor (optional).
        right_tenor: Option<f64>,
    },

    /// Credit spread widening/tightening.
    CreditSpread {
        /// Shift in basis points.
        shift_bps: f64,
    },

    /// Custom bump function.
    Custom {
        /// Name of the custom bump.
        name: String,
        /// Function that computes shift at each tenor.
        shift_fn: Arc<dyn Fn(f64) -> f64 + Send + Sync>,
    },
}

impl std::fmt::Debug for ScenarioBump {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScenarioBump::Parallel { shift_bps } => f
                .debug_struct("Parallel")
                .field("shift_bps", shift_bps)
                .finish(),
            ScenarioBump::Steepener {
                short_shift_bps,
                long_shift_bps,
                pivot_tenor,
            } => f
                .debug_struct("Steepener")
                .field("short_shift_bps", short_shift_bps)
                .field("long_shift_bps", long_shift_bps)
                .field("pivot_tenor", pivot_tenor)
                .finish(),
            ScenarioBump::Flattener {
                short_shift_bps,
                long_shift_bps,
                pivot_tenor,
            } => f
                .debug_struct("Flattener")
                .field("short_shift_bps", short_shift_bps)
                .field("long_shift_bps", long_shift_bps)
                .field("pivot_tenor", pivot_tenor)
                .finish(),
            ScenarioBump::KeyRate {
                tenor,
                shift_bps,
                left_tenor,
                right_tenor,
            } => f
                .debug_struct("KeyRate")
                .field("tenor", tenor)
                .field("shift_bps", shift_bps)
                .field("left_tenor", left_tenor)
                .field("right_tenor", right_tenor)
                .finish(),
            ScenarioBump::CreditSpread { shift_bps } => f
                .debug_struct("CreditSpread")
                .field("shift_bps", shift_bps)
                .finish(),
            ScenarioBump::Custom { name, .. } => f
                .debug_struct("Custom")
                .field("name", name)
                .finish_non_exhaustive(),
        }
    }
}

impl ScenarioBump {
    /// Creates a parallel shift bump.
    #[must_use]
    pub fn parallel(shift_bps: f64) -> Self {
        ScenarioBump::Parallel { shift_bps }
    }

    /// Creates a steepener bump.
    ///
    /// Short rates decrease, long rates increase.
    ///
    /// # Arguments
    ///
    /// * `short_shift_bps` - Downward shift at short end (positive = rates down)
    /// * `long_shift_bps` - Upward shift at long end (positive = rates up)
    /// * `pivot_tenor` - Tenor where shift is zero
    #[must_use]
    pub fn steepener(short_shift_bps: f64, long_shift_bps: f64, pivot_tenor: f64) -> Self {
        ScenarioBump::Steepener {
            short_shift_bps,
            long_shift_bps,
            pivot_tenor,
        }
    }

    /// Creates a flattener bump.
    ///
    /// Short rates increase, long rates decrease.
    #[must_use]
    pub fn flattener(short_shift_bps: f64, long_shift_bps: f64, pivot_tenor: f64) -> Self {
        ScenarioBump::Flattener {
            short_shift_bps,
            long_shift_bps,
            pivot_tenor,
        }
    }

    /// Creates a key-rate bump.
    #[must_use]
    pub fn key_rate(tenor: f64, shift_bps: f64) -> Self {
        use super::key_rate::STANDARD_KEY_TENORS;

        // Find neighbors
        let mut left = None;
        let mut right = None;
        for &kt in STANDARD_KEY_TENORS {
            if kt < tenor {
                left = Some(kt);
            } else if kt > tenor && right.is_none() {
                right = Some(kt);
                break;
            }
        }

        ScenarioBump::KeyRate {
            tenor,
            shift_bps,
            left_tenor: left,
            right_tenor: right,
        }
    }

    /// Creates a credit spread bump.
    #[must_use]
    pub fn credit_spread(shift_bps: f64) -> Self {
        ScenarioBump::CreditSpread { shift_bps }
    }

    /// Creates a custom bump with a user-defined function.
    #[must_use]
    pub fn custom<F>(name: impl Into<String>, shift_fn: F) -> Self
    where
        F: Fn(f64) -> f64 + Send + Sync + 'static,
    {
        ScenarioBump::Custom {
            name: name.into(),
            shift_fn: Arc::new(shift_fn),
        }
    }

    /// Computes the shift at a given tenor.
    pub fn shift_at(&self, t: f64) -> f64 {
        match self {
            ScenarioBump::Parallel { shift_bps } => shift_bps / 10_000.0,

            ScenarioBump::Steepener {
                short_shift_bps,
                long_shift_bps,
                pivot_tenor,
            } => {
                // Linear interpolation from -short_shift at t=0 to +long_shift at t=30
                // Zero at pivot
                let shift = if t < *pivot_tenor {
                    // Below pivot: negative shift (rates down)
                    -short_shift_bps * (1.0 - t / pivot_tenor)
                } else {
                    // Above pivot: positive shift (rates up)
                    // Scale so that shift reaches long_shift_bps at 30Y
                    let remaining = 30.0 - pivot_tenor;
                    if remaining > 0.0 {
                        long_shift_bps * ((t - pivot_tenor) / remaining).min(1.0)
                    } else {
                        *long_shift_bps
                    }
                };
                shift / 10_000.0
            }

            ScenarioBump::Flattener {
                short_shift_bps,
                long_shift_bps,
                pivot_tenor,
            } => {
                // Opposite of steepener
                let shift = if t < *pivot_tenor {
                    // Below pivot: positive shift (rates up)
                    short_shift_bps * (1.0 - t / pivot_tenor)
                } else {
                    // Above pivot: negative shift (rates down)
                    let remaining = 30.0 - pivot_tenor;
                    if remaining > 0.0 {
                        -long_shift_bps * ((t - pivot_tenor) / remaining).min(1.0)
                    } else {
                        -long_shift_bps
                    }
                };
                shift / 10_000.0
            }

            ScenarioBump::KeyRate {
                tenor,
                shift_bps,
                left_tenor,
                right_tenor,
            } => {
                let weight = key_rate_weight(t, *tenor, *left_tenor, *right_tenor);
                weight * shift_bps / 10_000.0
            }

            ScenarioBump::CreditSpread { shift_bps } => shift_bps / 10_000.0,

            ScenarioBump::Custom { shift_fn, .. } => shift_fn(t),
        }
    }

    /// Returns a description of this bump.
    #[must_use]
    pub fn description(&self) -> String {
        match self {
            ScenarioBump::Parallel { shift_bps } => {
                format!("Parallel {}{:.0}bp", if *shift_bps >= 0.0 { "+" } else { "" }, shift_bps)
            }
            ScenarioBump::Steepener {
                short_shift_bps,
                long_shift_bps,
                pivot_tenor,
            } => {
                format!(
                    "Steepener -{:.0}bp/{:.0}Y/+{:.0}bp",
                    short_shift_bps, pivot_tenor, long_shift_bps
                )
            }
            ScenarioBump::Flattener {
                short_shift_bps,
                long_shift_bps,
                pivot_tenor,
            } => {
                format!(
                    "Flattener +{:.0}bp/{:.0}Y/-{:.0}bp",
                    short_shift_bps, pivot_tenor, long_shift_bps
                )
            }
            ScenarioBump::KeyRate { tenor, shift_bps, .. } => {
                format!(
                    "KR{:.0}Y {}{:.0}bp",
                    tenor,
                    if *shift_bps >= 0.0 { "+" } else { "" },
                    shift_bps
                )
            }
            ScenarioBump::CreditSpread { shift_bps } => {
                format!(
                    "Credit {}{:.0}bp",
                    if *shift_bps >= 0.0 { "+" } else { "" },
                    shift_bps
                )
            }
            ScenarioBump::Custom { name, .. } => name.clone(),
        }
    }
}

/// Computes triangular key-rate weight.
fn key_rate_weight(t: f64, key_tenor: f64, left: Option<f64>, right: Option<f64>) -> f64 {
    if (t - key_tenor).abs() < 1e-10 {
        return 1.0;
    }

    if t < key_tenor {
        match left {
            Some(l) if t >= l => (t - l) / (key_tenor - l),
            Some(l) if t < l => 0.0,
            None => 1.0,
            _ => 0.0,
        }
    } else {
        match right {
            Some(r) if t <= r => (r - t) / (r - key_tenor),
            Some(r) if t > r => 0.0,
            None => 1.0,
            _ => 0.0,
        }
    }
}

/// A scenario consisting of multiple bumps.
///
/// Bumps are applied additively.
#[derive(Debug, Clone)]
pub struct Scenario {
    /// Name of the scenario.
    name: String,
    /// Bumps to apply.
    bumps: Vec<ScenarioBump>,
}

impl Scenario {
    /// Creates a new scenario with the given name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            bumps: Vec::new(),
        }
    }

    /// Adds a bump to the scenario.
    #[must_use]
    pub fn with_bump(mut self, bump: ScenarioBump) -> Self {
        self.bumps.push(bump);
        self
    }

    /// Adds multiple bumps to the scenario.
    #[must_use]
    pub fn with_bumps(mut self, bumps: impl IntoIterator<Item = ScenarioBump>) -> Self {
        self.bumps.extend(bumps);
        self
    }

    /// Returns the scenario name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the bumps in this scenario.
    #[must_use]
    pub fn bumps(&self) -> &[ScenarioBump] {
        &self.bumps
    }

    /// Computes the total shift at a given tenor.
    #[must_use]
    pub fn total_shift_at(&self, t: f64) -> f64 {
        self.bumps.iter().map(|b| b.shift_at(t)).sum()
    }

    /// Applies the scenario to a curve.
    #[must_use]
    pub fn apply<'a, T: TermStructure>(&'a self, curve: &'a T) -> ScenarioCurve<'a, T> {
        ScenarioCurve {
            base: curve,
            scenario: self,
        }
    }

    /// Applies the scenario to an Arc-wrapped curve.
    #[must_use]
    pub fn apply_arc<T: TermStructure>(self, curve: Arc<T>) -> ArcScenarioCurve<T> {
        ArcScenarioCurve {
            base: curve,
            scenario: Arc::new(self),
        }
    }

    /// Returns a description of all bumps in this scenario.
    #[must_use]
    pub fn description(&self) -> String {
        if self.bumps.is_empty() {
            format!("{}: (no bumps)", self.name)
        } else {
            let bump_descs: Vec<_> = self.bumps.iter().map(|b| b.description()).collect();
            format!("{}: {}", self.name, bump_descs.join(", "))
        }
    }
}

/// Pre-built common scenarios.
pub mod presets {
    use super::*;

    /// Parallel up 100bp scenario.
    #[must_use]
    pub fn parallel_up_100bp() -> Scenario {
        Scenario::new("Parallel +100bp").with_bump(ScenarioBump::parallel(100.0))
    }

    /// Parallel down 100bp scenario.
    #[must_use]
    pub fn parallel_down_100bp() -> Scenario {
        Scenario::new("Parallel -100bp").with_bump(ScenarioBump::parallel(-100.0))
    }

    /// Parallel up 50bp scenario.
    #[must_use]
    pub fn parallel_up_50bp() -> Scenario {
        Scenario::new("Parallel +50bp").with_bump(ScenarioBump::parallel(50.0))
    }

    /// Parallel down 50bp scenario.
    #[must_use]
    pub fn parallel_down_50bp() -> Scenario {
        Scenario::new("Parallel -50bp").with_bump(ScenarioBump::parallel(-50.0))
    }

    /// Steepener: short rates -25bp, long rates +25bp, pivot at 5Y.
    #[must_use]
    pub fn steepener_50bp() -> Scenario {
        Scenario::new("Steepener 50bp").with_bump(ScenarioBump::steepener(25.0, 25.0, 5.0))
    }

    /// Flattener: short rates +25bp, long rates -25bp, pivot at 5Y.
    #[must_use]
    pub fn flattener_50bp() -> Scenario {
        Scenario::new("Flattener 50bp").with_bump(ScenarioBump::flattener(25.0, 25.0, 5.0))
    }

    /// Credit widening 50bp.
    #[must_use]
    pub fn credit_widening_50bp() -> Scenario {
        Scenario::new("Credit +50bp").with_bump(ScenarioBump::credit_spread(50.0))
    }

    /// Credit tightening 50bp.
    #[must_use]
    pub fn credit_tightening_50bp() -> Scenario {
        Scenario::new("Credit -50bp").with_bump(ScenarioBump::credit_spread(-50.0))
    }

    /// Combined rates up + credit widening (flight to quality reverse).
    #[must_use]
    pub fn rates_up_credit_wide() -> Scenario {
        Scenario::new("Rates +100bp, Credit +50bp")
            .with_bump(ScenarioBump::parallel(100.0))
            .with_bump(ScenarioBump::credit_spread(50.0))
    }

    /// Combined rates down + credit tightening (risk-on).
    #[must_use]
    pub fn rates_down_credit_tight() -> Scenario {
        Scenario::new("Rates -100bp, Credit -50bp")
            .with_bump(ScenarioBump::parallel(-100.0))
            .with_bump(ScenarioBump::credit_spread(-50.0))
    }

    /// Standard regulatory scenarios for stress testing.
    #[must_use]
    pub fn regulatory_scenarios() -> Vec<Scenario> {
        vec![
            parallel_up_100bp(),
            parallel_down_100bp(),
            steepener_50bp(),
            flattener_50bp(),
            credit_widening_50bp(),
        ]
    }
}

/// A curve with a scenario applied.
#[derive(Debug)]
pub struct ScenarioCurve<'a, T: TermStructure> {
    /// The base curve.
    base: &'a T,
    /// The scenario being applied.
    scenario: &'a Scenario,
}

impl<'a, T: TermStructure> ScenarioCurve<'a, T> {
    /// Returns a reference to the base curve.
    #[must_use]
    pub fn base(&self) -> &T {
        self.base
    }

    /// Returns the scenario being applied.
    #[must_use]
    pub fn scenario(&self) -> &Scenario {
        self.scenario
    }
}

impl<T: TermStructure> TermStructure for ScenarioCurve<'_, T> {
    fn reference_date(&self) -> Date {
        self.base.reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        let base_value = self.base.value_at(t);
        let total_shift = self.scenario.total_shift_at(t);

        match self.base.value_type() {
            ValueType::ZeroRate { .. }
            | ValueType::ForwardRate { .. }
            | ValueType::InstantaneousForward
            | ValueType::HazardRate
            | ValueType::ParSwapRate { .. }
            | ValueType::CreditSpread { .. } => base_value + total_shift,

            ValueType::DiscountFactor => base_value * (-total_shift * t).exp(),
            ValueType::SurvivalProbability => base_value * (-total_shift * t).exp(),

            ValueType::InflationIndexRatio | ValueType::FxForwardPoints => {
                base_value + total_shift
            }
        }
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        self.base.tenor_bounds()
    }

    fn value_type(&self) -> ValueType {
        self.base.value_type()
    }

    fn derivative_at(&self, _t: f64) -> Option<f64> {
        // Complex scenario shifts affect derivative in non-trivial ways
        None
    }

    fn max_date(&self) -> Date {
        self.base.max_date()
    }
}

/// Arc-owned scenario curve.
#[derive(Debug, Clone)]
pub struct ArcScenarioCurve<T: TermStructure> {
    /// The base curve (Arc-owned).
    base: Arc<T>,
    /// The scenario (Arc-owned).
    scenario: Arc<Scenario>,
}

impl<T: TermStructure> ArcScenarioCurve<T> {
    /// Returns a reference to the base curve.
    #[must_use]
    pub fn base(&self) -> &T {
        &self.base
    }

    /// Returns the scenario being applied.
    #[must_use]
    pub fn scenario(&self) -> &Scenario {
        &self.scenario
    }
}

impl<T: TermStructure> TermStructure for ArcScenarioCurve<T> {
    fn reference_date(&self) -> Date {
        self.base.reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        let base_value = self.base.value_at(t);
        let total_shift = self.scenario.total_shift_at(t);

        match self.base.value_type() {
            ValueType::ZeroRate { .. }
            | ValueType::ForwardRate { .. }
            | ValueType::InstantaneousForward
            | ValueType::HazardRate
            | ValueType::ParSwapRate { .. }
            | ValueType::CreditSpread { .. } => base_value + total_shift,

            ValueType::DiscountFactor => base_value * (-total_shift * t).exp(),
            ValueType::SurvivalProbability => base_value * (-total_shift * t).exp(),

            ValueType::InflationIndexRatio | ValueType::FxForwardPoints => {
                base_value + total_shift
            }
        }
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        self.base.tenor_bounds()
    }

    fn value_type(&self) -> ValueType {
        self.base.value_type()
    }

    fn derivative_at(&self, _t: f64) -> Option<f64> {
        None
    }

    fn max_date(&self) -> Date {
        self.base.max_date()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscreteCurve;
    use crate::InterpolationMethod;
    use approx::assert_relative_eq;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::Compounding;

    fn sample_zero_curve() -> DiscreteCurve {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let tenors = vec![1.0, 2.0, 5.0, 10.0, 20.0, 30.0];
        let rates = vec![0.04, 0.042, 0.046, 0.05, 0.052, 0.054];

        DiscreteCurve::new(
            today,
            tenors,
            rates,
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            },
            InterpolationMethod::Linear,
        )
        .unwrap()
    }

    #[test]
    fn test_parallel_bump() {
        let bump = ScenarioBump::parallel(100.0);
        assert_relative_eq!(bump.shift_at(5.0), 0.01, epsilon = 1e-10);
        assert_relative_eq!(bump.shift_at(10.0), 0.01, epsilon = 1e-10);
    }

    #[test]
    fn test_steepener() {
        let bump = ScenarioBump::steepener(50.0, 50.0, 10.0);

        // At t=0: -50bp
        assert_relative_eq!(bump.shift_at(0.0), -0.005, epsilon = 1e-10);

        // At pivot (10Y): 0
        assert_relative_eq!(bump.shift_at(10.0), 0.0, epsilon = 1e-10);

        // At 30Y: +50bp
        assert_relative_eq!(bump.shift_at(30.0), 0.005, epsilon = 1e-10);
    }

    #[test]
    fn test_flattener() {
        let bump = ScenarioBump::flattener(50.0, 50.0, 10.0);

        // At t=0: +50bp
        assert_relative_eq!(bump.shift_at(0.0), 0.005, epsilon = 1e-10);

        // At pivot (10Y): 0
        assert_relative_eq!(bump.shift_at(10.0), 0.0, epsilon = 1e-10);

        // At 30Y: -50bp
        assert_relative_eq!(bump.shift_at(30.0), -0.005, epsilon = 1e-10);
    }

    #[test]
    fn test_key_rate_bump() {
        let bump = ScenarioBump::key_rate(5.0, 100.0);

        // At key tenor
        assert_relative_eq!(bump.shift_at(5.0), 0.01, epsilon = 1e-10);

        // At neighbors
        assert_relative_eq!(bump.shift_at(3.0), 0.0, epsilon = 1e-10);
        assert_relative_eq!(bump.shift_at(7.0), 0.0, epsilon = 1e-10);
    }

    #[test]
    fn test_scenario_creation() {
        let scenario = Scenario::new("Test Scenario")
            .with_bump(ScenarioBump::parallel(50.0))
            .with_bump(ScenarioBump::credit_spread(25.0));

        assert_eq!(scenario.name(), "Test Scenario");
        assert_eq!(scenario.bumps().len(), 2);
    }

    #[test]
    fn test_scenario_total_shift() {
        let scenario = Scenario::new("Combined")
            .with_bump(ScenarioBump::parallel(50.0))
            .with_bump(ScenarioBump::parallel(25.0));

        // Total should be 75bp at all tenors
        assert_relative_eq!(scenario.total_shift_at(5.0), 0.0075, epsilon = 1e-10);
        assert_relative_eq!(scenario.total_shift_at(10.0), 0.0075, epsilon = 1e-10);
    }

    #[test]
    fn test_scenario_apply() {
        let curve = sample_zero_curve();
        let scenario = Scenario::new("Parallel +100bp").with_bump(ScenarioBump::parallel(100.0));

        let bumped = scenario.apply(&curve);

        let base_rate = curve.value_at(5.0);
        let bumped_rate = bumped.value_at(5.0);

        assert_relative_eq!(bumped_rate - base_rate, 0.01, epsilon = 1e-10);
    }

    #[test]
    fn test_preset_parallel_up() {
        let scenario = presets::parallel_up_100bp();
        assert_eq!(scenario.name(), "Parallel +100bp");
        assert_relative_eq!(scenario.total_shift_at(5.0), 0.01, epsilon = 1e-10);
    }

    #[test]
    fn test_preset_steepener() {
        let scenario = presets::steepener_50bp();

        // Short end should be down
        assert!(scenario.total_shift_at(1.0) < 0.0);

        // At pivot should be near zero
        assert!(scenario.total_shift_at(5.0).abs() < 0.0001);

        // Long end should be up
        assert!(scenario.total_shift_at(30.0) > 0.0);
    }

    #[test]
    fn test_preset_flattener() {
        let scenario = presets::flattener_50bp();

        // Short end should be up
        assert!(scenario.total_shift_at(1.0) > 0.0);

        // At pivot should be near zero
        assert!(scenario.total_shift_at(5.0).abs() < 0.0001);

        // Long end should be down
        assert!(scenario.total_shift_at(30.0) < 0.0);
    }

    #[test]
    fn test_regulatory_scenarios() {
        let scenarios = presets::regulatory_scenarios();
        assert_eq!(scenarios.len(), 5);
    }

    #[test]
    fn test_scenario_description() {
        let scenario = Scenario::new("Test")
            .with_bump(ScenarioBump::parallel(100.0))
            .with_bump(ScenarioBump::credit_spread(50.0));

        let desc = scenario.description();
        assert!(desc.contains("Test"));
        assert!(desc.contains("Parallel"));
        assert!(desc.contains("Credit"));
    }

    #[test]
    fn test_custom_bump() {
        let bump = ScenarioBump::custom("Hump", |t: f64| {
            // Hump centered at 5Y
            let peak: f64 = 0.01; // 100bp
            let width: f64 = 3.0;
            peak * (-(t - 5.0).powi(2) / (2.0 * width.powi(2))).exp()
        });

        // Peak at 5Y
        let shift_5y = bump.shift_at(5.0);
        assert_relative_eq!(shift_5y, 0.01, epsilon = 1e-10);

        // Lower at other tenors
        let shift_1y = bump.shift_at(1.0);
        let shift_10y = bump.shift_at(10.0);
        assert!(shift_1y < shift_5y);
        assert!(shift_10y < shift_5y);
    }

    #[test]
    fn test_preserves_curve_properties() {
        let curve = sample_zero_curve();
        let scenario = presets::parallel_up_100bp();
        let bumped = scenario.apply(&curve);

        assert_eq!(curve.reference_date(), bumped.reference_date());
        assert_eq!(curve.tenor_bounds(), bumped.tenor_bounds());
        assert_eq!(curve.value_type(), bumped.value_type());
    }

    #[test]
    fn test_arc_scenario_curve() {
        let curve = Arc::new(sample_zero_curve());
        let scenario = presets::parallel_up_50bp();
        let bumped = scenario.apply_arc(curve.clone());

        let base_rate = curve.value_at(5.0);
        let bumped_rate = bumped.value_at(5.0);

        assert_relative_eq!(bumped_rate - base_rate, 0.005, epsilon = 1e-10);
    }
}
