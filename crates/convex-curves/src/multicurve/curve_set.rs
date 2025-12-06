//! Curve set container for multi-curve framework.
//!
//! A [`CurveSet`] holds a complete multi-curve environment with:
//! - One discount curve (OIS)
//! - Multiple projection curves (by rate index)
//! - Optional FX forward curves

use std::collections::HashMap;
use std::sync::Arc;

use convex_core::Date;

use crate::curves::{DiscountCurve, ForwardCurve};
use crate::error::{CurveError, CurveResult};
use crate::traits::Curve;

use super::fx_forward::FxForwardCurve;
use super::rate_index::RateIndex;
use super::CurrencyPair;

/// A complete multi-curve environment.
///
/// Holds all the curves needed for pricing in a multi-curve framework:
///
/// - **Discount curve**: OIS curve used for discounting cash flows
/// - **Projection curves**: Forward curves for different rate indices
/// - **FX forward curves**: Cross-currency forward rates
///
/// # Thread Safety
///
/// `CurveSet` uses `Arc` internally and is fully thread-safe for concurrent read access.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::multicurve::*;
///
/// let curve_set = CurveSetBuilder::new(reference_date)
///     .discount_curve(ois_curve)
///     .projection_curve(RateIndex::TermSOFR3M, sofr_3m_curve)
///     .projection_curve(RateIndex::EURIBOR { tenor: Tenor::M3 }, euribor_curve)
///     .build()?;
///
/// // Get discount factor
/// let df = curve_set.discount_factor(date)?;
///
/// // Get forward rate for a specific index
/// let fwd = curve_set.forward_rate(&RateIndex::TermSOFR3M, start, end)?;
/// ```
#[derive(Clone)]
pub struct CurveSet {
    /// Reference date for all curves
    reference_date: Date,
    /// Discount (OIS) curve
    discount_curve: Arc<DiscountCurve>,
    /// Projection curves by rate index
    projection_curves: HashMap<RateIndex, Arc<ForwardCurve>>,
    /// FX forward curves by currency pair
    fx_curves: HashMap<CurrencyPair, Arc<FxForwardCurve>>,
}

impl std::fmt::Debug for CurveSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CurveSet")
            .field("reference_date", &self.reference_date)
            .field("projection_curves", &self.projection_curves.keys().collect::<Vec<_>>())
            .field("fx_curves", &self.fx_curves.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl CurveSet {
    /// Returns the reference date for all curves.
    #[must_use]
    pub fn reference_date(&self) -> Date {
        self.reference_date
    }

    /// Returns the discount curve.
    #[must_use]
    pub fn discount_curve(&self) -> &DiscountCurve {
        &self.discount_curve
    }

    /// Returns a projection curve for the given rate index.
    #[must_use]
    pub fn projection_curve(&self, index: &RateIndex) -> Option<&ForwardCurve> {
        self.projection_curves.get(index).map(Arc::as_ref)
    }

    /// Returns all available rate indices.
    pub fn available_indices(&self) -> impl Iterator<Item = &RateIndex> {
        self.projection_curves.keys()
    }

    /// Returns all available currency pairs for FX.
    pub fn available_fx_pairs(&self) -> impl Iterator<Item = &CurrencyPair> {
        self.fx_curves.keys()
    }

    // ==================== Discount curve methods ====================

    /// Returns the discount factor from the reference date to a specific date.
    ///
    /// Uses the OIS discount curve.
    pub fn discount_factor(&self, date: Date) -> CurveResult<f64> {
        self.discount_curve.discount_factor_at(date)
    }

    /// Returns the discount factor for a time in years.
    pub fn discount_factor_at(&self, t: f64) -> CurveResult<f64> {
        self.discount_curve.discount_factor(t)
    }

    /// Returns the zero rate from the discount curve.
    pub fn zero_rate(&self, t: f64, compounding: crate::compounding::Compounding) -> CurveResult<f64> {
        self.discount_curve.zero_rate(t, compounding)
    }

    // ==================== Projection curve methods ====================

    /// Returns the forward rate for a specific rate index.
    ///
    /// # Arguments
    ///
    /// * `index` - The rate index (e.g., Term SOFR 3M)
    /// * `start` - Start date of the forward period
    /// * `end` - End date of the forward period
    ///
    /// # Errors
    ///
    /// Returns an error if no projection curve exists for the given index.
    pub fn forward_rate(&self, index: &RateIndex, start: Date, end: Date) -> CurveResult<f64> {
        let curve = self
            .projection_curves
            .get(index)
            .ok_or_else(|| CurveError::invalid_data(format!("No projection curve for index: {index}")))?;

        // Convert dates to time fractions
        let t1 = self.year_fraction(start);
        let t2 = self.year_fraction(end);

        // The ForwardCurve's forward_rate_at gives rate for its fixed tenor
        // For general forward rate between t1 and t2, we need to use the base curve
        let base = curve.base_curve();
        let fwd = base.forward_rate(t1, t2)?;

        // Add the spread from the projection curve
        Ok(fwd + curve.spread())
    }

    /// Returns the forward rate for a specific rate index using year fractions.
    pub fn forward_rate_at(&self, index: &RateIndex, t1: f64, t2: f64) -> CurveResult<f64> {
        let curve = self
            .projection_curves
            .get(index)
            .ok_or_else(|| CurveError::invalid_data(format!("No projection curve for index: {index}")))?;

        let base = curve.base_curve();
        let fwd = base.forward_rate(t1, t2)?;
        Ok(fwd + curve.spread())
    }

    /// Returns the forward rate starting at time t for the index's natural tenor.
    ///
    /// For example, for Term SOFR 3M, this returns the 3M forward rate starting at t.
    pub fn forward_rate_for_tenor(&self, index: &RateIndex, t: f64) -> CurveResult<f64> {
        let curve = self
            .projection_curves
            .get(index)
            .ok_or_else(|| CurveError::invalid_data(format!("No projection curve for index: {index}")))?;

        curve.forward_rate_at(t)
    }

    // ==================== FX curve methods ====================

    /// Returns the FX forward rate for a currency pair.
    ///
    /// # Arguments
    ///
    /// * `pair` - The currency pair
    /// * `date` - The forward date
    ///
    /// # Returns
    ///
    /// The FX forward rate (foreign per domestic)
    pub fn fx_forward(&self, pair: &CurrencyPair, date: Date) -> CurveResult<f64> {
        let curve = self
            .fx_curves
            .get(pair)
            .ok_or_else(|| CurveError::invalid_data(format!("No FX curve for pair: {pair}")))?;

        let t = self.year_fraction(date);
        curve.forward_rate(t)
    }

    /// Returns the FX forward rate for a currency pair at time t.
    pub fn fx_forward_at(&self, pair: &CurrencyPair, t: f64) -> CurveResult<f64> {
        let curve = self
            .fx_curves
            .get(pair)
            .ok_or_else(|| CurveError::invalid_data(format!("No FX curve for pair: {pair}")))?;

        curve.forward_rate(t)
    }

    /// Returns the FX spot rate for a currency pair.
    pub fn fx_spot(&self, pair: &CurrencyPair) -> CurveResult<f64> {
        let curve = self
            .fx_curves
            .get(pair)
            .ok_or_else(|| CurveError::invalid_data(format!("No FX curve for pair: {pair}")))?;

        Ok(curve.spot_rate())
    }

    // ==================== Utility methods ====================

    /// Returns the year fraction from the reference date.
    fn year_fraction(&self, date: Date) -> f64 {
        self.reference_date.days_between(&date) as f64 / 365.0
    }

    /// Returns true if a projection curve exists for the given index.
    #[must_use]
    pub fn has_projection_curve(&self, index: &RateIndex) -> bool {
        self.projection_curves.contains_key(index)
    }

    /// Returns true if an FX curve exists for the given pair.
    #[must_use]
    pub fn has_fx_curve(&self, pair: &CurrencyPair) -> bool {
        self.fx_curves.contains_key(pair)
    }

    /// Returns the number of projection curves.
    #[must_use]
    pub fn projection_curve_count(&self) -> usize {
        self.projection_curves.len()
    }

    /// Returns the number of FX curves.
    #[must_use]
    pub fn fx_curve_count(&self) -> usize {
        self.fx_curves.len()
    }
}

/// Builder for [`CurveSet`].
pub struct CurveSetBuilder {
    reference_date: Date,
    discount_curve: Option<Arc<DiscountCurve>>,
    projection_curves: HashMap<RateIndex, Arc<ForwardCurve>>,
    fx_curves: HashMap<CurrencyPair, Arc<FxForwardCurve>>,
}

impl CurveSetBuilder {
    /// Creates a new builder.
    #[must_use]
    pub fn new(reference_date: Date) -> Self {
        Self {
            reference_date,
            discount_curve: None,
            projection_curves: HashMap::new(),
            fx_curves: HashMap::new(),
        }
    }

    /// Sets the discount (OIS) curve.
    #[must_use]
    pub fn discount_curve(mut self, curve: DiscountCurve) -> Self {
        self.discount_curve = Some(Arc::new(curve));
        self
    }

    /// Sets the discount curve from an Arc.
    #[must_use]
    pub fn discount_curve_arc(mut self, curve: Arc<DiscountCurve>) -> Self {
        self.discount_curve = Some(curve);
        self
    }

    /// Adds a projection curve for a rate index.
    #[must_use]
    pub fn projection_curve(mut self, index: RateIndex, curve: ForwardCurve) -> Self {
        self.projection_curves.insert(index, Arc::new(curve));
        self
    }

    /// Adds a projection curve from an Arc.
    #[must_use]
    pub fn projection_curve_arc(mut self, index: RateIndex, curve: Arc<ForwardCurve>) -> Self {
        self.projection_curves.insert(index, curve);
        self
    }

    /// Adds an FX forward curve.
    #[must_use]
    pub fn fx_curve(mut self, pair: CurrencyPair, curve: FxForwardCurve) -> Self {
        self.fx_curves.insert(pair, Arc::new(curve));
        self
    }

    /// Adds an FX forward curve from an Arc.
    #[must_use]
    pub fn fx_curve_arc(mut self, pair: CurrencyPair, curve: Arc<FxForwardCurve>) -> Self {
        self.fx_curves.insert(pair, curve);
        self
    }

    /// Builds the curve set.
    ///
    /// # Errors
    ///
    /// Returns an error if no discount curve was provided.
    pub fn build(self) -> CurveResult<CurveSet> {
        let discount_curve = self
            .discount_curve
            .ok_or_else(|| CurveError::invalid_data("Discount curve is required"))?;

        Ok(CurveSet {
            reference_date: self.reference_date,
            discount_curve,
            projection_curves: self.projection_curves,
            fx_curves: self.fx_curves,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::{DiscountCurveBuilder, ForwardCurveBuilder};
    use crate::interpolation::InterpolationMethod;
    use crate::multicurve::Tenor;

    fn sample_discount_curve(ref_date: Date) -> DiscountCurve {
        DiscountCurveBuilder::new(ref_date)
            .add_pillar(0.25, 0.9975)
            .add_pillar(0.5, 0.995)
            .add_pillar(1.0, 0.98)
            .add_pillar(2.0, 0.96)
            .add_pillar(5.0, 0.90)
            .with_interpolation(InterpolationMethod::LogLinear)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    #[test]
    fn test_curve_set_basic() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let discount = sample_discount_curve(ref_date);

        let curve_set = CurveSetBuilder::new(ref_date)
            .discount_curve(discount)
            .build()
            .unwrap();

        assert_eq!(curve_set.reference_date(), ref_date);
        assert_eq!(curve_set.projection_curve_count(), 0);
    }

    #[test]
    fn test_curve_set_with_projection() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let discount = sample_discount_curve(ref_date);

        let projection = ForwardCurveBuilder::new()
            .base_curve(Arc::new(discount.clone()))
            .tenor_months(3)
            .spread_bps(5.0)
            .build()
            .unwrap();

        let curve_set = CurveSetBuilder::new(ref_date)
            .discount_curve(discount)
            .projection_curve(RateIndex::TermSOFR { tenor: Tenor::M3 }, projection)
            .build()
            .unwrap();

        assert_eq!(curve_set.projection_curve_count(), 1);
        assert!(curve_set.has_projection_curve(&RateIndex::term_sofr_3m()));
    }

    #[test]
    fn test_curve_set_discount_factor() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let discount = sample_discount_curve(ref_date);

        let curve_set = CurveSetBuilder::new(ref_date)
            .discount_curve(discount)
            .build()
            .unwrap();

        let df = curve_set.discount_factor_at(1.0).unwrap();
        assert!(df > 0.97 && df < 0.99);
    }

    #[test]
    fn test_curve_set_forward_rate() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let discount = sample_discount_curve(ref_date);

        let projection = ForwardCurveBuilder::new()
            .base_curve(Arc::new(discount.clone()))
            .tenor_months(3)
            .spread_bps(0.0)
            .build()
            .unwrap();

        let curve_set = CurveSetBuilder::new(ref_date)
            .discount_curve(discount)
            .projection_curve(RateIndex::term_sofr_3m(), projection)
            .build()
            .unwrap();

        let fwd = curve_set.forward_rate_at(&RateIndex::term_sofr_3m(), 1.0, 2.0).unwrap();
        assert!(fwd > 0.01 && fwd < 0.10);
    }

    #[test]
    fn test_curve_set_missing_projection() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let discount = sample_discount_curve(ref_date);

        let curve_set = CurveSetBuilder::new(ref_date)
            .discount_curve(discount)
            .build()
            .unwrap();

        let result = curve_set.forward_rate_at(&RateIndex::euribor_3m(), 1.0, 2.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_curve_set_builder_no_discount_fails() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();

        let result = CurveSetBuilder::new(ref_date).build();
        assert!(result.is_err());
    }
}
