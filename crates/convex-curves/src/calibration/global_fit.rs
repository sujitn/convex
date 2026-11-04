//! Global curve fitting using optimization.
//!
//! This module provides curve calibration through global optimization,
//! fitting all instruments simultaneously rather than sequentially.
//!
//! # Approach
//!
//! The [`GlobalFitter`] uses a Levenberg-Marquardt style algorithm to minimize
//! the sum of squared pricing errors across all instruments. This provides:
//!
//! - Better fit quality than sequential bootstrap
//! - Stability with over-determined systems
//! - Handling of instrument interdependencies
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_curves::calibration::{GlobalFitter, Deposit, Swap, InstrumentSet};
//!
//! let mut instruments = InstrumentSet::new();
//! instruments.add(Deposit::from_tenor(today, 0.25, 0.04, Act360));
//! instruments.add(Swap::from_tenor(today, 2.0, 0.045, SemiAnnual, Thirty360US));
//!
//! let fitter = GlobalFitter::default();
//! let result = fitter.fit(today, &instruments)?;
//!
//! let curve = result.curve;
//! println!("RMS error: {:.2e}", result.rms_error);
//! ```

#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, Date};

use crate::curves::DiscreteCurve;
use crate::error::{CurveError, CurveResult};
use crate::wrappers::RateCurve;
use crate::{InterpolationMethod, ValueType};

use super::instruments::{CalibrationInstrument, InstrumentSet};

/// Hard cap for one dense calibration problem; this is not a capacity certificate.
pub const MAX_CALIBRATION_INSTRUMENTS: usize = 128;
const MAX_REFINEMENT_PASSES: usize = 32;

fn finite_calibration(value: f64) -> CurveResult<f64> {
    if !value.is_finite() {
        return Err(CurveError::invalid_value(
            "nonfinite calibration arithmetic",
        ));
    }
    Ok(value)
}
fn checked_residual(result: CurveResult<f64>) -> CurveResult<f64> {
    finite_calibration(result?)
}
fn validate_instruments(date: Date, instruments: &InstrumentSet) -> CurveResult<()> {
    if instruments.is_empty() || instruments.len() > MAX_CALIBRATION_INSTRUMENTS {
        return Err(CurveError::invalid_value(
            "calibration instrument count outside 1..=128",
        ));
    }
    for instrument in instruments.instruments() {
        if instrument.reference_date() != date {
            return Err(CurveError::reference_date_mismatch(
                date,
                instrument.reference_date(),
            ));
        }
        if !instrument.tenor().is_finite()
            || instrument.tenor() <= 0.0
            || !instrument.quote().is_finite()
        {
            return Err(CurveError::invalid_value(
                "calibration tenor must be finite positive and quote finite",
            ));
        }
    }
    Ok(())
}

/// Configuration for the global fitter.
#[derive(Debug, Clone, Copy)]
pub struct FitterConfig {
    /// Maximum number of iterations.
    pub max_iterations: usize,
    /// Tolerance for convergence (RMS error threshold).
    pub tolerance: f64,
    /// Initial Levenberg-Marquardt damping parameter.
    pub initial_lambda: f64,
    /// Lambda adjustment factor.
    pub lambda_factor: f64,
    /// Minimum lambda value.
    pub min_lambda: f64,
    /// Maximum lambda value.
    pub max_lambda: f64,
    /// Finite difference step for Jacobian.
    pub jacobian_step: f64,
}

impl Default for FitterConfig {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            tolerance: 1e-10,
            initial_lambda: 0.001,
            lambda_factor: 10.0,
            min_lambda: 1e-10,
            max_lambda: 1e10,
            jacobian_step: 1e-6,
        }
    }
}

impl FitterConfig {
    /// Checks finite numerical settings and the bounded iteration budget.
    pub fn validate(&self) -> CurveResult<()> {
        if self.max_iterations == 0
            || self.max_iterations > convex_math::bounds::MAX_SOLVER_ITERATIONS as usize
        {
            return Err(CurveError::invalid_value(
                "global-fit iteration limit exceeded",
            ));
        }
        let parameters = [
            self.tolerance,
            self.initial_lambda,
            self.lambda_factor,
            self.min_lambda,
            self.max_lambda,
            self.jacobian_step,
        ];
        if parameters.iter().any(|v| !v.is_finite() || *v <= 0.0)
            || self.lambda_factor <= 1.0
            || self.min_lambda > self.initial_lambda
            || self.initial_lambda > self.max_lambda
        {
            return Err(CurveError::invalid_value(
                "invalid global-fit numerical settings",
            ));
        }
        Ok(())
    }

    /// Creates a new configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum iterations.
    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// Sets the tolerance.
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }
}

/// Result of curve calibration.
#[derive(Debug, Clone)]
pub struct CalibrationResult {
    /// The calibrated curve.
    pub curve: DiscreteCurve,
    /// Pricing errors for each instrument (in rate terms).
    pub residuals: Vec<f64>,
    /// Number of iterations used.
    pub iterations: usize,
    /// Final RMS error.
    pub rms_error: f64,
    /// Whether calibration converged.
    pub converged: bool,
}

impl CalibrationResult {
    /// Returns the maximum absolute error.
    #[must_use]
    pub fn max_error(&self) -> f64 {
        self.residuals.iter().map(|r| r.abs()).fold(0.0, f64::max)
    }

    /// Returns errors in basis points.
    #[must_use]
    pub fn errors_bps(&self) -> Vec<f64> {
        self.residuals.iter().map(|r| r * 10_000.0).collect()
    }

    /// Prints a summary of the calibration result.
    pub fn summary(&self) -> String {
        let max_error_bps = self.max_error() * 10_000.0;
        let rms_bps = self.rms_error * 10_000.0;

        format!(
            "Calibration {}: {} iterations, RMS={:.4}bp, Max={:.4}bp",
            if self.converged {
                "converged"
            } else {
                "FAILED"
            },
            self.iterations,
            rms_bps,
            max_error_bps
        )
    }
}

/// Global curve fitter using Levenberg-Marquardt optimization.
///
/// Calibrates a discount curve by minimizing the sum of squared pricing
/// errors across all instruments simultaneously.
#[derive(Debug, Clone)]
pub struct GlobalFitter {
    /// Configuration.
    config: FitterConfig,
    /// Value type for the output curve.
    value_type: ValueType,
    /// Interpolation method.
    interpolation: InterpolationMethod,
}

impl Default for GlobalFitter {
    fn default() -> Self {
        Self {
            config: FitterConfig::default(),
            value_type: ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            },
            interpolation: InterpolationMethod::Linear,
        }
    }
}

impl GlobalFitter {
    /// Creates a new global fitter with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a fitter with custom configuration.
    #[must_use]
    pub fn with_config(config: FitterConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// Sets the output value type.
    #[must_use]
    pub fn output_as(mut self, value_type: ValueType) -> Self {
        self.value_type = value_type;
        self
    }

    /// Sets the interpolation method.
    #[must_use]
    pub fn interpolation(mut self, method: InterpolationMethod) -> Self {
        self.interpolation = method;
        self
    }

    /// Fits a curve to the given instruments.
    ///
    /// # Arguments
    ///
    /// * `reference_date` - The curve's reference date
    /// * `instruments` - Set of calibration instruments
    ///
    /// # Returns
    ///
    /// A [`CalibrationResult`] containing the fitted curve and diagnostics.
    pub fn fit(
        &self,
        reference_date: Date,
        instruments: &InstrumentSet,
    ) -> CurveResult<CalibrationResult> {
        validate_instruments(reference_date, instruments)?;
        self.config.validate()?;

        // Get sorted tenors and initial guesses from quotes
        let mut sorted_instruments: Vec<_> = instruments.instruments().iter().collect();
        sorted_instruments.sort_by(|a, b| a.tenor().total_cmp(&b.tenor()));

        let tenors: Vec<f64> = sorted_instruments.iter().map(|i| i.tenor()).collect();
        let quotes: Vec<f64> = sorted_instruments.iter().map(|i| i.quote()).collect();

        // Add t=0 point
        let mut curve_tenors = vec![0.0];
        curve_tenors.extend(&tenors);

        // Initial guess: use instrument quotes as zero rates
        let mut values = vec![quotes[0]]; // Start with first quote
        values.extend(&quotes);

        // Run Levenberg-Marquardt
        let (final_values, iterations, converged) =
            self.levenberg_marquardt(reference_date, &curve_tenors, &values, &sorted_instruments)?;

        // Build final curve
        let curve = DiscreteCurve::new(
            reference_date,
            curve_tenors.clone(),
            final_values,
            self.value_type.clone(),
            self.interpolation,
        )?;

        // Compute final residuals
        let rate_curve = RateCurve::new(curve.clone());
        let residuals: Vec<f64> = sorted_instruments
            .iter()
            .map(|inst| checked_residual(inst.pricing_error(&rate_curve)))
            .collect::<CurveResult<_>>()?;

        let rms_error = finite_calibration(
            (residuals.iter().map(|r| r * r).sum::<f64>() / residuals.len() as f64).sqrt(),
        )?;
        if !converged || rms_error >= self.config.tolerance {
            return Err(CurveError::calibration_failed(
                iterations,
                rms_error,
                "global fit did not reach requested tolerance",
            ));
        }

        Ok(CalibrationResult {
            curve,
            residuals,
            iterations,
            rms_error,
            converged,
        })
    }

    /// Runs the Levenberg-Marquardt algorithm.
    fn levenberg_marquardt(
        &self,
        reference_date: Date,
        tenors: &[f64],
        initial_values: &[f64],
        instruments: &[&Box<dyn CalibrationInstrument>],
    ) -> CurveResult<(Vec<f64>, usize, bool)> {
        let n = tenors.len();
        let m = instruments.len();

        let mut values = initial_values.to_vec();
        let mut lambda = self.config.initial_lambda;
        let mut prev_error = f64::MAX;

        for iteration in 0..self.config.max_iterations {
            // Build curve with current values
            let curve = DiscreteCurve::new(
                reference_date,
                tenors.to_vec(),
                values.clone(),
                self.value_type.clone(),
                self.interpolation,
            )?;
            let rate_curve = RateCurve::new(curve);

            // Compute residuals
            let residuals: Vec<f64> = instruments
                .iter()
                .map(|inst| checked_residual(inst.pricing_error(&rate_curve)))
                .collect::<CurveResult<_>>()?;

            let error = finite_calibration(residuals.iter().map(|r| r * r).sum::<f64>())?;

            // Check convergence
            let rms = (error / m as f64).sqrt();
            if rms < self.config.tolerance {
                return Ok((values, iteration + 1, true));
            }

            // Compute Jacobian (numerical)
            let jacobian = self.compute_jacobian(reference_date, tenors, &values, instruments)?;

            // Compute J^T * J and J^T * r
            let jtj = self.matrix_multiply_transpose(&jacobian, &jacobian, n)?;
            let jtr = self.vector_multiply_transpose(&jacobian, &residuals, n)?;

            // Solve (J^T J + λI) δ = -J^T r
            let delta = self.solve_damped_system(&jtj, &jtr, lambda, n)?;

            // Update values
            let mut new_values = values.clone();
            for i in 0..n {
                new_values[i] = finite_calibration(new_values[i] - delta[i])?;
                // Ensure values stay reasonable
                new_values[i] = new_values[i].clamp(-0.10, 0.30);
            }

            // Evaluate new error
            let new_curve = DiscreteCurve::new(
                reference_date,
                tenors.to_vec(),
                new_values.clone(),
                self.value_type.clone(),
                self.interpolation,
            )?;
            let new_rate_curve = RateCurve::new(new_curve);

            let new_residuals: Vec<f64> = instruments
                .iter()
                .map(|inst| checked_residual(inst.pricing_error(&new_rate_curve)))
                .collect::<CurveResult<_>>()?;
            let new_error = finite_calibration(new_residuals.iter().map(|r| r * r).sum())?;

            if new_error < error {
                // Accept step, reduce damping
                values = new_values;
                lambda = (lambda / self.config.lambda_factor).max(self.config.min_lambda);
                prev_error = new_error;
            } else {
                // Reject step, increase damping
                lambda = (lambda * self.config.lambda_factor).min(self.config.max_lambda);
            }

            // Check if stuck
            if (prev_error - error).abs() < 1e-16 && iteration > 10 {
                return Ok((values, iteration + 1, rms < self.config.tolerance));
            }
        }

        // Did not converge
        let final_curve = DiscreteCurve::new(
            reference_date,
            tenors.to_vec(),
            values.clone(),
            self.value_type.clone(),
            self.interpolation,
        )?;
        let rate_curve = RateCurve::new(final_curve);
        let residuals: Vec<f64> = instruments
            .iter()
            .map(|inst| checked_residual(inst.pricing_error(&rate_curve)))
            .collect::<CurveResult<_>>()?;
        let rms = (residuals.iter().map(|r| r * r).sum::<f64>() / m as f64).sqrt();

        // May have gotten close enough
        Ok((
            values,
            self.config.max_iterations,
            rms < self.config.tolerance,
        ))
    }

    /// Computes the Jacobian matrix numerically.
    fn compute_jacobian(
        &self,
        reference_date: Date,
        tenors: &[f64],
        values: &[f64],
        instruments: &[&Box<dyn CalibrationInstrument>],
    ) -> CurveResult<Vec<Vec<f64>>> {
        let n = tenors.len();
        let m = instruments.len();
        let h = self.config.jacobian_step;

        let mut jacobian = vec![vec![0.0; n]; m];

        for j in 0..n {
            // Bump value j up
            let mut values_up = values.to_vec();
            values_up[j] += h;

            let curve_up = DiscreteCurve::new(
                reference_date,
                tenors.to_vec(),
                values_up,
                self.value_type.clone(),
                self.interpolation,
            )?;
            let rate_curve_up = RateCurve::new(curve_up);

            // Bump value j down
            let mut values_down = values.to_vec();
            values_down[j] -= h;

            let curve_down = DiscreteCurve::new(
                reference_date,
                tenors.to_vec(),
                values_down,
                self.value_type.clone(),
                self.interpolation,
            )?;
            let rate_curve_down = RateCurve::new(curve_down);

            // Central difference for each instrument
            for i in 0..m {
                let r_up = checked_residual(instruments[i].pricing_error(&rate_curve_up))?;
                let r_down = checked_residual(instruments[i].pricing_error(&rate_curve_down))?;
                jacobian[i][j] = finite_calibration((r_up - r_down) / (2.0 * h))?;
            }
        }

        Ok(jacobian)
    }

    /// Computes J^T * J.
    fn matrix_multiply_transpose(
        &self,
        j: &[Vec<f64>],
        _j2: &[Vec<f64>],
        n: usize,
    ) -> CurveResult<Vec<Vec<f64>>> {
        let m = j.len();
        let mut result = vec![vec![0.0; n]; n];

        for i in 0..n {
            for k in 0..n {
                for row in 0..m {
                    result[i][k] = finite_calibration(result[i][k] + j[row][i] * j[row][k])?;
                }
            }
        }

        Ok(result)
    }

    /// Computes J^T * r.
    fn vector_multiply_transpose(
        &self,
        j: &[Vec<f64>],
        r: &[f64],
        n: usize,
    ) -> CurveResult<Vec<f64>> {
        let m = j.len();
        let mut result = vec![0.0; n];

        for i in 0..n {
            for row in 0..m {
                result[i] = finite_calibration(result[i] + j[row][i] * r[row])?;
            }
        }

        Ok(result)
    }

    /// Solves (A + λI) x = b using simple Gaussian elimination.
    fn solve_damped_system(
        &self,
        a: &[Vec<f64>],
        b: &[f64],
        lambda: f64,
        n: usize,
    ) -> CurveResult<Vec<f64>> {
        // Add damping to diagonal
        let mut aug = vec![vec![0.0; n + 1]; n];
        for i in 0..n {
            for j in 0..n {
                aug[i][j] = a[i][j];
            }
            aug[i][i] = finite_calibration(aug[i][i] + lambda)?;
            aug[i][n] = b[i];
        }

        // Forward elimination with partial pivoting
        for k in 0..n {
            // Find pivot
            let mut max_row = k;
            for i in k + 1..n {
                if aug[i][k].abs() > aug[max_row][k].abs() {
                    max_row = i;
                }
            }
            aug.swap(k, max_row);

            if aug[k][k].abs() < 1e-15 {
                return Err(CurveError::invalid_value(
                    "singular damped calibration system",
                ));
            }

            // Eliminate
            for i in k + 1..n {
                let factor = finite_calibration(aug[i][k] / aug[k][k])?;
                for j in k..=n {
                    aug[i][j] = finite_calibration(aug[i][j] - factor * aug[k][j])?;
                }
            }
        }

        // Back substitution
        let mut x = vec![0.0; n];
        for i in (0..n).rev() {
            let mut sum = aug[i][n];
            for j in i + 1..n {
                sum = finite_calibration(sum - aug[i][j] * x[j])?;
            }
            x[i] = finite_calibration(sum / aug[i][i])?;
        }

        Ok(x)
    }
}

/// Approximate sequential quote curve for diagnostics.
///
/// This method does not solve instrument repricing; use `PiecewiseBootstrapper`
/// for an enforced residual tolerance. Its historical completion flag reports
/// construction, not calibration accuracy.
///
/// Builds an approximate curve from maturity-ordered quotes.
#[derive(Debug, Clone, Default)]
pub struct SequentialBootstrapper {
    /// Value type for output curve.
    value_type: ValueType,
    /// Interpolation method.
    interpolation: InterpolationMethod,
}

impl SequentialBootstrapper {
    /// Creates a new bootstrapper.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the output value type.
    #[must_use]
    pub fn output_as(mut self, value_type: ValueType) -> Self {
        self.value_type = value_type;
        self
    }

    /// Bootstraps a curve from instruments.
    pub fn bootstrap(
        &self,
        reference_date: Date,
        instruments: &InstrumentSet,
    ) -> CurveResult<CalibrationResult> {
        validate_instruments(reference_date, instruments)?;

        // Sort by maturity
        let mut sorted: Vec<_> = instruments.instruments().iter().collect();
        sorted.sort_by(|a, b| a.tenor().total_cmp(&b.tenor()));

        // Bootstrap each instrument (no t=0 point needed for simple bootstrap)
        let mut tenors = Vec::with_capacity(sorted.len());
        let mut values = Vec::with_capacity(sorted.len());

        // Bootstrap each instrument
        for inst in &sorted {
            let tenor = inst.tenor();
            let quote = inst.quote();

            // For simple deposits, zero rate ≈ quote
            // This is a simplified bootstrap - full version would solve for exact DF
            tenors.push(tenor);
            values.push(quote);
        }

        let curve = DiscreteCurve::new(
            reference_date,
            tenors,
            values,
            self.value_type.clone(),
            self.interpolation,
        )?;

        // Compute residuals
        let rate_curve = RateCurve::new(curve.clone());
        let residuals: Vec<f64> = sorted
            .iter()
            .map(|inst| checked_residual(inst.pricing_error(&rate_curve)))
            .collect::<CurveResult<_>>()?;

        let rms = finite_calibration(
            (residuals.iter().map(|r| r * r).sum::<f64>() / residuals.len() as f64).sqrt(),
        )?;

        Ok(CalibrationResult {
            curve,
            residuals,
            iterations: 1,
            rms_error: rms,
            converged: true,
        })
    }
}

/// Piecewise bootstrapper using iterative root-finding.
///
/// Solves for each discount factor
/// exactly, one instrument at a time in maturity order. Each point is solved
/// using Brent's method to find the discount factor that reprices the instrument.
///
/// # Advantages over Global Fitting
///
/// - Exact fit to market quotes (within solver tolerance)
/// - Faster for standard curves
/// - More predictable behavior
/// - Industry standard approach
///
/// # Disadvantages
///
/// - Sensitive to instrument ordering
/// - Can be unstable with noisy data
/// - Doesn't handle overlapping instruments well
#[derive(Debug, Clone)]
pub struct PiecewiseBootstrapper {
    /// Value type for output curve.
    value_type: ValueType,
    /// Interpolation method.
    interpolation: InterpolationMethod,
    /// Solver tolerance.
    tolerance: f64,
    /// Maximum solver iterations per point.
    max_iterations: usize,
}

impl Default for PiecewiseBootstrapper {
    fn default() -> Self {
        Self {
            value_type: ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            },
            interpolation: InterpolationMethod::Linear,
            tolerance: 1e-12,
            max_iterations: 100,
        }
    }
}

impl PiecewiseBootstrapper {
    /// Creates a new piecewise bootstrapper.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the output value type.
    #[must_use]
    pub fn output_as(mut self, value_type: ValueType) -> Self {
        self.value_type = value_type;
        self
    }

    /// Sets the interpolation method.
    #[must_use]
    pub fn interpolation(mut self, method: InterpolationMethod) -> Self {
        self.interpolation = method;
        self
    }

    /// Sets the solver tolerance.
    #[must_use]
    pub fn tolerance(mut self, tol: f64) -> Self {
        self.tolerance = tol;
        self
    }

    /// Bootstraps a curve from instruments using iterative root-finding.
    ///
    /// For each instrument in maturity order:
    /// 1. Build a temporary curve from previously solved points
    /// 2. Use Brent's method to find the zero rate that reprices the instrument
    /// 3. Add the solved point to the curve
    pub fn bootstrap(
        &self,
        reference_date: Date,
        instruments: &InstrumentSet,
    ) -> CurveResult<CalibrationResult> {
        use convex_math::solvers::SolverConfig;

        validate_instruments(reference_date, instruments)?;
        let solver_config = SolverConfig {
            tolerance: self.tolerance,
            max_iterations: u32::try_from(self.max_iterations)
                .map_err(|_| CurveError::invalid_value("bootstrap iteration budget overflow"))?,
        };
        solver_config
            .validate()
            .map_err(|error| CurveError::MathError {
                reason: error.to_string(),
            })?;
        let mut sorted: Vec<_> = instruments.instruments().iter().collect();
        sorted.sort_by(|a, b| a.tenor().total_cmp(&b.tenor()));
        let mut tenors = vec![0.0];
        let mut values = vec![sorted[0].quote()];
        let mut total_iterations = 0usize;

        // The quote is an initial guess, never a fallback for a failed solve.
        for (index, instrument) in sorted.iter().enumerate() {
            tenors.push(instrument.tenor());
            values.push(instrument.quote());
            let solution = self.solve_pillar(
                reference_date,
                &tenors,
                &values,
                index + 1,
                instrument.as_ref(),
                &solver_config,
            )?;
            values[index + 1] = solution.root;
            total_iterations += solution.iterations as usize;
        }

        // A later node can affect earlier cashflow interpolation. Check the
        // entire curve after each bounded sweep without relaxing tolerance.
        for pass in 0..=MAX_REFINEMENT_PASSES {
            let curve = DiscreteCurve::new(
                reference_date,
                tenors.clone(),
                values.clone(),
                self.value_type.clone(),
                self.interpolation,
            )?;
            let rate_curve = RateCurve::new(curve.clone());
            let residuals: Vec<f64> = sorted
                .iter()
                .map(|inst| checked_residual(inst.pricing_error(&rate_curve)))
                .collect::<CurveResult<_>>()?;
            let rms = finite_calibration(
                (residuals.iter().map(|r| r * r).sum::<f64>() / residuals.len() as f64).sqrt(),
            )?;
            if rms < self.tolerance {
                return Ok(CalibrationResult {
                    curve,
                    residuals,
                    iterations: total_iterations,
                    rms_error: rms,
                    converged: true,
                });
            }
            if pass == MAX_REFINEMENT_PASSES {
                return Err(CurveError::calibration_failed(
                    total_iterations,
                    rms,
                    "bootstrap refinement limit reached without requested repricing tolerance",
                ));
            }
            for (index, instrument) in sorted.iter().enumerate() {
                let solution = self.solve_pillar(
                    reference_date,
                    &tenors,
                    &values,
                    index + 1,
                    instrument.as_ref(),
                    &solver_config,
                )?;
                values[index + 1] = solution.root;
                total_iterations += solution.iterations as usize;
            }
        }
        Err(CurveError::invalid_value("bootstrap refinement exhausted"))
    }

    fn solve_pillar(
        &self,
        date: Date,
        tenors: &[f64],
        values: &[f64],
        pillar: usize,
        instrument: &dyn CalibrationInstrument,
        config: &convex_math::solvers::SolverConfig,
    ) -> CurveResult<convex_math::solvers::SolverResult> {
        let callback_error = std::cell::RefCell::new(None);
        let objective = |rate: f64| {
            let mut candidate = values.to_vec();
            candidate[pillar] = rate;
            let result = DiscreteCurve::new(
                date,
                tenors.to_vec(),
                candidate,
                self.value_type.clone(),
                self.interpolation,
            )
            .and_then(|curve| checked_residual(instrument.pricing_error(&RateCurve::new(curve))));
            match result {
                Ok(value) => value,
                Err(error) => {
                    *callback_error.borrow_mut() = Some(error);
                    f64::NAN
                }
            }
        };
        let result = convex_math::solvers::brent(objective, -0.10, 0.50, config);
        if let Some(error) = callback_error.into_inner() {
            return Err(error);
        }
        result.map_err(|error| CurveError::MathError {
            reason: error.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibration::instruments::{Deposit, Ois, Swap};
    use approx::assert_relative_eq;
    use convex_core::types::Frequency;

    fn make_flat_instruments(today: Date, rate: f64) -> InstrumentSet {
        let dc = DayCountConvention::Act360;

        InstrumentSet::new()
            .with(Deposit::from_tenor(today, 0.25, rate, dc))
            .with(Deposit::from_tenor(today, 0.5, rate, dc))
            .with(Deposit::from_tenor(today, 1.0, rate, dc))
            .with(Swap::from_tenor(
                today,
                2.0,
                rate,
                Frequency::SemiAnnual,
                DayCountConvention::Thirty360US,
            ))
            .with(Swap::from_tenor(
                today,
                5.0,
                rate,
                Frequency::SemiAnnual,
                DayCountConvention::Thirty360US,
            ))
    }

    #[test]
    fn test_fitter_config() {
        let config = FitterConfig::default()
            .with_max_iterations(50)
            .with_tolerance(1e-8);

        assert_eq!(config.max_iterations, 50);
        assert_relative_eq!(config.tolerance, 1e-8);
    }

    #[test]
    fn test_global_fitter_flat_curve() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let instruments = make_flat_instruments(today, 0.04);

        let fitter = GlobalFitter::new();
        let result = fitter.fit(today, &instruments).unwrap();

        // Should converge with small error on flat curve
        assert!(result.rms_error < 0.001); // Less than 10bp
        println!("{}", result.summary());
    }

    #[test]
    fn test_sequential_bootstrap() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let instruments = make_flat_instruments(today, 0.04);

        let bootstrapper = SequentialBootstrapper::new();
        let result = bootstrapper.bootstrap(today, &instruments).unwrap();

        assert!(result.converged);
        assert_eq!(result.iterations, 1);
    }

    #[test]
    fn test_piecewise_bootstrap() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let instruments = make_flat_instruments(today, 0.04);

        let bootstrapper = PiecewiseBootstrapper::new();
        let result = bootstrapper.bootstrap(today, &instruments).unwrap();

        // Piecewise should achieve very tight fit
        assert!(
            result.rms_error < 1e-6,
            "RMS error too high: {}",
            result.rms_error
        );
        assert!(result.converged);
        println!("Piecewise bootstrap: {}", result.summary());
    }

    #[test]
    fn test_piecewise_vs_global() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let dc = DayCountConvention::Act360;

        // Create upward sloping curve
        let instruments = InstrumentSet::new()
            .with(Deposit::from_tenor(today, 0.25, 0.035, dc))
            .with(Deposit::from_tenor(today, 0.5, 0.040, dc))
            .with(Deposit::from_tenor(today, 1.0, 0.045, dc));

        // Both methods should work
        let global_result = GlobalFitter::new().fit(today, &instruments).unwrap();
        let piecewise_result = PiecewiseBootstrapper::new()
            .bootstrap(today, &instruments)
            .unwrap();

        println!("Global RMS: {:.2e}", global_result.rms_error);
        println!("Piecewise RMS: {:.2e}", piecewise_result.rms_error);

        // Both should achieve good fit
        assert!(global_result.rms_error < 0.01);
        assert!(piecewise_result.rms_error < 1e-6);
    }

    #[test]
    fn test_calibration_result_summary() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let instruments = make_flat_instruments(today, 0.045);

        let fitter = GlobalFitter::new();
        let result = fitter.fit(today, &instruments).unwrap();

        let summary = result.summary();
        assert!(summary.contains("converged") || summary.contains("FAILED"));
    }

    #[test]
    fn test_empty_instruments_error() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let instruments = InstrumentSet::new();

        let fitter = GlobalFitter::new();
        let result = fitter.fit(today, &instruments);

        assert!(result.is_err());
    }

    #[test]
    fn test_upward_sloping_curve() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let dc = DayCountConvention::Act360;

        // Create upward sloping curve instruments
        let instruments = InstrumentSet::new()
            .with(Deposit::from_tenor(today, 0.25, 0.035, dc))
            .with(Deposit::from_tenor(today, 0.5, 0.038, dc))
            .with(Deposit::from_tenor(today, 1.0, 0.042, dc))
            .with(Swap::from_tenor(
                today,
                2.0,
                0.045,
                Frequency::SemiAnnual,
                DayCountConvention::Thirty360US,
            ))
            .with(Swap::from_tenor(
                today,
                5.0,
                0.050,
                Frequency::SemiAnnual,
                DayCountConvention::Thirty360US,
            ));

        let fitter = GlobalFitter::new();
        let result = fitter.fit(today, &instruments).unwrap();

        // Should calibrate with reasonable accuracy
        assert!(result.rms_error < 0.005); // Less than 50bp
        println!("Upward sloping: {}", result.summary());
    }

    #[test]
    fn test_ois_calibration() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let dc = DayCountConvention::Act360;

        let instruments = InstrumentSet::new()
            .with(Ois::from_tenor(today, 0.25, 0.04, dc))
            .with(Ois::from_tenor(today, 0.5, 0.041, dc))
            .with(Ois::from_tenor(today, 1.0, 0.042, dc))
            .with(Ois::from_tenor(today, 2.0, 0.044, dc));

        let fitter = GlobalFitter::new();
        let result = fitter.fit(today, &instruments).unwrap();

        println!("OIS curve: {}", result.summary());
        assert!(result.rms_error < 0.01);
    }

    #[test]
    fn test_errors_in_bps() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let instruments = make_flat_instruments(today, 0.04);

        let fitter = GlobalFitter::new();
        let result = fitter.fit(today, &instruments).unwrap();

        let bps = result.errors_bps();
        assert_eq!(bps.len(), 5);

        // All errors should be small in bps
        for &e in &bps {
            assert!(e.abs() < 100.0); // Less than 100bp
        }
    }
}
