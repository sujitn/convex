//! Global bootstrap using optimization.
//!
//! Fits a curve to market instruments by minimizing the sum of squared
//! pricing errors using numerical optimization.

use convex_core::Date;
use convex_math::optimization::{gradient_descent, OptimizationConfig};

use crate::curves::{DiscountCurve, DiscountCurveBuilder};
use crate::error::{CurveError, CurveResult};
use crate::instruments::CurveInstrument;
use crate::interpolation::InterpolationMethod;

/// Type of curve to fit in global bootstrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalCurveType {
    /// Piecewise zero rates at pillar points (most flexible).
    PiecewiseZero,
    /// Piecewise discount factors at pillar points.
    PiecewiseDiscount,
}

impl Default for GlobalCurveType {
    fn default() -> Self {
        Self::PiecewiseZero
    }
}

/// Configuration for global bootstrap.
#[derive(Debug, Clone, Copy)]
pub struct GlobalBootstrapConfig {
    /// Type of curve to fit.
    pub curve_type: GlobalCurveType,
    /// Interpolation method for the curve.
    pub interpolation: InterpolationMethod,
    /// Whether to enable extrapolation.
    pub allow_extrapolation: bool,
    /// Optimization tolerance.
    pub tolerance: f64,
    /// Maximum optimization iterations.
    pub max_iterations: u32,
    /// Roughness penalty for smoothness (0 = no penalty).
    pub roughness_penalty: f64,
}

impl Default for GlobalBootstrapConfig {
    fn default() -> Self {
        Self {
            curve_type: GlobalCurveType::PiecewiseZero,
            interpolation: InterpolationMethod::LogLinear,
            allow_extrapolation: true,
            tolerance: 1e-10,
            max_iterations: 200,
            roughness_penalty: 0.0,
        }
    }
}

/// Global bootstrapper using optimization.
///
/// Instead of solving for each discount factor sequentially, this
/// bootstrapper minimizes the sum of squared pricing errors across
/// all instruments simultaneously.
///
/// # Objective Function
///
/// ```text
/// min Σ wi × (PVi(curve))² + λ × R(curve)
/// ```
///
/// where:
/// - PVi is the present value of instrument i
/// - wi is the weight for instrument i (default 1.0)
/// - λ is the roughness penalty
/// - R(curve) is a roughness measure (e.g., integral of squared second derivative)
///
/// # Use Cases
///
/// - Fitting Nelson-Siegel/Svensson curves
/// - Handling noisy or sparse data
/// - Imposing smoothness constraints
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::bootstrap::GlobalBootstrapper;
///
/// let curve = GlobalBootstrapper::new(reference_date)
///     .add_instrument(deposit_3m)
///     .add_instrument(deposit_6m)
///     .add_instrument(swap_2y)
///     .with_roughness_penalty(0.001)
///     .bootstrap()?;
/// ```
pub struct GlobalBootstrapper {
    /// Reference date for the curve.
    reference_date: Date,
    /// Instruments to fit.
    instruments: Vec<Box<dyn CurveInstrument>>,
    /// Weights for each instrument (default 1.0).
    weights: Vec<f64>,
    /// Bootstrap configuration.
    config: GlobalBootstrapConfig,
}

impl GlobalBootstrapper {
    /// Creates a new global bootstrapper.
    ///
    /// # Arguments
    ///
    /// * `reference_date` - The curve's reference/valuation date
    pub fn new(reference_date: Date) -> Self {
        Self {
            reference_date,
            instruments: Vec::new(),
            weights: Vec::new(),
            config: GlobalBootstrapConfig::default(),
        }
    }

    /// Sets the bootstrap configuration.
    #[must_use]
    pub fn with_config(mut self, config: GlobalBootstrapConfig) -> Self {
        self.config = config;
        self
    }

    /// Sets the curve type to fit.
    #[must_use]
    pub fn with_curve_type(mut self, curve_type: GlobalCurveType) -> Self {
        self.config.curve_type = curve_type;
        self
    }

    /// Sets the interpolation method.
    #[must_use]
    pub fn with_interpolation(mut self, method: InterpolationMethod) -> Self {
        self.config.interpolation = method;
        self
    }

    /// Sets the roughness penalty for smoothness.
    #[must_use]
    pub fn with_roughness_penalty(mut self, penalty: f64) -> Self {
        self.config.roughness_penalty = penalty;
        self
    }

    /// Adds an instrument with default weight (1.0).
    #[must_use]
    pub fn add_instrument<I: CurveInstrument + 'static>(mut self, instrument: I) -> Self {
        self.instruments.push(Box::new(instrument));
        self.weights.push(1.0);
        self
    }

    /// Adds an instrument with a specific weight.
    #[must_use]
    pub fn add_weighted_instrument<I: CurveInstrument + 'static>(
        mut self,
        instrument: I,
        weight: f64,
    ) -> Self {
        self.instruments.push(Box::new(instrument));
        self.weights.push(weight);
        self
    }

    /// Bootstraps the curve using global optimization.
    ///
    /// # Returns
    ///
    /// A `DiscountCurve` that minimizes the weighted sum of squared
    /// pricing errors.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No instruments are provided
    /// - Optimization fails to converge
    /// - Curve construction fails
    pub fn bootstrap(self) -> CurveResult<DiscountCurve> {
        if self.instruments.is_empty() {
            return Err(CurveError::invalid_data("No instruments provided for bootstrap"));
        }

        // Sort instruments by pillar date
        let mut indexed: Vec<_> = self
            .instruments
            .iter()
            .zip(self.weights.iter())
            .enumerate()
            .collect();
        indexed.sort_by_key(|(_, (inst, _))| inst.pillar_date());

        let sorted_instruments: Vec<_> = indexed.iter().map(|(_, (inst, _))| *inst).collect();
        let sorted_weights: Vec<_> = indexed.iter().map(|(_, (_, w))| **w).collect();

        // Get pillar times
        let pillar_times: Vec<f64> = sorted_instruments
            .iter()
            .map(|inst| self.year_fraction(inst.pillar_date()))
            .collect();

        // Initial guess: flat curve at 4%
        let initial_params: Vec<f64> = match self.config.curve_type {
            GlobalCurveType::PiecewiseZero => vec![0.04; sorted_instruments.len()],
            GlobalCurveType::PiecewiseDiscount => {
                pillar_times.iter().map(|&t| (-0.04 * t).exp()).collect()
            }
        };

        // Build objective function
        let objective = |params: &[f64]| -> f64 {
            let curve = match self.build_curve_from_params(params, &pillar_times) {
                Ok(c) => c,
                Err(_) => return 1e20, // Penalty for invalid curve
            };

            let mut total = 0.0;

            // Sum of squared pricing errors
            for (inst, &weight) in sorted_instruments.iter().zip(sorted_weights.iter()) {
                let pv = inst.pv(&curve).unwrap_or(1e10);
                total += weight * pv * pv;
            }

            // Roughness penalty
            if self.config.roughness_penalty > 0.0 {
                total += self.config.roughness_penalty * self.roughness(params);
            }

            total
        };

        // Run optimization
        let opt_config = OptimizationConfig {
            tolerance: self.config.tolerance,
            max_iterations: self.config.max_iterations,
            step_size: 1e-6,
        };

        let result = gradient_descent(objective, &initial_params, &opt_config)
            .map_err(|e| CurveError::bootstrap_failed("global", e.to_string()))?;

        if !result.converged && result.objective_value > 1e-6 {
            return Err(CurveError::bootstrap_failed(
                "global",
                format!(
                    "Optimization did not converge: objective = {:.2e} after {} iterations",
                    result.objective_value, result.iterations
                ),
            ));
        }

        // Build final curve
        self.build_curve_from_params(&result.parameters, &pillar_times)
    }

    /// Builds a curve from optimization parameters.
    fn build_curve_from_params(
        &self,
        params: &[f64],
        pillar_times: &[f64],
    ) -> CurveResult<DiscountCurve> {
        let mut builder = DiscountCurveBuilder::new(self.reference_date)
            .with_interpolation(self.config.interpolation)
            .add_pillar(0.0, 1.0); // DF(0) = 1

        match self.config.curve_type {
            GlobalCurveType::PiecewiseZero => {
                for (&t, &rate) in pillar_times.iter().zip(params.iter()) {
                    if t > 0.0 {
                        let df = (-rate * t).exp();
                        builder = builder.add_pillar(t, df);
                    }
                }
            }
            GlobalCurveType::PiecewiseDiscount => {
                for (&t, &df) in pillar_times.iter().zip(params.iter()) {
                    if t > 0.0 && df > 0.0 && df <= 1.0 {
                        builder = builder.add_pillar(t, df);
                    }
                }
            }
        }

        if self.config.allow_extrapolation {
            builder = builder.with_extrapolation();
        }

        builder.build()
    }

    /// Calculates roughness penalty (sum of squared rate differences).
    fn roughness(&self, params: &[f64]) -> f64 {
        if params.len() < 2 {
            return 0.0;
        }

        let mut roughness = 0.0;
        for i in 1..params.len() {
            let diff = params[i] - params[i - 1];
            roughness += diff * diff;
        }
        roughness
    }

    /// Calculates year fraction from reference date.
    fn year_fraction(&self, date: Date) -> f64 {
        self.reference_date.days_between(&date) as f64 / 365.0
    }

    /// Returns the optimization result details.
    pub fn bootstrap_with_diagnostics(
        mut self,
    ) -> CurveResult<(DiscountCurve, GlobalBootstrapDiagnostics)> {
        if self.instruments.is_empty() {
            return Err(CurveError::invalid_data("No instruments provided"));
        }

        // Sort and prepare
        self.instruments.sort_by_key(|inst| inst.pillar_date());

        let pillar_times: Vec<f64> = self
            .instruments
            .iter()
            .map(|inst| self.year_fraction(inst.pillar_date()))
            .collect();

        let initial_params: Vec<f64> = match self.config.curve_type {
            GlobalCurveType::PiecewiseZero => vec![0.04; self.instruments.len()],
            GlobalCurveType::PiecewiseDiscount => {
                pillar_times.iter().map(|&t| (-0.04 * t).exp()).collect()
            }
        };

        let instruments = &self.instruments;
        let weights = &self.weights;
        let config = &self.config;
        let ref_date = self.reference_date;

        let objective = |params: &[f64]| -> f64 {
            let curve = match build_curve_helper(ref_date, params, &pillar_times, config) {
                Ok(c) => c,
                Err(_) => return 1e20,
            };

            let mut total = 0.0;
            for (inst, &weight) in instruments.iter().zip(weights.iter()) {
                let pv = inst.pv(&curve).unwrap_or(1e10);
                total += weight * pv * pv;
            }

            if config.roughness_penalty > 0.0 {
                total += config.roughness_penalty * roughness_helper(params);
            }

            total
        };

        let opt_config = OptimizationConfig {
            tolerance: self.config.tolerance,
            max_iterations: self.config.max_iterations,
            step_size: 1e-6,
        };

        let result = gradient_descent(objective, &initial_params, &opt_config)
            .map_err(|e| CurveError::bootstrap_failed("global", e.to_string()))?;

        let curve = self.build_curve_from_params(&result.parameters, &pillar_times)?;

        // Calculate per-instrument errors
        let mut instrument_errors = Vec::new();
        for inst in &self.instruments {
            let pv = inst.pv(&curve).unwrap_or(f64::NAN);
            instrument_errors.push(pv);
        }

        let diagnostics = GlobalBootstrapDiagnostics {
            converged: result.converged,
            iterations: result.iterations,
            final_objective: result.objective_value,
            instrument_errors,
            parameters: result.parameters,
        };

        Ok((curve, diagnostics))
    }
}

/// Diagnostics from global bootstrap.
#[derive(Debug, Clone)]
pub struct GlobalBootstrapDiagnostics {
    /// Whether the optimization converged.
    pub converged: bool,
    /// Number of iterations used.
    pub iterations: u32,
    /// Final objective function value.
    pub final_objective: f64,
    /// PV error for each instrument.
    pub instrument_errors: Vec<f64>,
    /// Optimal parameters found.
    pub parameters: Vec<f64>,
}

// Helper functions for closures
fn build_curve_helper(
    ref_date: Date,
    params: &[f64],
    pillar_times: &[f64],
    config: &GlobalBootstrapConfig,
) -> CurveResult<DiscountCurve> {
    let mut builder = DiscountCurveBuilder::new(ref_date)
        .with_interpolation(config.interpolation)
        .add_pillar(0.0, 1.0);

    match config.curve_type {
        GlobalCurveType::PiecewiseZero => {
            for (&t, &rate) in pillar_times.iter().zip(params.iter()) {
                if t > 0.0 {
                    let df = (-rate * t).exp();
                    builder = builder.add_pillar(t, df);
                }
            }
        }
        GlobalCurveType::PiecewiseDiscount => {
            for (&t, &df) in pillar_times.iter().zip(params.iter()) {
                if t > 0.0 && df > 0.0 && df <= 1.0 {
                    builder = builder.add_pillar(t, df);
                }
            }
        }
    }

    if config.allow_extrapolation {
        builder = builder.with_extrapolation();
    }

    builder.build()
}

fn roughness_helper(params: &[f64]) -> f64 {
    if params.len() < 2 {
        return 0.0;
    }
    let mut r = 0.0;
    for i in 1..params.len() {
        let diff = params[i] - params[i - 1];
        r += diff * diff;
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruments::Deposit;
    use crate::traits::Curve;
    use approx::assert_relative_eq;

    #[test]
    fn test_global_bootstrap_single_deposit() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let end_date = Date::from_ymd(2025, 7, 15).unwrap();

        let deposit = Deposit::new(ref_date, end_date, 0.05);

        let curve = GlobalBootstrapper::new(ref_date)
            .add_instrument(deposit)
            .bootstrap()
            .unwrap();

        assert_eq!(curve.reference_date(), ref_date);

        // Check DF at t=0
        assert_relative_eq!(curve.discount_factor(0.0).unwrap(), 1.0, epsilon = 1e-6);
    }

    #[test]
    fn test_global_bootstrap_multiple_deposits() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let deposits = vec![
            Deposit::new(ref_date, Date::from_ymd(2025, 4, 15).unwrap(), 0.050),
            Deposit::new(ref_date, Date::from_ymd(2025, 7, 15).unwrap(), 0.052),
            Deposit::new(ref_date, Date::from_ymd(2025, 10, 15).unwrap(), 0.054),
        ];

        let mut bootstrapper = GlobalBootstrapper::new(ref_date);
        for d in deposits {
            bootstrapper = bootstrapper.add_instrument(d);
        }

        let curve = bootstrapper.bootstrap().unwrap();

        // DFs should be monotonically decreasing
        let df_3m = curve.discount_factor(0.25).unwrap();
        let df_6m = curve.discount_factor(0.5).unwrap();
        let df_9m = curve.discount_factor(0.75).unwrap();

        assert!(df_3m > df_6m);
        assert!(df_6m > df_9m);
    }

    #[test]
    fn test_global_bootstrap_with_diagnostics() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let deposit = Deposit::new(
            ref_date,
            Date::from_ymd(2025, 7, 15).unwrap(),
            0.05,
        );

        let (curve, diagnostics) = GlobalBootstrapper::new(ref_date)
            .add_instrument(deposit)
            .bootstrap_with_diagnostics()
            .unwrap();

        assert!(diagnostics.converged || diagnostics.final_objective < 1e-6);
        assert_eq!(diagnostics.instrument_errors.len(), 1);

        // The instrument should reprice close to zero
        let abs_error = diagnostics.instrument_errors[0].abs();
        assert!(abs_error < 0.01, "Error too large: {}", abs_error);

        // Curve should be valid
        assert!(curve.discount_factor(0.5).is_ok());
    }

    #[test]
    fn test_global_bootstrap_empty_fails() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let result = GlobalBootstrapper::new(ref_date).bootstrap();

        assert!(result.is_err());
    }

    #[test]
    fn test_global_bootstrap_with_roughness_penalty() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let deposits = vec![
            Deposit::new(ref_date, Date::from_ymd(2025, 4, 15).unwrap(), 0.050),
            Deposit::new(ref_date, Date::from_ymd(2025, 7, 15).unwrap(), 0.052),
            Deposit::new(ref_date, Date::from_ymd(2025, 10, 15).unwrap(), 0.048),
        ];

        let mut bootstrapper = GlobalBootstrapper::new(ref_date)
            .with_roughness_penalty(0.001);

        for d in deposits {
            bootstrapper = bootstrapper.add_instrument(d);
        }

        let curve = bootstrapper.bootstrap().unwrap();

        // Should still produce a valid curve
        assert!(curve.discount_factor(0.5).is_ok());
    }
}
