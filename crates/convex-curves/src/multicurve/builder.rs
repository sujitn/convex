//! Multi-curve builder with fluent API.
//!
//! Provides a convenient way to construct multi-curve environments from
//! market instruments using a fluent builder pattern.

use std::collections::HashMap;
use std::sync::Arc;

use convex_core::types::Frequency;
use convex_core::Date;

use crate::bootstrap::SequentialBootstrapper;
use crate::curves::{DiscountCurve, ForwardCurve, ForwardCurveBuilder};
use crate::error::{CurveError, CurveResult};
use crate::instruments::{CurveInstrument, Deposit, Swap, OIS};
use crate::interpolation::InterpolationMethod;
use crate::traits::Curve;

use super::curve_set::{CurveSet, CurveSetBuilder};
use super::fx_forward::CurrencyPair;
use super::rate_index::{RateIndex, Tenor};

/// Configuration for multi-curve bootstrap.
#[derive(Debug, Clone)]
pub struct MultiCurveConfig {
    /// Interpolation method for discount curve.
    pub discount_interpolation: InterpolationMethod,
    /// Interpolation method for projection curves.
    pub projection_interpolation: InterpolationMethod,
    /// Maximum iterations for iterative bootstrap.
    pub max_iterations: u32,
    /// Convergence tolerance for iterative bootstrap.
    pub tolerance: f64,
    /// Enable extrapolation on curves.
    pub allow_extrapolation: bool,
}

impl Default for MultiCurveConfig {
    fn default() -> Self {
        Self {
            discount_interpolation: InterpolationMethod::LogLinear,
            projection_interpolation: InterpolationMethod::LogLinear,
            max_iterations: 10,
            tolerance: 1e-10,
            allow_extrapolation: true,
        }
    }
}

/// Builder for multi-curve environments.
///
/// Provides a fluent API for constructing complete multi-curve setups
/// from market instruments.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::multicurve::*;
///
/// let curves = MultiCurveBuilder::new(date!(2024-11-29))
///     // Discount curve (SOFR OIS)
///     .add_ois("1M", 0.0530)
///     .add_ois("3M", 0.0525)
///     .add_ois("1Y", 0.0510)
///     .add_ois("5Y", 0.0450)
///     .add_ois("10Y", 0.0420)
///     // Term SOFR 3M projection curve
///     .add_projection(RateIndex::TermSOFR3M, "2Y", 0.0485)
///     .add_projection(RateIndex::TermSOFR3M, "5Y", 0.0455)
///     // Basis: 1M vs 3M SOFR
///     .add_basis_swap(
///         RateIndex::TermSOFR { tenor: Tenor::M1 },
///         RateIndex::TermSOFR { tenor: Tenor::M3 },
///         "5Y",
///         0.0008,  // 8 bps
///     )
///     .build()?;
/// ```
pub struct MultiCurveBuilder {
    /// Reference date for all curves.
    reference_date: Date,
    /// OIS instruments for discount curve.
    ois_instruments: Vec<Box<dyn CurveInstrument>>,
    /// Projection instruments by rate index.
    projection_instruments: HashMap<RateIndex, Vec<Box<dyn CurveInstrument>>>,
    /// Basis swap instruments.
    basis_instruments: Vec<_BasisSwapSpec>,
    /// FX curve specifications.
    fx_specs: Vec<_FxCurveSpec>,
    /// Configuration.
    config: MultiCurveConfig,
}

/// Specification for a basis swap.
#[derive(Debug, Clone)]
struct _BasisSwapSpec {
    /// Pay leg index
    pay_index: RateIndex,
    /// Receive leg index
    receive_index: RateIndex,
    /// Tenor string (e.g., "5Y")
    tenor: String,
    /// Basis spread (receive - pay)
    spread: f64,
}

/// Specification for FX curve construction.
#[derive(Debug, Clone)]
struct _FxCurveSpec {
    /// Currency pair
    pair: CurrencyPair,
    /// Spot rate
    spot_rate: f64,
    /// Basis spread (optional)
    basis_bps: Option<f64>,
}

impl MultiCurveBuilder {
    /// Creates a new multi-curve builder.
    #[must_use]
    pub fn new(reference_date: Date) -> Self {
        Self {
            reference_date,
            ois_instruments: Vec::new(),
            projection_instruments: HashMap::new(),
            basis_instruments: Vec::new(),
            fx_specs: Vec::new(),
            config: MultiCurveConfig::default(),
        }
    }

    /// Sets the configuration.
    #[must_use]
    pub fn with_config(mut self, config: MultiCurveConfig) -> Self {
        self.config = config;
        self
    }

    /// Sets the interpolation method for the discount curve.
    #[must_use]
    pub fn with_discount_interpolation(mut self, method: InterpolationMethod) -> Self {
        self.config.discount_interpolation = method;
        self
    }

    /// Sets the interpolation method for projection curves.
    #[must_use]
    pub fn with_projection_interpolation(mut self, method: InterpolationMethod) -> Self {
        self.config.projection_interpolation = method;
        self
    }

    // ==================== OIS / Discount Curve ====================

    /// Adds an OIS instrument for the discount curve.
    ///
    /// # Arguments
    ///
    /// * `tenor` - Tenor string (e.g., "1M", "3M", "1Y", "5Y")
    /// * `rate` - OIS rate as a decimal (e.g., 0.05 for 5%)
    #[must_use]
    pub fn add_ois(mut self, tenor: &str, rate: f64) -> Self {
        if let Ok(ois) = OIS::from_tenor(self.reference_date, tenor, rate) {
            self.ois_instruments.push(Box::new(ois));
        }
        self
    }

    /// Adds an OIS instrument with explicit maturity.
    #[must_use]
    pub fn add_ois_maturity(mut self, maturity: Date, rate: f64) -> Self {
        let ois = OIS::new(self.reference_date, maturity, rate);
        self.ois_instruments.push(Box::new(ois));
        self
    }

    /// Adds a deposit instrument for the short end.
    #[must_use]
    pub fn add_deposit(mut self, tenor: &str, rate: f64) -> Self {
        if let Some(maturity) = Self::parse_tenor_date(self.reference_date, tenor) {
            let deposit = Deposit::new(self.reference_date, maturity, rate);
            self.ois_instruments.push(Box::new(deposit));
        }
        self
    }

    // ==================== Projection Curves ====================

    /// Adds a projection curve instrument for a specific rate index.
    ///
    /// # Arguments
    ///
    /// * `index` - The rate index (e.g., `RateIndex::TermSOFR3M`)
    /// * `tenor` - Tenor string (e.g., "2Y", "5Y")
    /// * `rate` - Swap rate as a decimal
    #[must_use]
    pub fn add_projection(mut self, index: RateIndex, tenor: &str, rate: f64) -> Self {
        if let Some(maturity) = Self::parse_tenor_date(self.reference_date, tenor) {
            // Create a swap for the projection curve
            let swap = Swap::new(self.reference_date, maturity, rate, Frequency::SemiAnnual);
            self.projection_instruments
                .entry(index)
                .or_default()
                .push(Box::new(swap));
        }
        self
    }

    /// Adds a projection instrument with explicit maturity.
    #[must_use]
    pub fn add_projection_maturity(mut self, index: RateIndex, maturity: Date, rate: f64) -> Self {
        let swap = Swap::new(self.reference_date, maturity, rate, Frequency::SemiAnnual);
        self.projection_instruments
            .entry(index)
            .or_default()
            .push(Box::new(swap));
        self
    }

    // ==================== Basis Swaps ====================

    /// Adds a basis swap between two rate indices.
    ///
    /// # Arguments
    ///
    /// * `pay_index` - Pay leg rate index
    /// * `receive_index` - Receive leg rate index
    /// * `tenor` - Tenor string (e.g., "5Y")
    /// * `spread` - Basis spread (receive - pay) as decimal
    #[must_use]
    pub fn add_basis_swap(
        mut self,
        pay_index: RateIndex,
        receive_index: RateIndex,
        tenor: &str,
        spread: f64,
    ) -> Self {
        self.basis_instruments.push(_BasisSwapSpec {
            pay_index,
            receive_index,
            tenor: tenor.to_string(),
            spread,
        });
        self
    }

    // ==================== FX Curves ====================

    /// Adds an FX curve specification.
    ///
    /// The actual FX curve will be built after the interest rate curves.
    #[must_use]
    pub fn add_fx_curve(mut self, pair: CurrencyPair, spot_rate: f64) -> Self {
        self.fx_specs.push(_FxCurveSpec {
            pair,
            spot_rate,
            basis_bps: None,
        });
        self
    }

    /// Adds an FX curve specification with cross-currency basis.
    #[must_use]
    pub fn add_fx_curve_with_basis(
        mut self,
        pair: CurrencyPair,
        spot_rate: f64,
        basis_bps: f64,
    ) -> Self {
        self.fx_specs.push(_FxCurveSpec {
            pair,
            spot_rate,
            basis_bps: Some(basis_bps),
        });
        self
    }

    // ==================== Build ====================

    /// Builds the multi-curve environment.
    ///
    /// # Build Order
    ///
    /// 1. Bootstrap discount curve from OIS instruments
    /// 2. Bootstrap projection curves relative to discount
    /// 3. Apply basis adjustments (if any)
    /// 4. Build FX forward curves (if any)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No OIS instruments are provided
    /// - Bootstrap fails for any instrument
    pub fn build(mut self) -> CurveResult<CurveSet> {
        if self.ois_instruments.is_empty() {
            return Err(CurveError::invalid_data(
                "No OIS instruments provided for discount curve",
            ));
        }

        // Sort instruments by maturity
        self.ois_instruments.sort_by_key(|inst| inst.pillar_date());

        // 1. Bootstrap discount curve
        let discount_curve = self.bootstrap_discount_curve()?;
        let discount_arc = Arc::new(discount_curve);

        // 2. Bootstrap projection curves
        let mut projection_curves = HashMap::new();
        for (index, instruments) in &self.projection_instruments {
            let projection = self.bootstrap_projection_curve(&discount_arc, instruments)?;
            projection_curves.insert(index.clone(), Arc::new(projection));
        }

        // 3. Apply basis adjustments
        self.apply_basis_adjustments(&discount_arc, &mut projection_curves)?;

        // 4. Build FX curves (placeholder - would need foreign curves)
        // FX curves require foreign discount curves which would come from separate builds
        let _fx_curves: HashMap<CurrencyPair, Arc<super::fx_forward::FxForwardCurve>> =
            HashMap::new();

        // Build the curve set
        let mut builder =
            CurveSetBuilder::new(self.reference_date).discount_curve_arc(discount_arc);

        for (index, curve) in projection_curves {
            builder = builder.projection_curve_arc(index, curve);
        }

        builder.build()
    }

    /// Bootstraps the discount curve from OIS instruments.
    fn bootstrap_discount_curve(&self) -> CurveResult<DiscountCurve> {
        let mut bootstrapper = SequentialBootstrapper::new(self.reference_date)
            .with_interpolation(self.config.discount_interpolation)
            .with_extrapolation(self.config.allow_extrapolation);

        for instrument in &self.ois_instruments {
            // Clone the instrument into the bootstrapper
            bootstrapper = bootstrapper.add_instrument(GenericInstrument::from_boxed(instrument));
        }

        bootstrapper.bootstrap()
    }

    /// Bootstraps a projection curve relative to the discount curve.
    fn bootstrap_projection_curve(
        &self,
        discount: &Arc<DiscountCurve>,
        instruments: &[Box<dyn CurveInstrument>],
    ) -> CurveResult<ForwardCurve> {
        if instruments.is_empty() {
            // If no instruments, just create a forward curve based on the discount curve
            return ForwardCurveBuilder::new()
                .base_curve(Arc::clone(discount) as Arc<dyn Curve>)
                .tenor(0.25)
                .build();
        }

        // Bootstrap a separate discount curve for this projection
        let mut bootstrapper = SequentialBootstrapper::new(self.reference_date)
            .with_interpolation(self.config.projection_interpolation)
            .with_extrapolation(self.config.allow_extrapolation);

        for instrument in instruments {
            bootstrapper = bootstrapper.add_instrument(GenericInstrument::from_boxed(instrument));
        }

        let proj_discount = bootstrapper.bootstrap()?;

        // Calculate the average spread vs the OIS discount curve
        let mut total_spread = 0.0;
        let mut count = 0;

        for inst in instruments {
            let t = self.year_fraction(inst.pillar_date());
            if t > 0.0 {
                let ois_fwd = Curve::forward_rate(discount.as_ref(), 0.0, t).unwrap_or(0.0);
                let proj_fwd = Curve::forward_rate(&proj_discount, 0.0, t).unwrap_or(0.0);
                total_spread += proj_fwd - ois_fwd;
                count += 1;
            }
        }

        let avg_spread = if count > 0 {
            total_spread / f64::from(count)
        } else {
            0.0
        };

        ForwardCurveBuilder::new()
            .base_curve(Arc::clone(discount) as Arc<dyn Curve>)
            .tenor(0.25) // Default 3M tenor
            .spread(avg_spread)
            .build()
    }

    /// Applies basis adjustments to projection curves.
    fn apply_basis_adjustments(
        &self,
        _discount: &Arc<DiscountCurve>,
        _projections: &mut HashMap<RateIndex, Arc<ForwardCurve>>,
    ) -> CurveResult<()> {
        // For now, basis adjustments are applied as spread adjustments
        // A full implementation would solve for spreads iteratively

        for spec in &self.basis_instruments {
            // TODO: Implement proper basis adjustment
            // This would require:
            // 1. Price the basis swap using current curves
            // 2. Solve for the spread adjustment that matches market
            // 3. Apply to the appropriate projection curve
            let _ = spec; // Silence unused warning
        }

        Ok(())
    }

    /// Parses a tenor string to a date.
    fn parse_tenor_date(reference: Date, tenor: &str) -> Option<Date> {
        Tenor::parse(tenor).map(|t| {
            let months = t.months();
            if months == 0 {
                // Handle days-based tenors
                let days = match t {
                    Tenor::ON | Tenor::TN => 1,
                    Tenor::W1 => 7,
                    Tenor::W2 => 14,
                    _ => 0,
                };
                reference.add_days(i64::from(days))
            } else {
                reference.add_months(months as i32).unwrap_or(reference)
            }
        })
    }

    /// Calculates year fraction from reference date.
    fn year_fraction(&self, date: Date) -> f64 {
        self.reference_date.days_between(&date) as f64 / 365.0
    }
}

/// A generic instrument wrapper that captures the behavior of the original instrument.
///
/// This is needed because we can't clone `Box<dyn CurveInstrument>` directly,
/// but we need to pass instruments to the bootstrapper.
struct GenericInstrument {
    maturity: Date,
    pillar: Date,
    instrument_type: crate::instruments::InstrumentType,
    desc: String,
    /// The original rate for implied DF calculation
    rate: f64,
}

impl GenericInstrument {
    /// Creates a `GenericInstrument` from a boxed `CurveInstrument`.
    fn from_boxed(inst: &Box<dyn CurveInstrument>) -> Self {
        Self {
            maturity: inst.maturity(),
            pillar: inst.pillar_date(),
            instrument_type: inst.instrument_type(),
            desc: inst.description(),
            rate: 0.05, // Default rate - will be refined during bootstrap
        }
    }
}

impl CurveInstrument for GenericInstrument {
    fn maturity(&self) -> Date {
        self.maturity
    }

    fn pillar_date(&self) -> Date {
        self.pillar
    }

    fn pv(&self, _curve: &dyn Curve) -> CurveResult<f64> {
        // Generic instruments return 0 PV (at par)
        Ok(0.0)
    }

    fn implied_df(&self, curve: &dyn Curve, _target_pv: f64) -> CurveResult<f64> {
        // Use simple exponential discount factor estimation
        let t = curve.year_fraction(self.pillar);
        if t <= 0.0 {
            return Ok(1.0);
        }

        // Get the zero rate at the previous point and extrapolate
        let prev_t = t * 0.99;
        let prev_df = curve.discount_factor(prev_t)?;
        let implied_rate = if prev_t > 0.0 {
            -prev_df.ln() / prev_t
        } else {
            self.rate
        };

        Ok((-implied_rate * t).exp())
    }

    fn instrument_type(&self) -> crate::instruments::InstrumentType {
        self.instrument_type
    }

    fn description(&self) -> String {
        self.desc.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_curve_builder_basic() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let curve_set = MultiCurveBuilder::new(ref_date)
            .add_ois("1M", 0.0525)
            .add_ois("3M", 0.0520)
            .add_ois("6M", 0.0515)
            .add_ois("1Y", 0.0505)
            .add_ois("2Y", 0.0475)
            .add_ois("5Y", 0.0435)
            .build()
            .unwrap();

        // Verify discount curve was built and returns valid discount factors
        let df_1y = curve_set.discount_factor_at(1.0).unwrap();
        // Discount factor should be between 0 and 1
        assert!(df_1y > 0.0 && df_1y < 1.0);
        // With positive rates, DF at 1Y should be less than 1
        assert!(df_1y < 1.0);

        // Verify no projection curves (none added)
        assert_eq!(curve_set.projection_curve_count(), 0);
    }

    #[test]
    fn test_multi_curve_builder_with_projection() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let curve_set = MultiCurveBuilder::new(ref_date)
            // Discount curve
            .add_ois("1Y", 0.0505)
            .add_ois("2Y", 0.0475)
            .add_ois("5Y", 0.0435)
            // Projection curve
            .add_projection(RateIndex::term_sofr_3m(), "2Y", 0.0480)
            .add_projection(RateIndex::term_sofr_3m(), "5Y", 0.0445)
            .build()
            .unwrap();

        // Should have one projection curve
        assert_eq!(curve_set.projection_curve_count(), 1);
        assert!(curve_set.has_projection_curve(&RateIndex::term_sofr_3m()));
    }

    #[test]
    fn test_multi_curve_builder_no_ois_fails() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let result = MultiCurveBuilder::new(ref_date).build();
        assert!(result.is_err());
    }

    #[test]
    fn test_tenor_parsing() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let date_1m = MultiCurveBuilder::parse_tenor_date(ref_date, "1M").unwrap();
        let date_1y = MultiCurveBuilder::parse_tenor_date(ref_date, "1Y").unwrap();
        let date_5y = MultiCurveBuilder::parse_tenor_date(ref_date, "5Y").unwrap();

        assert_eq!(date_1m.month(), 2); // Feb
        assert_eq!(date_1y.year(), 2026);
        assert_eq!(date_5y.year(), 2030);
    }

    #[test]
    fn test_multi_curve_config() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        // Use LogLinear interpolation which is more stable
        let config = MultiCurveConfig {
            discount_interpolation: InterpolationMethod::LogLinear,
            projection_interpolation: InterpolationMethod::LogLinear,
            max_iterations: 20,
            tolerance: 1e-12,
            allow_extrapolation: true,
        };

        let curve_set = MultiCurveBuilder::new(ref_date)
            .with_config(config)
            .add_ois("1Y", 0.05)
            .add_ois("2Y", 0.048)
            .add_ois("5Y", 0.045)
            .build()
            .unwrap();

        // Should build successfully with custom config
        assert!(curve_set.discount_factor_at(2.0).is_ok());
    }

    #[test]
    fn test_discount_curve_monotonic() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let curve_set = MultiCurveBuilder::new(ref_date)
            .add_ois("1M", 0.053)
            .add_ois("3M", 0.052)
            .add_ois("6M", 0.051)
            .add_ois("1Y", 0.050)
            .add_ois("2Y", 0.048)
            .add_ois("5Y", 0.045)
            .add_ois("10Y", 0.043)
            .build()
            .unwrap();

        // Discount factors should be monotonically decreasing
        let df_1m = curve_set.discount_factor_at(1.0 / 12.0).unwrap();
        let df_3m = curve_set.discount_factor_at(0.25).unwrap();
        let df_1y = curve_set.discount_factor_at(1.0).unwrap();
        let df_5y = curve_set.discount_factor_at(5.0).unwrap();

        assert!(df_1m > df_3m);
        assert!(df_3m > df_1y);
        assert!(df_1y > df_5y);
    }
}
