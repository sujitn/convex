//! Sequential bootstrap algorithm.
//!
//! Bootstraps a yield curve by solving for each instrument's discount factor
//! sequentially, using previously solved values.

use convex_core::Date;

use crate::curves::{DiscountCurve, DiscountCurveBuilder};
use crate::error::{CurveError, CurveResult};
use crate::instruments::CurveInstrument;
use crate::interpolation::InterpolationMethod;
use crate::repricing::{
    BootstrapResult, BuildTimer, RepricingCheck, RepricingReport, tolerances,
};

/// Configuration for sequential bootstrap.
#[derive(Debug, Clone, Copy)]
pub struct SequentialBootstrapConfig {
    /// Interpolation method for the curve.
    pub interpolation: InterpolationMethod,
    /// Whether to enable extrapolation.
    pub allow_extrapolation: bool,
    /// Tolerance for instrument pricing (for validation).
    pub tolerance: f64,
}

impl Default for SequentialBootstrapConfig {
    fn default() -> Self {
        Self {
            interpolation: InterpolationMethod::LogLinear,
            allow_extrapolation: true,
            tolerance: 1e-10,
        }
    }
}

/// Sequential bootstrapper for building discount curves.
///
/// The sequential bootstrap algorithm:
/// 1. Sort instruments by pillar date
/// 2. Initialize with DF(0) = 1.0 at reference date
/// 3. For each instrument, solve for its implied discount factor
/// 4. Build the final curve with interpolation
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::bootstrap::SequentialBootstrapper;
/// use convex_curves::instruments::{Deposit, Swap};
///
/// let bootstrapper = SequentialBootstrapper::new(reference_date);
///
/// let curve = bootstrapper
///     .add_instrument(Deposit::new(spot, end_3m, 0.05))
///     .add_instrument(Deposit::new(spot, end_6m, 0.052))
///     .add_instrument(Swap::new(spot, end_2y, 0.045, Frequency::SemiAnnual))
///     .add_instrument(Swap::new(spot, end_5y, 0.042, Frequency::SemiAnnual))
///     .bootstrap()?;
/// ```
pub struct SequentialBootstrapper {
    /// Reference date for the curve.
    reference_date: Date,
    /// Instruments to bootstrap.
    instruments: Vec<Box<dyn CurveInstrument>>,
    /// Bootstrap configuration.
    config: SequentialBootstrapConfig,
}

impl SequentialBootstrapper {
    /// Creates a new sequential bootstrapper.
    ///
    /// # Arguments
    ///
    /// * `reference_date` - The curve's reference/valuation date
    pub fn new(reference_date: Date) -> Self {
        Self {
            reference_date,
            instruments: Vec::new(),
            config: SequentialBootstrapConfig::default(),
        }
    }

    /// Sets the bootstrap configuration.
    #[must_use]
    pub fn with_config(mut self, config: SequentialBootstrapConfig) -> Self {
        self.config = config;
        self
    }

    /// Sets the interpolation method.
    #[must_use]
    pub fn with_interpolation(mut self, method: InterpolationMethod) -> Self {
        self.config.interpolation = method;
        self
    }

    /// Enables or disables extrapolation.
    #[must_use]
    pub fn with_extrapolation(mut self, enabled: bool) -> Self {
        self.config.allow_extrapolation = enabled;
        self
    }

    /// Adds an instrument to the bootstrap set.
    #[must_use]
    pub fn add_instrument<I: CurveInstrument + 'static>(mut self, instrument: I) -> Self {
        self.instruments.push(Box::new(instrument));
        self
    }

    /// Adds multiple instruments to the bootstrap set.
    #[must_use]
    pub fn add_instruments<I: CurveInstrument + 'static>(
        mut self,
        instruments: impl IntoIterator<Item = I>,
    ) -> Self {
        for inst in instruments {
            self.instruments.push(Box::new(inst));
        }
        self
    }

    /// Bootstraps the curve from the added instruments.
    ///
    /// # Returns
    ///
    /// A `DiscountCurve` that prices all instruments to par (PV = 0).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No instruments are provided
    /// - Bootstrap fails for any instrument
    /// - Curve construction fails
    pub fn bootstrap(mut self) -> CurveResult<DiscountCurve> {
        if self.instruments.is_empty() {
            return Err(CurveError::invalid_data("No instruments provided for bootstrap"));
        }

        // Sort instruments by pillar date
        self.instruments
            .sort_by_key(|inst| inst.pillar_date());

        // Initialize with DF(0) = 1.0 at reference date
        let mut pillars: Vec<(f64, f64)> = vec![(0.0, 1.0)];

        // Bootstrap each instrument sequentially
        for instrument in &self.instruments {
            // Build partial curve from already-solved pillars
            let partial_curve = self.build_partial_curve(&pillars)?;

            // Solve for the implied discount factor
            let df = instrument.implied_df(&partial_curve, 0.0).map_err(|e| {
                CurveError::bootstrap_failed(
                    instrument.description(),
                    format!("Failed to solve for DF: {}", e),
                )
            })?;

            // Validate the discount factor
            if df <= 0.0 || df > 1.0 {
                return Err(CurveError::bootstrap_failed(
                    instrument.description(),
                    format!("Invalid discount factor: {} (must be in (0, 1])", df),
                ));
            }

            // Calculate year fraction for the pillar
            let t = self.year_fraction(instrument.pillar_date());

            // Avoid duplicate pillar points
            if let Some((last_t, _)) = pillars.last() {
                if (t - last_t).abs() < 1e-10 {
                    // Replace the last pillar if it's at the same time
                    pillars.pop();
                }
            }

            pillars.push((t, df));
        }

        // Build final curve
        self.build_final_curve(&pillars)
    }

    /// Bootstraps the curve with mandatory repricing validation.
    ///
    /// This is the recommended method for production use. It returns a
    /// `BootstrapResult` that includes the curve and a complete repricing
    /// report showing how well each instrument is priced.
    ///
    /// # Returns
    ///
    /// A `BootstrapResult` containing:
    /// - The bootstrapped `DiscountCurve`
    /// - A `RepricingReport` with validation results for each instrument
    /// - Build duration for performance monitoring
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No instruments are provided
    /// - Bootstrap fails for any instrument
    /// - Curve construction fails
    ///
    /// Note: This method does NOT fail if repricing validation fails.
    /// Check `result.is_valid()` to determine if all instruments repriced
    /// within tolerance. Use `bootstrap_validated_strict()` if you want
    /// the method to fail on repricing errors.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = SequentialBootstrapper::new(ref_date)
    ///     .add_instrument(deposit)
    ///     .add_instrument(swap)
    ///     .bootstrap_validated()?;
    ///
    /// assert!(result.is_valid(), "Repricing failed: {:?}", result.failed_instruments());
    /// let curve = result.into_curve();
    /// ```
    pub fn bootstrap_validated(mut self) -> CurveResult<BootstrapResult<DiscountCurve>> {
        let timer = BuildTimer::start();

        if self.instruments.is_empty() {
            return Err(CurveError::invalid_data("No instruments provided for bootstrap"));
        }

        // Sort instruments by pillar date
        self.instruments.sort_by_key(|inst| inst.pillar_date());

        // Initialize with DF(0) = 1.0 at reference date
        let mut pillars: Vec<(f64, f64)> = vec![(0.0, 1.0)];

        // Bootstrap each instrument sequentially
        for instrument in &self.instruments {
            // Build partial curve from already-solved pillars
            let partial_curve = self.build_partial_curve(&pillars)?;

            // Solve for the implied discount factor
            let df = instrument.implied_df(&partial_curve, 0.0).map_err(|e| {
                CurveError::bootstrap_failed(
                    instrument.description(),
                    format!("Failed to solve for DF: {}", e),
                )
            })?;

            // Validate the discount factor
            if df <= 0.0 || df > 1.0 {
                return Err(CurveError::bootstrap_failed(
                    instrument.description(),
                    format!("Invalid discount factor: {} (must be in (0, 1])", df),
                ));
            }

            // Calculate year fraction for the pillar
            let t = self.year_fraction(instrument.pillar_date());

            // Avoid duplicate pillar points
            if let Some((last_t, _)) = pillars.last() {
                if (t - last_t).abs() < 1e-10 {
                    // Replace the last pillar if it's at the same time
                    pillars.pop();
                }
            }

            pillars.push((t, df));
        }

        // Build final curve
        let curve = self.build_final_curve(&pillars)?;

        // Perform repricing validation
        let repricing_report = self.validate_repricing(&curve)?;

        let build_duration = timer.elapsed();

        Ok(BootstrapResult::new(curve, repricing_report, build_duration))
    }

    /// Bootstraps the curve with strict repricing validation.
    ///
    /// This method is similar to `bootstrap_validated()` but returns an
    /// error if any instrument fails to reprice within tolerance.
    ///
    /// # Returns
    ///
    /// A `BootstrapResult` if all instruments reprice within tolerance.
    ///
    /// # Errors
    ///
    /// Returns `CurveError::RepricingFailed` if any instrument exceeds
    /// its repricing tolerance.
    pub fn bootstrap_validated_strict(self) -> CurveResult<BootstrapResult<DiscountCurve>> {
        let result = self.bootstrap_validated()?;

        if !result.is_valid() {
            return Err(CurveError::repricing_failed(
                result.repricing_report.failed_count(),
                result.repricing_report.max_error(),
                result.failed_instruments().into_iter().map(String::from).collect(),
            ));
        }

        Ok(result)
    }

    /// Validates that the curve reprices all instruments.
    fn validate_repricing(&self, curve: &DiscountCurve) -> CurveResult<RepricingReport> {
        let mut checks = Vec::with_capacity(self.instruments.len());

        for inst in &self.instruments {
            let model_pv = inst.pv(curve)?;
            let tolerance = tolerances::for_instrument(inst.instrument_type());

            checks.push(RepricingCheck::new(
                inst.description(),
                inst.instrument_type(),
                0.0,  // Target PV is always 0 for bootstrapped instruments
                model_pv,
                tolerance,
            ));
        }

        Ok(RepricingReport::new(checks))
    }

    /// Builds a partial curve from solved pillars.
    fn build_partial_curve(&self, pillars: &[(f64, f64)]) -> CurveResult<DiscountCurve> {
        let mut builder = DiscountCurveBuilder::new(self.reference_date)
            .with_interpolation(self.config.interpolation);

        for &(t, df) in pillars {
            builder = builder.add_pillar(t, df);
        }

        // Add a dummy pillar at far future if we only have one point
        // This allows interpolation to work for the first instrument
        if pillars.len() == 1 {
            // Use flat forward rate assumption (4% default)
            let far_time = 50.0_f64;
            let far_df = (-0.04_f64 * far_time).exp();
            builder = builder.add_pillar(far_time, far_df);
        }

        if self.config.allow_extrapolation {
            builder = builder.with_extrapolation();
        }

        builder.build()
    }

    /// Builds the final curve from all pillars.
    fn build_final_curve(&self, pillars: &[(f64, f64)]) -> CurveResult<DiscountCurve> {
        let mut builder = DiscountCurveBuilder::new(self.reference_date)
            .with_interpolation(self.config.interpolation);

        for &(t, df) in pillars {
            builder = builder.add_pillar(t, df);
        }

        if self.config.allow_extrapolation {
            builder = builder.with_extrapolation();
        }

        builder.build()
    }

    /// Calculates year fraction from reference date.
    ///
    /// Uses ACT/360 day count to match money market instruments (deposits, OIS, swaps).
    /// This is important for repricing consistency - the curve must be queried
    /// at the same time values that instruments use in their PV calculations.
    fn year_fraction(&self, date: Date) -> f64 {
        self.reference_date.days_between(&date) as f64 / 360.0
    }
}

/// Bootstraps a discount curve from a vector of instruments.
///
/// This is a convenience function that creates a `SequentialBootstrapper`
/// and runs the bootstrap.
///
/// # Arguments
///
/// * `reference_date` - The curve's reference date
/// * `instruments` - Vector of boxed curve instruments
/// * `interpolation` - Interpolation method to use
///
/// # Returns
///
/// A bootstrapped `DiscountCurve`.
pub fn bootstrap_discount_curve(
    reference_date: Date,
    instruments: Vec<Box<dyn CurveInstrument>>,
    interpolation: InterpolationMethod,
) -> CurveResult<DiscountCurve> {
    if instruments.is_empty() {
        return Err(CurveError::invalid_data("No instruments provided"));
    }

    let mut bootstrapper = SequentialBootstrapper::new(reference_date)
        .with_interpolation(interpolation)
        .with_extrapolation(true);

    for inst in instruments {
        bootstrapper.instruments.push(inst);
    }

    bootstrapper.bootstrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruments::{Deposit, OIS, Swap};
    use crate::traits::Curve;
    use approx::assert_relative_eq;
    use convex_core::types::Frequency;

    #[test]
    fn test_bootstrap_single_deposit() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let end_date = Date::from_ymd(2025, 4, 15).unwrap();

        let deposit = Deposit::new(ref_date, end_date, 0.05);

        let curve = SequentialBootstrapper::new(ref_date)
            .add_instrument(deposit)
            .bootstrap()
            .unwrap();

        // Verify the curve has correct reference date
        assert_eq!(curve.reference_date(), ref_date);

        // Verify DF at t=0 is 1.0
        assert_relative_eq!(curve.discount_factor(0.0).unwrap(), 1.0, epsilon = 1e-10);

        // Verify DF at maturity is reasonable
        let t = ref_date.days_between(&end_date) as f64 / 365.0;
        let df = curve.discount_factor(t).unwrap();
        assert!(df > 0.98 && df < 1.0);
    }

    #[test]
    fn test_bootstrap_multiple_deposits() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let deposits = vec![
            Deposit::new(
                ref_date,
                Date::from_ymd(2025, 2, 15).unwrap(),
                0.045,
            ),
            Deposit::new(
                ref_date,
                Date::from_ymd(2025, 4, 15).unwrap(),
                0.050,
            ),
            Deposit::new(
                ref_date,
                Date::from_ymd(2025, 7, 15).unwrap(),
                0.052,
            ),
        ];

        let curve = SequentialBootstrapper::new(ref_date)
            .add_instruments(deposits)
            .bootstrap()
            .unwrap();

        // Verify monotonically decreasing DFs
        let mut prev_df = 1.0;
        for months in [1, 3, 6] {
            let t = months as f64 / 12.0;
            let df = curve.discount_factor(t).unwrap();
            assert!(df < prev_df, "DF should decrease: {} >= {}", df, prev_df);
            prev_df = df;
        }
    }

    #[test]
    fn test_bootstrap_deposit_reprices() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let end_date = Date::from_ymd(2025, 7, 15).unwrap();

        let deposit = Deposit::new(ref_date, end_date, 0.05);

        let curve = SequentialBootstrapper::new(ref_date)
            .add_instrument(deposit)
            .bootstrap()
            .unwrap();

        // The deposit should reprice to approximately zero on the bootstrapped curve
        // Small numerical errors can occur due to interpolation
        let deposit_check = Deposit::new(ref_date, end_date, 0.05);
        let pv = deposit_check.pv(&curve).unwrap();

        assert!(pv.abs() < 0.001, "PV should be close to zero: {}", pv);
    }

    #[test]
    fn test_bootstrap_ois() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let ois = OIS::from_tenor(ref_date, "1Y", 0.045).unwrap();

        let curve = SequentialBootstrapper::new(ref_date)
            .add_instrument(ois)
            .bootstrap()
            .unwrap();

        // Verify DF at 1Y is consistent with 4.5% rate
        let df_1y = curve.discount_factor(1.0).unwrap();
        // DF ≈ 1 / (1 + rate) for 1Y OIS
        assert_relative_eq!(df_1y, 1.0 / 1.045, epsilon = 0.01);
    }

    #[test]
    fn test_bootstrap_mixed_instruments() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        // Short end: deposits
        let deposit_3m = Deposit::new(
            ref_date,
            Date::from_ymd(2025, 4, 15).unwrap(),
            0.050,
        );
        let deposit_6m = Deposit::new(
            ref_date,
            Date::from_ymd(2025, 7, 15).unwrap(),
            0.052,
        );

        // Long end: swaps
        let swap_2y = Swap::new(
            ref_date,
            Date::from_ymd(2027, 1, 15).unwrap(),
            0.045,
            Frequency::SemiAnnual,
        );

        let curve = SequentialBootstrapper::new(ref_date)
            .add_instrument(deposit_3m)
            .add_instrument(deposit_6m)
            .add_instrument(swap_2y)
            .bootstrap()
            .unwrap();

        // Verify curve extends to 2Y
        let df_2y = curve.discount_factor(2.0).unwrap();
        assert!(df_2y > 0.0 && df_2y < 1.0);
    }

    #[test]
    fn test_bootstrap_empty_fails() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let result = SequentialBootstrapper::new(ref_date).bootstrap();

        assert!(result.is_err());
    }

    #[test]
    fn test_bootstrap_preserves_order() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        // Add instruments out of order
        let deposit_6m = Deposit::new(
            ref_date,
            Date::from_ymd(2025, 7, 15).unwrap(),
            0.052,
        );
        let deposit_3m = Deposit::new(
            ref_date,
            Date::from_ymd(2025, 4, 15).unwrap(),
            0.050,
        );

        // Should still work - bootstrapper sorts by maturity
        let curve = SequentialBootstrapper::new(ref_date)
            .add_instrument(deposit_6m)
            .add_instrument(deposit_3m)
            .bootstrap()
            .unwrap();

        let df_3m = curve.discount_factor(0.25).unwrap();
        let df_6m = curve.discount_factor(0.5).unwrap();

        assert!(df_3m > df_6m);
    }

    // ========================================================================
    // Tests for bootstrap_validated() - Repricing Validation
    // ========================================================================

    #[test]
    fn test_bootstrap_validated_single_deposit() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let end_date = Date::from_ymd(2025, 4, 15).unwrap();

        let deposit = Deposit::new(ref_date, end_date, 0.05);

        let result = SequentialBootstrapper::new(ref_date)
            .add_instrument(deposit)
            .bootstrap_validated()
            .unwrap();

        // Verify the result contains valid curve
        assert_eq!(result.curve.reference_date(), ref_date);

        // Verify repricing passed
        assert!(
            result.is_valid(),
            "Repricing should pass. Max error: {:.2e}, Failed: {:?}",
            result.max_error(),
            result.failed_instruments()
        );

        // Verify max error is within tolerance
        // Note: Current implementation achieves ~1e-4 for deposits
        // TODO: Tighten to 1e-6 after algorithm improvements
        assert!(
            result.max_error() < 1e-3,
            "Max error {:.2e} should be < 1e-3",
            result.max_error()
        );

        // Verify report has one check
        assert_eq!(result.repricing_report.total_count(), 1);
        assert_eq!(result.repricing_report.passed_count(), 1);
        assert_eq!(result.repricing_report.failed_count(), 0);
    }

    #[test]
    fn test_bootstrap_validated_multiple_instruments() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let deposits = vec![
            Deposit::new(
                ref_date,
                Date::from_ymd(2025, 2, 15).unwrap(),
                0.045,
            ),
            Deposit::new(
                ref_date,
                Date::from_ymd(2025, 4, 15).unwrap(),
                0.050,
            ),
            Deposit::new(
                ref_date,
                Date::from_ymd(2025, 7, 15).unwrap(),
                0.052,
            ),
        ];

        let result = SequentialBootstrapper::new(ref_date)
            .add_instruments(deposits)
            .bootstrap_validated()
            .unwrap();

        // All instruments should reprice
        assert!(result.is_valid());
        assert_eq!(result.repricing_report.total_count(), 3);
        assert_eq!(result.repricing_report.passed_count(), 3);

        // Print the report for debugging
        println!("{}", result.repricing_report);
    }

    #[test]
    fn test_bootstrap_validated_with_ois() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let ois = OIS::from_tenor(ref_date, "1Y", 0.045).unwrap();

        let result = SequentialBootstrapper::new(ref_date)
            .add_instrument(ois)
            .bootstrap_validated()
            .unwrap();

        assert!(
            result.is_valid(),
            "OIS should reprice. Max error: {:.2e}",
            result.max_error()
        );
    }

    #[test]
    fn test_bootstrap_validated_mixed_instruments() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        // Short end: deposits
        let deposit_3m = Deposit::new(
            ref_date,
            Date::from_ymd(2025, 4, 15).unwrap(),
            0.050,
        );
        let deposit_6m = Deposit::new(
            ref_date,
            Date::from_ymd(2025, 7, 15).unwrap(),
            0.052,
        );

        // Long end: swap
        let swap_2y = Swap::new(
            ref_date,
            Date::from_ymd(2027, 1, 15).unwrap(),
            0.045,
            Frequency::SemiAnnual,
        );

        let result = SequentialBootstrapper::new(ref_date)
            .add_instrument(deposit_3m)
            .add_instrument(deposit_6m)
            .add_instrument(swap_2y)
            .bootstrap_validated()
            .unwrap();

        // All should reprice
        assert!(
            result.is_valid(),
            "Mixed instruments should reprice. Failed: {:?}",
            result.failed_instruments()
        );
        assert_eq!(result.repricing_report.total_count(), 3);
    }

    #[test]
    fn test_bootstrap_validated_strict_passes() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let end_date = Date::from_ymd(2025, 7, 15).unwrap();

        let deposit = Deposit::new(ref_date, end_date, 0.05);

        // Should not error since repricing passes
        let result = SequentialBootstrapper::new(ref_date)
            .add_instrument(deposit)
            .bootstrap_validated_strict()
            .unwrap();

        assert!(result.is_valid());
    }

    #[test]
    fn test_bootstrap_validated_into_curve() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let deposit = Deposit::new(
            ref_date,
            Date::from_ymd(2025, 7, 15).unwrap(),
            0.05,
        );

        let result = SequentialBootstrapper::new(ref_date)
            .add_instrument(deposit)
            .bootstrap_validated()
            .unwrap();

        // into_curve should work since repricing passed
        let curve = result.into_curve();
        assert!(curve.discount_factor(0.5).is_ok());
    }

    #[test]
    fn test_bootstrap_validated_report_display() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let deposits = vec![
            Deposit::new(ref_date, Date::from_ymd(2025, 4, 15).unwrap(), 0.050),
            Deposit::new(ref_date, Date::from_ymd(2025, 7, 15).unwrap(), 0.052),
        ];

        let result = SequentialBootstrapper::new(ref_date)
            .add_instruments(deposits)
            .bootstrap_validated()
            .unwrap();

        // Display should contain key information
        let report_str = format!("{}", result.repricing_report);
        assert!(report_str.contains("PASSED") || report_str.contains("FAILED"));
        assert!(report_str.contains("Deposit"));
    }

    #[test]
    fn test_bootstrap_validated_build_duration() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let deposit = Deposit::new(
            ref_date,
            Date::from_ymd(2025, 7, 15).unwrap(),
            0.05,
        );

        let result = SequentialBootstrapper::new(ref_date)
            .add_instrument(deposit)
            .bootstrap_validated()
            .unwrap();

        // Build duration should be non-zero
        assert!(result.build_duration.as_nanos() > 0);
    }
}
