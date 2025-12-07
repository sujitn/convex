//! Curve validation and quality checks.
//!
//! Provides tools to validate bootstrapped curves for:
//! - Instrument repricing accuracy
//! - Positive forward rates
//! - Monotonic discount factors
//! - Curve smoothness
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_curves::validation::{CurveValidator, ValidationReport};
//!
//! let validator = CurveValidator::default();
//! let report = validator.validate(&curve, &instruments)?;
//!
//! if report.has_errors() {
//!     println!("Validation failed: {:?}", report.errors());
//! }
//! ```

use crate::curves::DiscountCurve;
use crate::error::CurveResult;
use crate::instruments::CurveInstrument;
use crate::traits::Curve;

/// Validation error types.
#[derive(Debug, Clone)]
pub enum ValidationError {
    /// An instrument failed to reprice to par.
    RepriceFailed {
        /// Instrument description.
        instrument: String,
        /// Calculated PV.
        pv: f64,
        /// Acceptable tolerance.
        tolerance: f64,
    },

    /// Forward rate is negative at a point.
    NegativeForward {
        /// Time in years where negative forward was found.
        time: f64,
        /// The negative forward rate.
        rate: f64,
    },

    /// Discount factors are not monotonically decreasing.
    NonMonotonicDF {
        /// Time in years where non-monotonicity was found.
        time: f64,
        /// DF at this time.
        df: f64,
        /// DF at previous time.
        prev_df: f64,
    },

    /// Curve is not smooth (excessive curvature).
    NotSmooth {
        /// Time in years where excessive curvature was found.
        time: f64,
        /// Second derivative value.
        curvature: f64,
        /// Maximum allowed curvature.
        threshold: f64,
    },

    /// Forward rate exceeds maximum threshold.
    ForwardTooHigh {
        /// Time in years.
        time: f64,
        /// The high forward rate.
        rate: f64,
        /// Maximum allowed rate.
        max_rate: f64,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RepriceFailed {
                instrument,
                pv,
                tolerance,
            } => {
                write!(
                    f,
                    "Reprice failed for {instrument}: PV = {pv:.6} (tolerance: {tolerance:.1e})"
                )
            }
            Self::NegativeForward { time, rate } => {
                write!(
                    f,
                    "Negative forward at t={:.2}Y: {:.4}%",
                    time,
                    rate * 100.0
                )
            }
            Self::NonMonotonicDF { time, df, prev_df } => {
                write!(
                    f,
                    "Non-monotonic DF at t={time:.2}Y: DF={df:.6} >= prev={prev_df:.6}"
                )
            }
            Self::NotSmooth {
                time,
                curvature,
                threshold,
            } => {
                write!(
                    f,
                    "Not smooth at t={time:.2}Y: curvature={curvature:.4} > threshold={threshold:.4}"
                )
            }
            Self::ForwardTooHigh {
                time,
                rate,
                max_rate,
            } => {
                write!(
                    f,
                    "Forward too high at t={:.2}Y: {:.4}% > max {:.4}%",
                    time,
                    rate * 100.0,
                    max_rate * 100.0
                )
            }
        }
    }
}

/// Validation warning types.
#[derive(Debug, Clone)]
pub enum ValidationWarning {
    /// An instrument repriced with small but non-zero PV.
    RepriceImprecise {
        /// Instrument description.
        instrument: String,
        /// Calculated PV.
        pv: f64,
    },

    /// Forward rates are inverted (short end higher than long end).
    InvertedCurve {
        /// Time range where inversion starts.
        start_time: f64,
        /// Time range where inversion ends.
        end_time: f64,
    },

    /// Zero rate is unusually high or low.
    UnusualZeroRate {
        /// Time in years.
        time: f64,
        /// The unusual rate.
        rate: f64,
    },
}

impl std::fmt::Display for ValidationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RepriceImprecise { instrument, pv } => {
                write!(f, "Imprecise reprice for {instrument}: PV = {pv:.6}")
            }
            Self::InvertedCurve {
                start_time,
                end_time,
            } => {
                write!(
                    f,
                    "Inverted curve from t={start_time:.2}Y to t={end_time:.2}Y"
                )
            }
            Self::UnusualZeroRate { time, rate } => {
                write!(
                    f,
                    "Unusual zero rate at t={:.2}Y: {:.4}%",
                    time,
                    rate * 100.0
                )
            }
        }
    }
}

/// Result of curve validation.
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    /// Validation errors (curve should not be used if present).
    errors: Vec<ValidationError>,
    /// Validation warnings (curve usable but with caveats).
    warnings: Vec<ValidationWarning>,
    /// Per-instrument PV residuals.
    residuals: Vec<(String, f64)>,
    /// Maximum absolute PV residual.
    max_residual: f64,
    /// Root mean square of PV residuals.
    rms_residual: f64,
}

impl ValidationReport {
    /// Creates a new empty report.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if validation passed (no errors).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns true if there are errors.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Returns true if there are warnings.
    #[must_use]
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    /// Returns the validation errors.
    #[must_use]
    pub fn errors(&self) -> &[ValidationError] {
        &self.errors
    }

    /// Returns the validation warnings.
    #[must_use]
    pub fn warnings(&self) -> &[ValidationWarning] {
        &self.warnings
    }

    /// Returns the per-instrument residuals.
    #[must_use]
    pub fn residuals(&self) -> &[(String, f64)] {
        &self.residuals
    }

    /// Returns the maximum absolute residual.
    #[must_use]
    pub fn max_residual(&self) -> f64 {
        self.max_residual
    }

    /// Returns the RMS residual.
    #[must_use]
    pub fn rms_residual(&self) -> f64 {
        self.rms_residual
    }

    /// Adds an error to the report.
    pub fn add_error(&mut self, error: ValidationError) {
        self.errors.push(error);
    }

    /// Adds a warning to the report.
    pub fn add_warning(&mut self, warning: ValidationWarning) {
        self.warnings.push(warning);
    }
}

impl std::fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Validation Report:")?;
        writeln!(
            f,
            "  Status: {}",
            if self.is_valid() { "PASSED" } else { "FAILED" }
        )?;
        writeln!(f, "  Max Residual: {:.2e}", self.max_residual)?;
        writeln!(f, "  RMS Residual: {:.2e}", self.rms_residual)?;

        if !self.errors.is_empty() {
            writeln!(f, "  Errors ({}):", self.errors.len())?;
            for err in &self.errors {
                writeln!(f, "    - {err}")?;
            }
        }

        if !self.warnings.is_empty() {
            writeln!(f, "  Warnings ({}):", self.warnings.len())?;
            for warn in &self.warnings {
                writeln!(f, "    - {warn}")?;
            }
        }

        Ok(())
    }
}

/// Curve validator with configurable tolerances.
#[derive(Debug, Clone)]
pub struct CurveValidator {
    /// Tolerance for instrument repricing (absolute PV).
    reprice_tolerance: f64,
    /// Floor for forward rates (typically 0.0 for positive forwards).
    forward_floor: f64,
    /// Ceiling for forward rates (sanity check).
    forward_ceiling: f64,
    /// Maximum curvature for smoothness check.
    smoothness_threshold: f64,
    /// Warning threshold for repricing (less strict than error).
    reprice_warning_threshold: f64,
    /// Check interval for forward rates (in years).
    forward_check_interval: f64,
    /// Maximum maturity to check (in years).
    max_maturity_check: f64,
}

impl Default for CurveValidator {
    fn default() -> Self {
        Self {
            reprice_tolerance: 1e-6,   // 0.0001 bps
            forward_floor: -0.01,      // Allow slightly negative (for market stress)
            forward_ceiling: 0.30,     // 30% max forward rate
            smoothness_threshold: 0.1, // Max curvature
            reprice_warning_threshold: 1e-4,
            forward_check_interval: 1.0 / 12.0, // Monthly
            max_maturity_check: 50.0,           // 50 years
        }
    }
}

impl CurveValidator {
    /// Creates a new validator with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the reprice tolerance.
    #[must_use]
    pub fn with_reprice_tolerance(mut self, tolerance: f64) -> Self {
        self.reprice_tolerance = tolerance;
        self
    }

    /// Sets the forward rate floor.
    #[must_use]
    pub fn with_forward_floor(mut self, floor: f64) -> Self {
        self.forward_floor = floor;
        self
    }

    /// Sets the forward rate ceiling.
    #[must_use]
    pub fn with_forward_ceiling(mut self, ceiling: f64) -> Self {
        self.forward_ceiling = ceiling;
        self
    }

    /// Sets the smoothness threshold.
    #[must_use]
    pub fn with_smoothness_threshold(mut self, threshold: f64) -> Self {
        self.smoothness_threshold = threshold;
        self
    }

    /// Creates a strict validator for production use.
    #[must_use]
    pub fn strict() -> Self {
        Self {
            reprice_tolerance: 1e-8,
            forward_floor: 0.0,
            forward_ceiling: 0.20,
            smoothness_threshold: 0.05,
            reprice_warning_threshold: 1e-6,
            forward_check_interval: 1.0 / 52.0, // Weekly
            max_maturity_check: 100.0,
        }
    }

    /// Creates a relaxed validator for testing.
    #[must_use]
    pub fn relaxed() -> Self {
        Self {
            reprice_tolerance: 1e-3,
            forward_floor: -0.05,
            forward_ceiling: 0.50,
            smoothness_threshold: 1.0,
            reprice_warning_threshold: 1e-2,
            forward_check_interval: 1.0 / 4.0, // Quarterly
            max_maturity_check: 30.0,
        }
    }

    /// Validates a curve against instruments.
    ///
    /// # Arguments
    ///
    /// * `curve` - The discount curve to validate
    /// * `instruments` - The instruments used to build the curve
    ///
    /// # Returns
    ///
    /// A `ValidationReport` containing errors, warnings, and metrics.
    pub fn validate(
        &self,
        curve: &DiscountCurve,
        instruments: &[Box<dyn CurveInstrument>],
    ) -> CurveResult<ValidationReport> {
        let mut report = ValidationReport::new();

        // Check 1: All instruments reprice to par
        self.check_repricing(curve, instruments, &mut report)?;

        // Check 2: Forward rates within bounds
        self.check_forward_rates(curve, &mut report)?;

        // Check 3: Discount factors monotonically decreasing
        self.check_monotonic_df(curve, &mut report)?;

        // Check 4: Smoothness (optional, may not apply to all interpolations)
        self.check_smoothness(curve, &mut report)?;

        Ok(report)
    }

    /// Checks that all instruments reprice to par.
    fn check_repricing(
        &self,
        curve: &DiscountCurve,
        instruments: &[Box<dyn CurveInstrument>],
        report: &mut ValidationReport,
    ) -> CurveResult<()> {
        let mut sum_sq = 0.0;
        let mut max_abs = 0.0;

        for inst in instruments {
            let pv = inst.pv(curve)?;
            let abs_pv = pv.abs();

            report.residuals.push((inst.description(), pv));

            if abs_pv > max_abs {
                max_abs = abs_pv;
            }
            sum_sq += pv * pv;

            if abs_pv > self.reprice_tolerance {
                report.add_error(ValidationError::RepriceFailed {
                    instrument: inst.description(),
                    pv,
                    tolerance: self.reprice_tolerance,
                });
            } else if abs_pv > self.reprice_warning_threshold {
                report.add_warning(ValidationWarning::RepriceImprecise {
                    instrument: inst.description(),
                    pv,
                });
            }
        }

        report.max_residual = max_abs;
        if !instruments.is_empty() {
            report.rms_residual = (sum_sq / instruments.len() as f64).sqrt();
        }

        Ok(())
    }

    /// Checks that forward rates are within bounds.
    fn check_forward_rates(
        &self,
        curve: &DiscountCurve,
        report: &mut ValidationReport,
    ) -> CurveResult<()> {
        let mut t = 0.0;
        let mut prev_fwd = None;
        let mut inversion_start = None;

        while t <= self.max_maturity_check {
            if let Ok(fwd) = curve.instantaneous_forward(t) {
                // Check floor
                if fwd < self.forward_floor {
                    report.add_error(ValidationError::NegativeForward { time: t, rate: fwd });
                }

                // Check ceiling
                if fwd > self.forward_ceiling {
                    report.add_error(ValidationError::ForwardTooHigh {
                        time: t,
                        rate: fwd,
                        max_rate: self.forward_ceiling,
                    });
                }

                // Check for inversion (warning only)
                if let Some(prev) = prev_fwd {
                    if fwd < prev - 0.001 && inversion_start.is_none() {
                        inversion_start = Some(t - self.forward_check_interval);
                    } else if fwd >= prev && inversion_start.is_some() {
                        report.add_warning(ValidationWarning::InvertedCurve {
                            start_time: inversion_start.unwrap(),
                            end_time: t,
                        });
                        inversion_start = None;
                    }
                }

                prev_fwd = Some(fwd);
            }

            t += self.forward_check_interval;
        }

        Ok(())
    }

    /// Checks that discount factors are monotonically decreasing.
    fn check_monotonic_df(
        &self,
        curve: &DiscountCurve,
        report: &mut ValidationReport,
    ) -> CurveResult<()> {
        let mut prev_df = 1.0;
        let mut t = self.forward_check_interval;

        while t <= self.max_maturity_check {
            if let Ok(df) = curve.discount_factor(t) {
                if df >= prev_df {
                    report.add_error(ValidationError::NonMonotonicDF {
                        time: t,
                        df,
                        prev_df,
                    });
                }
                prev_df = df;
            }

            t += self.forward_check_interval;
        }

        Ok(())
    }

    /// Checks curve smoothness using second derivative approximation.
    fn check_smoothness(
        &self,
        curve: &DiscountCurve,
        report: &mut ValidationReport,
    ) -> CurveResult<()> {
        let h = 0.01; // Step size for numerical differentiation
        let mut t = 2.0 * h;

        while t <= self.max_maturity_check - h {
            // Approximate second derivative of forward rate
            let fwd_minus = curve.instantaneous_forward(t - h).unwrap_or(0.0);
            let fwd_center = curve.instantaneous_forward(t).unwrap_or(0.0);
            let fwd_plus = curve.instantaneous_forward(t + h).unwrap_or(0.0);

            let curvature = (fwd_plus - 2.0 * fwd_center + fwd_minus) / (h * h);

            if curvature.abs() > self.smoothness_threshold {
                report.add_error(ValidationError::NotSmooth {
                    time: t,
                    curvature: curvature.abs(),
                    threshold: self.smoothness_threshold,
                });
            }

            t += self.forward_check_interval;
        }

        Ok(())
    }
}

/// Quick validation for common use cases.
pub fn quick_validate(curve: &DiscountCurve) -> CurveResult<bool> {
    let validator = CurveValidator::relaxed();

    // Just check basic properties, no instruments
    let mut report = ValidationReport::new();

    validator.check_forward_rates(curve, &mut report)?;
    validator.check_monotonic_df(curve, &mut report)?;

    Ok(report.is_valid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscountCurveBuilder;

    fn sample_curve() -> DiscountCurve {
        DiscountCurveBuilder::new(convex_core::Date::from_ymd(2025, 1, 1).unwrap())
            .add_pillar(0.0, 1.0)
            .add_pillar(0.25, 0.9875)
            .add_pillar(0.5, 0.975)
            .add_pillar(1.0, 0.95)
            .add_pillar(2.0, 0.90)
            .add_pillar(5.0, 0.80)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    #[test]
    fn test_validator_default() {
        let validator = CurveValidator::default();
        assert!((validator.reprice_tolerance - 1e-6).abs() < 1e-10);
    }

    #[test]
    fn test_validator_strict() {
        let validator = CurveValidator::strict();
        assert!(validator.reprice_tolerance < CurveValidator::default().reprice_tolerance);
    }

    #[test]
    fn test_validator_relaxed() {
        let validator = CurveValidator::relaxed();
        assert!(validator.reprice_tolerance > CurveValidator::default().reprice_tolerance);
    }

    #[test]
    fn test_validate_good_curve() {
        let curve = sample_curve();
        let instruments: Vec<Box<dyn CurveInstrument>> = vec![];

        let validator = CurveValidator::relaxed();
        let report = validator.validate(&curve, &instruments).unwrap();

        // Should pass basic checks (no repricing since no instruments)
        assert!(report.residuals.is_empty());
    }

    #[test]
    fn test_quick_validate() {
        let curve = sample_curve();
        let valid = quick_validate(&curve).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_validation_report_display() {
        let mut report = ValidationReport::new();
        report.add_error(ValidationError::NegativeForward {
            time: 1.5,
            rate: -0.001,
        });

        let display = format!("{}", report);
        assert!(display.contains("FAILED"));
        assert!(display.contains("Negative forward"));
    }

    #[test]
    fn test_monotonic_df_check() {
        // Create a curve with non-monotonic DFs
        let curve = DiscountCurveBuilder::new(convex_core::Date::from_ymd(2025, 1, 1).unwrap())
            .add_pillar(0.0, 1.0)
            .add_pillar(1.0, 0.95)
            .add_pillar(2.0, 0.90)
            .with_extrapolation()
            .build()
            .unwrap();

        let validator = CurveValidator::relaxed();
        let report = validator.validate(&curve, &[]).unwrap();

        // Should pass since curve is monotonic
        let df_errors: Vec<_> = report
            .errors
            .iter()
            .filter(|e| matches!(e, ValidationError::NonMonotonicDF { .. }))
            .collect();
        assert!(df_errors.is_empty());
    }
}
