//! Bounded, scaled least squares for setup/calibration, not a hot-path kernel.
//! The caller supplies a terminating, nonpanicking residual function in native
//! units. A small objective alone never substitutes for native postconditions.

use crate::portable;
use nalgebra::{linalg::SVD, DMatrix, DVector, Dyn};

use crate::bounds::MAX_SOLVER_ITERATIONS;

/// Maximum parameters and residuals in this dense solver.
pub const MAX_FIT_DIMENSION: usize = 128;

/// Parameter start and closed admissible interval.
#[derive(Debug, Clone, Copy)]
pub struct FitParameter {
    /// Supplied starting value.
    pub initial: f64,
    /// Lower admissible value.
    pub lower: f64,
    /// Upper admissible value.
    pub upper: f64,
}

/// Native residual contract; scaling changes the objective, never its units.
#[derive(Debug, Clone, Copy)]
pub struct FitResidual {
    /// Positive native-unit normalization scale.
    pub scale: f64,
    /// Nonnegative objective weight. Zero does not waive native repricing.
    pub weight: f64,
    /// Positive absolute native residual bound, enforced on every output.
    pub tolerance: f64,
}

/// Explicit numerical choices. Existing root/fitter defaults are unaffected.
#[derive(Debug, Clone, Copy)]
pub struct LeastSquaresSettings {
    /// Maximum accepted or rejected LM attempts.
    pub max_iterations: usize,
    /// Weighted RMS target, after division by native scales.
    pub objective_tolerance: f64,
    /// Absolute parameter perturbation for a bounded finite-difference Jacobian.
    pub jacobian_step: f64,
    /// Smallest admitted singular value divided by the largest, in (0,1).
    pub minimum_singular_ratio: f64,
    /// Positive iteration budget for each SVD; zero/unbounded is prohibited.
    pub svd_iterations: usize,
}
impl Default for LeastSquaresSettings {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            objective_tolerance: 1e-10,
            jacobian_step: 1e-6,
            minimum_singular_ratio: 1e-10,
            svd_iterations: 1000,
        }
    }
}

/// Measured termination/identification information. No parameter/curve is
/// attached to a failure. Missing metrics mean the computation was not reached.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FitDiagnostics {
    /// Actual LM attempts, excluding initial validation/evaluation.
    pub iterations: usize,
    /// Actual residual callback invocations, including finite differences.
    pub evaluations: usize,
    /// Last successfully evaluated weighted scaled RMS.
    pub scaled_rms: Option<f64>,
    /// Last native residuals in caller order.
    pub native_residuals: Vec<f64>,
    /// Numerically identified rank of the undamped, weighted Jacobian.
    pub rank: Option<usize>,
    /// Largest/smallest singular value; absent for zero/rank-deficient systems.
    pub condition_estimate: Option<f64>,
    /// Number of parameters on their supplied bounds.
    pub active_bounds: usize,
}

/// Distinct invalid, callback, numerical and solver stopping outcomes.
#[derive(Debug, thiserror::Error)]
pub enum FitFailureReason<E> {
    /// Invalid dimensions, finite domain, settings or resource budget.
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),
    /// Original caller error, never converted to a zero residual.
    #[error("callback failed: {0}")]
    Callback(E),
    /// A finite arithmetic or output postcondition failed.
    #[error("nonfinite numerical arithmetic: {0}")]
    Numerical(&'static str),
    /// Weighted undamped Jacobian cannot identify all parameters.
    #[error("ill-conditioned or rank-deficient Jacobian")]
    IllConditioned,
    /// A bounded SVD failed to finish.
    #[error("SVD iteration limit")]
    DecompositionLimit,
    /// Requested residual postconditions unmet at the LM iteration limit.
    #[error("iteration limit without requested residual postconditions")]
    IterationLimit,
    /// No admissible progress at bounds or maximum damping.
    #[error("stagnation without requested residual postconditions")]
    Stagnation,
}

/// A failed fit carries diagnostics, never a valid fitted parameter vector.
#[derive(Debug, thiserror::Error)]
#[error("{reason}")]
pub struct FitFailure<E> {
    /// Actual cause.
    pub reason: FitFailureReason<E>,
    /// Last available measurements.
    pub diagnostics: FitDiagnostics,
}

/// Fully identified fitted parameters satisfying both objective/native bounds.
#[derive(Debug, Clone)]
pub struct LeastSquaresOutput {
    /// Parameters inside their supplied bounds.
    pub parameters: Vec<f64>,
    /// Actual convergence and rank information.
    pub diagnostics: FitDiagnostics,
}

type FitResult<T, E> = Result<T, FitFailure<E>>;
type DenseSvd = SVD<f64, Dyn, Dyn>;

struct Problem<'a, F> {
    callback: F,
    parameters: &'a [FitParameter],
    residuals: &'a [FitResidual],
    settings: LeastSquaresSettings,
    diagnostics: FitDiagnostics,
}
impl<F> Problem<'_, F> {
    fn fail<E>(&self, reason: FitFailureReason<E>) -> FitFailure<E> {
        FitFailure {
            reason,
            diagnostics: self.diagnostics.clone(),
        }
    }
    fn finite<E>(&self, value: f64, operation: &'static str) -> FitResult<f64, E> {
        if value.is_finite() {
            Ok(value)
        } else {
            Err(self.fail(FitFailureReason::Numerical(operation)))
        }
    }
    fn validate<E>(&self) -> FitResult<(), E> {
        let settings = self.settings;
        let count = self.parameters.len();
        let invalid = count == 0
            || count > MAX_FIT_DIMENSION
            || self.residuals.len() < count
            || self.residuals.len() > MAX_FIT_DIMENSION
            || settings.max_iterations == 0
            || settings.max_iterations > MAX_SOLVER_ITERATIONS as usize
            || settings.svd_iterations == 0
            || settings.svd_iterations > MAX_SOLVER_ITERATIONS as usize
            || !settings.objective_tolerance.is_finite()
            || settings.objective_tolerance <= 0.0
            || !settings.jacobian_step.is_finite()
            || settings.jacobian_step <= 0.0
            || !settings.minimum_singular_ratio.is_finite()
            || !(0.0..1.0).contains(&settings.minimum_singular_ratio)
            || settings.minimum_singular_ratio == 0.0;
        if invalid {
            return Err(self.fail(FitFailureReason::InvalidInput(
                "dimensions or solver settings",
            )));
        }
        for p in self.parameters {
            if !p.initial.is_finite()
                || !p.lower.is_finite()
                || !p.upper.is_finite()
                || p.lower >= p.upper
                || !((p.lower)..=(p.upper)).contains(&p.initial)
                || !(p.upper - p.lower).is_finite()
            {
                return Err(self.fail(FitFailureReason::InvalidInput("parameter bounds or start")));
            }
        }
        for r in self.residuals {
            if !r.scale.is_finite()
                || r.scale <= 0.0
                || !r.weight.is_finite()
                || r.weight < 0.0
                || !r.tolerance.is_finite()
                || r.tolerance <= 0.0
            {
                return Err(self.fail(FitFailureReason::InvalidInput(
                    "residual scale, weight or tolerance",
                )));
            }
        }
        if self.residuals.iter().filter(|r| r.weight > 0.0).count() < count {
            return Err(self.fail(FitFailureReason::InvalidInput(
                "too few positive-weight observations",
            )));
        }
        Ok(())
    }
    fn scaled<E>(&self, native: &[f64]) -> FitResult<DVector<f64>, E> {
        let values = native
            .iter()
            .zip(self.residuals)
            .map(|(value, contract)| {
                self.finite(
                    (value / contract.scale) * contract.weight.sqrt(),
                    "scaled residual",
                )
            })
            .collect::<FitResult<Vec<_>, E>>()?;
        Ok(DVector::from_vec(values))
    }
    fn measure<E>(&mut self, native: &[f64], parameters: &[f64]) -> FitResult<DVector<f64>, E> {
        let scaled = self.scaled(native)?;
        let weight = self.finite(
            self.residuals.iter().map(|r| r.weight).sum(),
            "total weight",
        )?;
        // hypot avoids squaring an otherwise finite large residual into infinity.
        let norm = scaled
            .iter()
            .fold(0.0_f64, |n, value| portable::hypot(n, *value));
        let rms = self.finite(norm / weight.sqrt(), "scaled RMS")?;
        self.diagnostics.scaled_rms = Some(rms);
        self.diagnostics.native_residuals = native.to_vec();
        self.diagnostics.active_bounds = parameters
            .iter()
            .zip(self.parameters)
            .filter(|(v, p)| **v == p.lower || **v == p.upper)
            .count();
        Ok(scaled)
    }
    fn converged(&self) -> bool {
        self.diagnostics
            .scaled_rms
            .is_some_and(|rms| rms <= self.settings.objective_tolerance)
            && self
                .diagnostics
                .native_residuals
                .iter()
                .zip(self.residuals)
                .all(|(r, contract)| r.abs() <= contract.tolerance)
    }
    fn svd<E>(&self, matrix: DMatrix<f64>, vectors: bool) -> FitResult<DenseSvd, E> {
        if matrix.iter().any(|v| !v.is_finite()) {
            return Err(self.fail(FitFailureReason::Numerical("Jacobian or damping matrix")));
        }
        let svd = SVD::try_new(
            matrix,
            vectors,
            vectors,
            5.0 * f64::EPSILON,
            self.settings.svd_iterations,
        )
        .ok_or_else(|| self.fail(FitFailureReason::DecompositionLimit))?;
        if svd.singular_values.iter().any(|v| !v.is_finite()) {
            return Err(self.fail(FitFailureReason::Numerical("singular values")));
        }
        Ok(svd)
    }
    fn identify<E>(&mut self, jacobian: &DMatrix<f64>) -> FitResult<(), E> {
        let svd = self.svd(jacobian.clone(), false)?;
        let largest = svd.singular_values.iter().copied().fold(0.0, f64::max);
        let smallest = svd
            .singular_values
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        let rank = svd
            .singular_values
            .iter()
            .filter(|&&s| s > 0.0 && s / largest >= self.settings.minimum_singular_ratio)
            .count();
        self.diagnostics.rank = Some(rank);
        self.diagnostics.condition_estimate = if smallest > 0.0 {
            Some(self.finite(largest / smallest, "condition estimate")?)
        } else {
            None
        };
        if rank != self.parameters.len() {
            return Err(self.fail(FitFailureReason::IllConditioned));
        }
        Ok(())
    }
    fn step<E>(
        &self,
        jacobian: &DMatrix<f64>,
        residual: &DVector<f64>,
        lambda: f64,
    ) -> FitResult<DVector<f64>, E> {
        let rows = jacobian.nrows();
        let columns = jacobian.ncols();
        // Solve [J; sqrt(lambda) I] delta = [-r; 0]. No J'J or inverse.
        let matrix = DMatrix::from_fn(rows + columns, columns, |i, j| {
            if i < rows {
                jacobian[(i, j)]
            } else if i - rows == j {
                lambda.sqrt()
            } else {
                0.0
            }
        });
        let rhs = DVector::from_fn(
            rows + columns,
            |i, _| if i < rows { -residual[i] } else { 0.0 },
        );
        let svd = self.svd(matrix, true)?;
        let step = svd
            .solve(&rhs, 0.0)
            .map_err(|_| self.fail(FitFailureReason::Numerical("damped SVD solve")))?;
        if step.iter().any(|v| !v.is_finite()) {
            return Err(self.fail(FitFailureReason::Numerical("parameter step")));
        }
        Ok(step)
    }
}
impl<F> Problem<'_, F> {
    fn evaluate<E>(&mut self, values: &[f64]) -> FitResult<Vec<f64>, E>
    where
        F: FnMut(&[f64]) -> Result<Vec<f64>, E>,
    {
        self.diagnostics.evaluations += 1;
        let native =
            (self.callback)(values).map_err(|e| self.fail(FitFailureReason::Callback(e)))?;
        if native.len() != self.residuals.len() {
            return Err(self.fail(FitFailureReason::InvalidInput(
                "callback residual dimension",
            )));
        }
        if native.iter().any(|r| !r.is_finite()) {
            return Err(self.fail(FitFailureReason::Numerical("callback residual")));
        }
        Ok(native)
    }
    fn jacobian<E>(&mut self, values: &[f64]) -> FitResult<DMatrix<f64>, E>
    where
        F: FnMut(&[f64]) -> Result<Vec<f64>, E>,
    {
        let mut matrix = DMatrix::zeros(self.residuals.len(), values.len());
        for (j, parameter) in self.parameters.iter().enumerate() {
            let mut up = values.to_vec();
            let mut down = values.to_vec();
            up[j] = self
                .finite(
                    values[j] + self.settings.jacobian_step,
                    "Jacobian perturbation",
                )?
                .min(parameter.upper);
            down[j] = self
                .finite(
                    values[j] - self.settings.jacobian_step,
                    "Jacobian perturbation",
                )?
                .max(parameter.lower);
            let span = self.finite(up[j] - down[j], "Jacobian span")?;
            if span <= 0.0 {
                return Err(self.fail(FitFailureReason::Stagnation));
            }
            let up_native = self.evaluate(&up)?;
            let down_native = self.evaluate(&down)?;
            let up_scaled = self.scaled(&up_native)?;
            let down_scaled = self.scaled(&down_native)?;
            for i in 0..self.residuals.len() {
                matrix[(i, j)] =
                    self.finite((up_scaled[i] - down_scaled[i]) / span, "Jacobian entry")?;
            }
        }
        Ok(matrix)
    }
    fn solve<E>(&mut self) -> FitResult<LeastSquaresOutput, E>
    where
        F: FnMut(&[f64]) -> Result<Vec<f64>, E>,
    {
        self.validate()?;
        let mut values: Vec<_> = self.parameters.iter().map(|p| p.initial).collect();
        let native = self.evaluate(&values)?;
        let mut residual = self.measure(&native, &values)?;
        let mut lambda = 1e-3_f64;
        for iteration in 0..=self.settings.max_iterations {
            // Identification is checked even when starting at an exact fit.
            let jacobian = self.jacobian(&values)?;
            self.identify(&jacobian)?;
            if self.converged() {
                return Ok(LeastSquaresOutput {
                    parameters: values,
                    diagnostics: self.diagnostics.clone(),
                });
            }
            if iteration == self.settings.max_iterations {
                return Err(self.fail(FitFailureReason::IterationLimit));
            }
            let delta = self.step(&jacobian, &residual, lambda)?;
            let trial = values
                .iter()
                .zip(delta.iter())
                .zip(self.parameters)
                .map(|((v, d), p)| {
                    self.finite(v + d, "trial parameter")
                        .map(|value| value.clamp(p.lower, p.upper))
                })
                .collect::<FitResult<Vec<_>, E>>()?;
            self.diagnostics.iterations += 1;
            if trial == values {
                return Err(self.fail(FitFailureReason::Stagnation));
            }
            let trial_native = self.evaluate(&trial)?;
            let trial_scaled = self.scaled(&trial_native)?;
            let norm = |v: &DVector<f64>| v.iter().fold(0.0_f64, |n, r| portable::hypot(n, *r));
            if norm(&trial_scaled) < norm(&residual) {
                values = trial;
                residual = self.measure(&trial_native, &values)?;
                lambda = (lambda / 10.0).max(1e-12);
            } else {
                if lambda >= 1e12 {
                    return Err(self.fail(FitFailureReason::Stagnation));
                }
                lambda = (lambda * 10.0).min(1e12);
            }
        }
        Err(self.fail(FitFailureReason::IterationLimit))
    }
}

/// Fit native residuals with explicit scales/weights and closed parameter bounds.
/// Every callback error propagates; no fallback, infinite iteration, or partial
/// fitted output is permitted. At most 128 parameters/residuals and 10,000 LM
/// attempts; each SVD has its own positive budget. Setup allocations are bounded
/// by dimensions. User callbacks must terminate and must not panic.
pub fn bounded_least_squares<F, E>(
    parameters: &[FitParameter],
    residuals: &[FitResidual],
    settings: LeastSquaresSettings,
    callback: F,
) -> FitResult<LeastSquaresOutput, E>
where
    F: FnMut(&[f64]) -> Result<Vec<f64>, E>,
{
    Problem {
        callback,
        parameters,
        residuals,
        settings,
        diagnostics: FitDiagnostics::default(),
    }
    .solve()
}
