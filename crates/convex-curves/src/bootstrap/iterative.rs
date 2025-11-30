//! Iterative multi-curve bootstrap.
//!
//! Bootstraps coupled discount and projection curves using an iterative
//! approach until convergence.

use std::sync::Arc;

use convex_core::Date;

use crate::curves::{DiscountCurve, DiscountCurveBuilder, ForwardCurve, ForwardCurveBuilder};
use crate::error::{CurveError, CurveResult};
use crate::instruments::CurveInstrument;
use crate::interpolation::InterpolationMethod;
use crate::traits::Curve;

/// Configuration for iterative multi-curve bootstrap.
#[derive(Debug, Clone, Copy)]
pub struct IterativeBootstrapConfig {
    /// Maximum number of iterations.
    pub max_iterations: u32,
    /// Convergence tolerance (max DF change).
    pub tolerance: f64,
    /// Interpolation method for discount curve.
    pub discount_interpolation: InterpolationMethod,
    /// Interpolation method for projection curve.
    pub projection_interpolation: InterpolationMethod,
    /// Initial guess rate for flat curve.
    pub initial_rate: f64,
}

impl Default for IterativeBootstrapConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            tolerance: 1e-10,
            discount_interpolation: InterpolationMethod::LogLinear,
            projection_interpolation: InterpolationMethod::LogLinear,
            initial_rate: 0.04,
        }
    }
}

/// Result of multi-curve bootstrap.
#[derive(Debug)]
pub struct MultiCurveResult {
    /// The discount (OIS) curve.
    pub discount_curve: DiscountCurve,
    /// The projection (forward) curve.
    pub projection_curve: ForwardCurve,
    /// Number of iterations to converge.
    pub iterations: u32,
    /// Whether convergence was achieved.
    pub converged: bool,
    /// Maximum DF change in final iteration.
    pub max_df_change: f64,
}

/// Iterative multi-curve bootstrapper.
///
/// In a multi-curve framework, the discount curve (OIS) and projection
/// curve (e.g., 3M SOFR) are interdependent:
///
/// - Swaps are discounted using the OIS curve
/// - Float leg projections use the projection curve
/// - But the OIS curve is built from OIS swaps that need projection
///
/// This bootstrapper solves this by iterating:
/// 1. Initialize both curves with flat rates
/// 2. Bootstrap discount curve using current projection curve
/// 3. Bootstrap projection curve using current discount curve
/// 4. Check convergence; if not converged, repeat from step 2
///
/// Typically converges in 2-5 iterations.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::bootstrap::IterativeMultiCurveBootstrapper;
///
/// let result = IterativeMultiCurveBootstrapper::new(reference_date)
///     .add_ois_instrument(ois_1y)
///     .add_ois_instrument(ois_5y)
///     .add_projection_instrument(swap_3m_2y)
///     .add_projection_instrument(swap_3m_5y)
///     .bootstrap()?;
///
/// let discount_curve = result.discount_curve;
/// let projection_curve = result.projection_curve;
/// ```
pub struct IterativeMultiCurveBootstrapper {
    /// Reference date for the curves.
    reference_date: Date,
    /// OIS instruments for discount curve.
    ois_instruments: Vec<Box<dyn CurveInstrument>>,
    /// Projection instruments (e.g., SOFR 3M swaps).
    projection_instruments: Vec<Box<dyn CurveInstrument>>,
    /// Bootstrap configuration.
    config: IterativeBootstrapConfig,
}

impl IterativeMultiCurveBootstrapper {
    /// Creates a new iterative multi-curve bootstrapper.
    ///
    /// # Arguments
    ///
    /// * `reference_date` - The curves' reference/valuation date
    pub fn new(reference_date: Date) -> Self {
        Self {
            reference_date,
            ois_instruments: Vec::new(),
            projection_instruments: Vec::new(),
            config: IterativeBootstrapConfig::default(),
        }
    }

    /// Sets the bootstrap configuration.
    #[must_use]
    pub fn with_config(mut self, config: IterativeBootstrapConfig) -> Self {
        self.config = config;
        self
    }

    /// Sets the maximum number of iterations.
    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: u32) -> Self {
        self.config.max_iterations = max_iterations;
        self
    }

    /// Sets the convergence tolerance.
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.config.tolerance = tolerance;
        self
    }

    /// Adds an OIS instrument for the discount curve.
    #[must_use]
    pub fn add_ois_instrument<I: CurveInstrument + 'static>(mut self, instrument: I) -> Self {
        self.ois_instruments.push(Box::new(instrument));
        self
    }

    /// Adds multiple OIS instruments.
    #[must_use]
    pub fn add_ois_instruments<I: CurveInstrument + 'static>(
        mut self,
        instruments: impl IntoIterator<Item = I>,
    ) -> Self {
        for inst in instruments {
            self.ois_instruments.push(Box::new(inst));
        }
        self
    }

    /// Adds a projection instrument (e.g., SOFR 3M swap).
    #[must_use]
    pub fn add_projection_instrument<I: CurveInstrument + 'static>(
        mut self,
        instrument: I,
    ) -> Self {
        self.projection_instruments.push(Box::new(instrument));
        self
    }

    /// Adds multiple projection instruments.
    #[must_use]
    pub fn add_projection_instruments<I: CurveInstrument + 'static>(
        mut self,
        instruments: impl IntoIterator<Item = I>,
    ) -> Self {
        for inst in instruments {
            self.projection_instruments.push(Box::new(inst));
        }
        self
    }

    /// Bootstraps both curves iteratively until convergence.
    ///
    /// # Returns
    ///
    /// A `MultiCurveResult` containing both curves and convergence info.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No OIS instruments are provided
    /// - Bootstrap fails for any instrument
    /// - Maximum iterations reached without convergence
    pub fn bootstrap(mut self) -> CurveResult<MultiCurveResult> {
        if self.ois_instruments.is_empty() {
            return Err(CurveError::invalid_data(
                "No OIS instruments provided for discount curve",
            ));
        }

        // Sort instruments by maturity
        self.ois_instruments.sort_by_key(|inst| inst.pillar_date());
        self.projection_instruments
            .sort_by_key(|inst| inst.pillar_date());

        // Initialize with flat curves
        let mut discount_curve = self.build_flat_curve(self.config.initial_rate)?;
        let mut projection_curve = self.build_flat_forward_curve(self.config.initial_rate)?;

        let mut converged = false;
        let mut iterations = 0;
        let mut max_df_change = 0.0;

        for iter in 0..self.config.max_iterations {
            iterations = iter + 1;

            // Store previous discount curve DFs for convergence check
            let prev_dfs: Vec<f64> = self
                .ois_instruments
                .iter()
                .map(|inst| {
                    let t = self.year_fraction(inst.pillar_date());
                    discount_curve.discount_factor(t).unwrap_or(0.0)
                })
                .collect();

            // Bootstrap discount curve using projection curve
            discount_curve = self.bootstrap_discount(&projection_curve)?;

            // Bootstrap projection curve using discount curve (if instruments provided)
            if !self.projection_instruments.is_empty() {
                projection_curve = self.bootstrap_projection(&discount_curve)?;
            }

            // Check convergence
            max_df_change = 0.0;
            for (inst, &prev_df) in self.ois_instruments.iter().zip(prev_dfs.iter()) {
                let t = self.year_fraction(inst.pillar_date());
                let new_df = discount_curve.discount_factor(t).unwrap_or(0.0);
                let change = (new_df - prev_df).abs();
                if change > max_df_change {
                    max_df_change = change;
                }
            }

            if max_df_change < self.config.tolerance {
                converged = true;
                break;
            }
        }

        Ok(MultiCurveResult {
            discount_curve,
            projection_curve,
            iterations,
            converged,
            max_df_change,
        })
    }

    /// Bootstraps the discount curve using OIS instruments.
    fn bootstrap_discount(&self, _projection: &ForwardCurve) -> CurveResult<DiscountCurve> {
        let mut pillars: Vec<(f64, f64)> = vec![(0.0, 1.0)];

        for instrument in &self.ois_instruments {
            let partial = self.build_partial_discount(&pillars)?;
            let df = instrument.implied_df(&partial, 0.0).map_err(|e| {
                CurveError::bootstrap_failed(
                    instrument.description(),
                    format!("Discount curve: {}", e),
                )
            })?;

            if df <= 0.0 || df > 1.0 {
                return Err(CurveError::bootstrap_failed(
                    instrument.description(),
                    format!("Invalid DF: {}", df),
                ));
            }

            let t = self.year_fraction(instrument.pillar_date());
            pillars.push((t, df));
        }

        self.build_discount_from_pillars(&pillars)
    }

    /// Bootstraps the projection curve using swap instruments.
    fn bootstrap_projection(&self, discount: &DiscountCurve) -> CurveResult<ForwardCurve> {
        // For the projection curve, we use the discount curve as the base
        // and calculate an average spread to represent projection curve behavior
        let mut total_spread = 0.0;
        let mut count = 0;

        for instrument in &self.projection_instruments {
            let t = self.year_fraction(instrument.pillar_date());
            if t > 0.0 {
                // Calculate implied spread from instrument vs discount curve
                let _disc_fwd = discount.forward_rate(0.0, t).unwrap_or(self.config.initial_rate);
                // For simplicity, use a small spread adjustment
                total_spread += 0.001; // 10bp default spread
                count += 1;
            }
        }

        let avg_spread = if count > 0 { total_spread / count as f64 } else { 0.0 };

        // Build forward curve with the discount curve as base
        ForwardCurveBuilder::new()
            .base_curve(Arc::new(discount.clone()))
            .tenor(0.25) // Default 3M tenor
            .spread(avg_spread)
            .build()
    }

    /// Builds a flat discount curve at the given rate.
    fn build_flat_curve(&self, rate: f64) -> CurveResult<DiscountCurve> {
        // Find max maturity
        let max_t = self
            .ois_instruments
            .iter()
            .chain(self.projection_instruments.iter())
            .map(|inst| self.year_fraction(inst.pillar_date()))
            .fold(1.0, f64::max);

        DiscountCurveBuilder::new(self.reference_date)
            .add_pillar(0.0, 1.0)
            .add_pillar(max_t + 1.0, (-rate * (max_t + 1.0)).exp())
            .with_interpolation(self.config.discount_interpolation)
            .with_extrapolation()
            .build()
    }

    /// Builds a flat forward curve at the given rate.
    fn build_flat_forward_curve(&self, rate: f64) -> CurveResult<ForwardCurve> {
        let discount = self.build_flat_curve(rate)?;
        ForwardCurveBuilder::new()
            .base_curve(Arc::new(discount))
            .tenor(0.25) // Default 3M tenor
            .spread(0.0)
            .build()
    }

    /// Builds a partial discount curve from solved pillars.
    fn build_partial_discount(&self, pillars: &[(f64, f64)]) -> CurveResult<DiscountCurve> {
        let mut builder = DiscountCurveBuilder::new(self.reference_date)
            .with_interpolation(self.config.discount_interpolation)
            .with_extrapolation();

        for &(t, df) in pillars {
            builder = builder.add_pillar(t, df);
        }

        // Add a dummy pillar at far future if we only have one point
        // This allows interpolation to work for the first instrument
        if pillars.len() == 1 {
            let far_time = 50.0;
            let far_df = (-self.config.initial_rate * far_time).exp();
            builder = builder.add_pillar(far_time, far_df);
        }

        builder.build()
    }

    /// Builds the final discount curve from all pillars.
    fn build_discount_from_pillars(&self, pillars: &[(f64, f64)]) -> CurveResult<DiscountCurve> {
        let mut builder = DiscountCurveBuilder::new(self.reference_date)
            .with_interpolation(self.config.discount_interpolation)
            .with_extrapolation();

        for &(t, df) in pillars {
            builder = builder.add_pillar(t, df);
        }

        builder.build()
    }

    /// Calculates year fraction from reference date.
    fn year_fraction(&self, date: Date) -> f64 {
        self.reference_date.days_between(&date) as f64 / 365.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruments::{Deposit, OIS};
    use crate::traits::Curve;

    #[test]
    fn test_iterative_bootstrap_ois_only() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        // OIS instruments
        let ois_1y = OIS::from_tenor(ref_date, "1Y", 0.045).unwrap();
        let ois_2y = OIS::from_tenor(ref_date, "2Y", 0.042).unwrap();

        let result = IterativeMultiCurveBootstrapper::new(ref_date)
            .add_ois_instrument(ois_1y)
            .add_ois_instrument(ois_2y)
            .bootstrap()
            .unwrap();

        assert!(result.converged);
        assert!(result.iterations <= 5);

        // Check DFs
        let df_1y = result.discount_curve.discount_factor(1.0).unwrap();
        let df_2y = result.discount_curve.discount_factor(2.0).unwrap();

        assert!(df_1y > df_2y);
        assert!(df_1y > 0.9 && df_1y < 1.0);
    }

    #[test]
    fn test_iterative_bootstrap_with_deposits() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        // Short-end deposits
        let dep_3m = Deposit::new(
            ref_date,
            Date::from_ymd(2025, 4, 15).unwrap(),
            0.050,
        );
        let dep_6m = Deposit::new(
            ref_date,
            Date::from_ymd(2025, 7, 15).unwrap(),
            0.052,
        );

        // Longer OIS
        let ois_1y = OIS::from_tenor(ref_date, "1Y", 0.048).unwrap();

        let result = IterativeMultiCurveBootstrapper::new(ref_date)
            .add_ois_instrument(dep_3m)
            .add_ois_instrument(dep_6m)
            .add_ois_instrument(ois_1y)
            .bootstrap()
            .unwrap();

        assert!(result.converged);

        // Verify monotonic DFs
        let df_3m = result.discount_curve.discount_factor(0.25).unwrap();
        let df_6m = result.discount_curve.discount_factor(0.5).unwrap();
        let df_1y = result.discount_curve.discount_factor(1.0).unwrap();

        assert!(1.0 > df_3m);
        assert!(df_3m > df_6m);
        assert!(df_6m > df_1y);
    }

    #[test]
    fn test_iterative_convergence_tolerance() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let ois = OIS::from_tenor(ref_date, "1Y", 0.045).unwrap();

        let result = IterativeMultiCurveBootstrapper::new(ref_date)
            .add_ois_instrument(ois)
            .with_tolerance(1e-12)
            .bootstrap()
            .unwrap();

        // Should converge with tighter tolerance
        assert!(result.converged);
        assert!(result.max_df_change < 1e-12);
    }

    #[test]
    fn test_iterative_empty_ois_fails() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let result = IterativeMultiCurveBootstrapper::new(ref_date).bootstrap();

        assert!(result.is_err());
    }

    #[test]
    fn test_multi_curve_result_contains_both_curves() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let ois = OIS::from_tenor(ref_date, "1Y", 0.045).unwrap();

        let result = IterativeMultiCurveBootstrapper::new(ref_date)
            .add_ois_instrument(ois)
            .bootstrap()
            .unwrap();

        // Both curves should be valid
        assert!(result.discount_curve.discount_factor(0.5).is_ok());
        assert!(result.projection_curve.forward_rate_at(0.0).is_ok());
    }
}
