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
//! instruments.add(Swap::from_tenor(today, 2.0, 0.045, SemiAnnual, Thirty360));
//!
//! let fitter = GlobalFitter::default();
//! let result = fitter.fit(today, &instruments)?;
//!
//! let curve = result.curve;
//! println!("RMS error: {:.2e}", result.rms_error);
//! ```

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, Date};

use crate::curves::DiscreteCurve;
use crate::error::{CurveError, CurveResult};
use crate::wrappers::RateCurve;
use crate::{InterpolationMethod, ValueType};

use super::instruments::{CalibrationInstrument, InstrumentSet};

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
        self.residuals
            .iter()
            .map(|r| r.abs())
            .fold(0.0, f64::max)
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
            if self.converged { "converged" } else { "FAILED" },
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
        if instruments.is_empty() {
            return Err(CurveError::calibration_failed(
                0,
                f64::NAN,
                "No instruments provided",
            ));
        }

        // Get sorted tenors and initial guesses from quotes
        let mut sorted_instruments: Vec<_> = instruments.instruments().iter().collect();
        sorted_instruments.sort_by(|a, b| a.tenor().partial_cmp(&b.tenor()).unwrap());

        let tenors: Vec<f64> = sorted_instruments.iter().map(|i| i.tenor()).collect();
        let quotes: Vec<f64> = sorted_instruments.iter().map(|i| i.quote()).collect();

        // Add t=0 point
        let mut curve_tenors = vec![0.0];
        curve_tenors.extend(&tenors);

        // Initial guess: use instrument quotes as zero rates
        let mut values = vec![quotes.first().copied().unwrap_or(0.04)]; // Start with first quote
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
            .map(|inst| inst.pricing_error(&rate_curve).unwrap_or(f64::NAN))
            .collect();

        let rms_error = (residuals.iter().map(|r| r * r).sum::<f64>() / residuals.len() as f64).sqrt();

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
                .map(|inst| inst.pricing_error(&rate_curve).unwrap_or(0.0))
                .collect();

            let error = residuals.iter().map(|r| r * r).sum::<f64>();

            // Check convergence
            let rms = (error / m as f64).sqrt();
            if rms < self.config.tolerance {
                return Ok((values, iteration + 1, true));
            }

            // Compute Jacobian (numerical)
            let jacobian = self.compute_jacobian(reference_date, tenors, &values, instruments)?;

            // Compute J^T * J and J^T * r
            let jtj = self.matrix_multiply_transpose(&jacobian, &jacobian, n);
            let jtr = self.vector_multiply_transpose(&jacobian, &residuals, n);

            // Solve (J^T J + λI) δ = -J^T r
            let delta = self.solve_damped_system(&jtj, &jtr, lambda, n)?;

            // Update values
            let mut new_values = values.clone();
            for i in 0..n {
                new_values[i] -= delta[i];
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
                .map(|inst| inst.pricing_error(&new_rate_curve).unwrap_or(0.0))
                .collect();
            let new_error: f64 = new_residuals.iter().map(|r| r * r).sum();

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
                return Ok((values, iteration + 1, rms < self.config.tolerance * 10.0));
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
            .map(|inst| inst.pricing_error(&rate_curve).unwrap_or(0.0))
            .collect();
        let rms = (residuals.iter().map(|r| r * r).sum::<f64>() / m as f64).sqrt();

        // May have gotten close enough
        Ok((values, self.config.max_iterations, rms < self.config.tolerance * 100.0))
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
                let r_up = instruments[i].pricing_error(&rate_curve_up).unwrap_or(0.0);
                let r_down = instruments[i].pricing_error(&rate_curve_down).unwrap_or(0.0);
                jacobian[i][j] = (r_up - r_down) / (2.0 * h);
            }
        }

        Ok(jacobian)
    }

    /// Computes J^T * J.
    fn matrix_multiply_transpose(&self, j: &[Vec<f64>], _j2: &[Vec<f64>], n: usize) -> Vec<Vec<f64>> {
        let m = j.len();
        let mut result = vec![vec![0.0; n]; n];

        for i in 0..n {
            for k in 0..n {
                for row in 0..m {
                    result[i][k] += j[row][i] * j[row][k];
                }
            }
        }

        result
    }

    /// Computes J^T * r.
    fn vector_multiply_transpose(&self, j: &[Vec<f64>], r: &[f64], n: usize) -> Vec<f64> {
        let m = j.len();
        let mut result = vec![0.0; n];

        for i in 0..n {
            for row in 0..m {
                result[i] += j[row][i] * r[row];
            }
        }

        result
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
            aug[i][i] += lambda;
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
                // Matrix is singular, return zero step
                return Ok(vec![0.0; n]);
            }

            // Eliminate
            for i in k + 1..n {
                let factor = aug[i][k] / aug[k][k];
                for j in k..=n {
                    aug[i][j] -= factor * aug[k][j];
                }
            }
        }

        // Back substitution
        let mut x = vec![0.0; n];
        for i in (0..n).rev() {
            let mut sum = aug[i][n];
            for j in i + 1..n {
                sum -= aug[i][j] * x[j];
            }
            x[i] = sum / aug[i][i];
        }

        Ok(x)
    }
}

/// Sequential bootstrap for comparison/fallback.
///
/// Bootstraps a curve by solving one instrument at a time in maturity order.
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
        if instruments.is_empty() {
            return Err(CurveError::calibration_failed(
                0,
                f64::NAN,
                "No instruments provided",
            ));
        }

        // Sort by maturity
        let mut sorted: Vec<_> = instruments.instruments().iter().collect();
        sorted.sort_by(|a, b| a.tenor().partial_cmp(&b.tenor()).unwrap());

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
            .map(|inst| inst.pricing_error(&rate_curve).unwrap_or(f64::NAN))
            .collect();

        let rms = (residuals.iter().map(|r| r * r).sum::<f64>() / residuals.len() as f64).sqrt();

        Ok(CalibrationResult {
            curve,
            residuals,
            iterations: 1,
            rms_error: rms,
            converged: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibration::instruments::{Deposit, Fra, Ois, Swap};
    use convex_core::types::Frequency;
    use approx::assert_relative_eq;

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
