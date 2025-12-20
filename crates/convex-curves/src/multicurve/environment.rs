//! Multi-curve pricing environment.
//!
//! Provides a complete curve environment for multi-curve pricing,
//! managing discount curves, projection curves, credit curves, and more.

use std::collections::HashMap;
use std::sync::Arc;

use convex_core::types::Date;

use crate::curves::DiscreteCurve;
use crate::error::{CurveError, CurveResult};
use crate::term_structure::{CurveRef, TermStructure};
use crate::wrappers::{CreditCurve, RateCurve};

use super::index::{Currency, CurrencyPair, RateIndex};

/// A complete curve environment for multi-curve pricing.
///
/// In the post-LIBOR world, accurate pricing requires:
/// - **Discount curves**: OIS-based curves for discounting cash flows
/// - **Projection curves**: Index-specific curves for floating leg projections
/// - **Credit curves**: Issuer-specific survival probability curves
/// - **Government curves**: Benchmark curves per currency
/// - **FX curves**: Forward curves for cross-currency pricing
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::multicurve::{MultiCurveEnvironment, RateIndex};
///
/// let env = MultiCurveEnvironment::builder(today)
///     .discount(Currency::Usd, sofr_curve)
///     .projection(RateIndex::Sofr, sofr_curve)
///     .projection(RateIndex::Euribor3M, euribor_curve)
///     .credit("AAPL", apple_credit_curve)
///     .build()?;
///
/// // Get curves for pricing
/// let discount = env.discount(Currency::Usd)?;
/// let projection = env.projection(RateIndex::Sofr)?;
/// ```
pub struct MultiCurveEnvironment {
    /// Reference/valuation date.
    reference_date: Date,

    /// OIS discount curves per currency.
    discount_curves: HashMap<Currency, CurveRef>,

    /// Projection curves per rate index.
    projection_curves: HashMap<RateIndex, CurveRef>,

    /// Credit curves per issuer.
    credit_curves: HashMap<String, Arc<CreditCurve<DiscreteCurve>>>,

    /// Government benchmark curves per currency.
    govt_curves: HashMap<Currency, CurveRef>,

    /// FX forward curves per currency pair.
    fx_curves: HashMap<CurrencyPair, CurveRef>,
}

impl MultiCurveEnvironment {
    /// Creates a builder for constructing a multi-curve environment.
    #[must_use]
    pub fn builder(reference_date: Date) -> MultiCurveEnvironmentBuilder {
        MultiCurveEnvironmentBuilder::new(reference_date)
    }

    /// Returns the reference date.
    #[must_use]
    pub fn reference_date(&self) -> Date {
        self.reference_date
    }

    /// Returns the discount curve for a currency.
    ///
    /// The discount curve should be the OIS curve (SOFR, ESTR, SONIA, etc.)
    /// for the given currency.
    pub fn discount(&self, currency: Currency) -> CurveResult<&dyn TermStructure> {
        self.discount_curves
            .get(&currency)
            .map(|c| c.as_ref())
            .ok_or_else(|| CurveError::curve_not_found(format!("{} discount", currency)))
    }

    /// Returns the projection curve for a rate index.
    ///
    /// The projection curve is used for calculating floating leg cash flows.
    pub fn projection(&self, index: RateIndex) -> CurveResult<&dyn TermStructure> {
        self.projection_curves
            .get(&index)
            .map(|c| c.as_ref())
            .ok_or_else(|| CurveError::curve_not_found(format!("{} projection", index)))
    }

    /// Returns the credit curve for an issuer.
    pub fn credit(&self, issuer: &str) -> CurveResult<&CreditCurve<DiscreteCurve>> {
        self.credit_curves
            .get(issuer)
            .map(|c| c.as_ref())
            .ok_or_else(|| CurveError::curve_not_found(format!("{} credit", issuer)))
    }

    /// Returns the government benchmark curve for a currency.
    pub fn govt(&self, currency: Currency) -> CurveResult<&dyn TermStructure> {
        self.govt_curves
            .get(&currency)
            .map(|c| c.as_ref())
            .ok_or_else(|| CurveError::curve_not_found(format!("{} govt", currency)))
    }

    /// Returns the FX forward curve for a currency pair.
    pub fn fx(&self, pair: CurrencyPair) -> CurveResult<&dyn TermStructure> {
        self.fx_curves
            .get(&pair)
            .map(|c| c.as_ref())
            .ok_or_else(|| CurveError::curve_not_found(format!("{} FX", pair)))
    }

    /// Returns true if a discount curve exists for the currency.
    #[must_use]
    pub fn has_discount(&self, currency: Currency) -> bool {
        self.discount_curves.contains_key(&currency)
    }

    /// Returns true if a projection curve exists for the index.
    #[must_use]
    pub fn has_projection(&self, index: RateIndex) -> bool {
        self.projection_curves.contains_key(&index)
    }

    /// Returns true if a credit curve exists for the issuer.
    #[must_use]
    pub fn has_credit(&self, issuer: &str) -> bool {
        self.credit_curves.contains_key(issuer)
    }

    /// Returns all available currencies with discount curves.
    #[must_use]
    pub fn available_currencies(&self) -> Vec<Currency> {
        self.discount_curves.keys().copied().collect()
    }

    /// Returns all available rate indices with projection curves.
    #[must_use]
    pub fn available_indices(&self) -> Vec<RateIndex> {
        self.projection_curves.keys().copied().collect()
    }

    /// Returns all available issuer names with credit curves.
    #[must_use]
    pub fn available_issuers(&self) -> Vec<&str> {
        self.credit_curves.keys().map(|s| s.as_str()).collect()
    }

    /// Returns a discount factor from the appropriate discount curve.
    pub fn discount_factor(&self, currency: Currency, date: Date) -> CurveResult<f64> {
        let curve = self.discount(currency)?;
        let rate_curve = RateCurve::new(CurveWrapper(curve));
        rate_curve.discount_factor(date)
    }

    /// Returns a forward rate from the projection curve.
    pub fn forward_rate(
        &self,
        index: RateIndex,
        start: Date,
        end: Date,
    ) -> CurveResult<f64> {
        let curve = self.projection(index)?;
        compute_forward_rate(curve, start, end)
    }

    /// Returns a survival probability from a credit curve.
    pub fn survival_probability(&self, issuer: &str, date: Date) -> CurveResult<f64> {
        let credit = self.credit(issuer)?;
        credit.survival_probability(date)
    }

    /// Creates a new environment with an additional/updated discount curve.
    #[must_use]
    pub fn with_discount(mut self, currency: Currency, curve: CurveRef) -> Self {
        self.discount_curves.insert(currency, curve);
        self
    }

    /// Creates a new environment with an additional/updated projection curve.
    #[must_use]
    pub fn with_projection(mut self, index: RateIndex, curve: CurveRef) -> Self {
        self.projection_curves.insert(index, curve);
        self
    }
}

/// Builder for constructing a [`MultiCurveEnvironment`].
pub struct MultiCurveEnvironmentBuilder {
    reference_date: Date,
    discount_curves: HashMap<Currency, CurveRef>,
    projection_curves: HashMap<RateIndex, CurveRef>,
    credit_curves: HashMap<String, Arc<CreditCurve<DiscreteCurve>>>,
    govt_curves: HashMap<Currency, CurveRef>,
    fx_curves: HashMap<CurrencyPair, CurveRef>,
}

impl MultiCurveEnvironmentBuilder {
    /// Creates a new builder.
    #[must_use]
    pub fn new(reference_date: Date) -> Self {
        Self {
            reference_date,
            discount_curves: HashMap::new(),
            projection_curves: HashMap::new(),
            credit_curves: HashMap::new(),
            govt_curves: HashMap::new(),
            fx_curves: HashMap::new(),
        }
    }

    /// Adds a discount curve for a currency.
    #[must_use]
    pub fn discount(mut self, currency: Currency, curve: impl TermStructure + 'static) -> Self {
        self.discount_curves.insert(currency, Arc::new(curve));
        self
    }

    /// Adds a discount curve from an Arc reference.
    #[must_use]
    pub fn discount_ref(mut self, currency: Currency, curve: CurveRef) -> Self {
        self.discount_curves.insert(currency, curve);
        self
    }

    /// Adds a projection curve for a rate index.
    #[must_use]
    pub fn projection(mut self, index: RateIndex, curve: impl TermStructure + 'static) -> Self {
        self.projection_curves.insert(index, Arc::new(curve));
        self
    }

    /// Adds a projection curve from an Arc reference.
    #[must_use]
    pub fn projection_ref(mut self, index: RateIndex, curve: CurveRef) -> Self {
        self.projection_curves.insert(index, curve);
        self
    }

    /// Sets both discount and projection for an overnight index.
    ///
    /// For OIS indices like SOFR, the same curve is typically used for
    /// both discounting and projection.
    #[must_use]
    pub fn ois_curve(mut self, index: RateIndex, curve: impl TermStructure + 'static) -> Self {
        let arc: CurveRef = Arc::new(curve);
        let currency = index.currency();

        // Use the same curve for both
        self.discount_curves.insert(currency, Arc::clone(&arc));
        self.projection_curves.insert(index, arc);
        self
    }

    /// Adds a credit curve for an issuer.
    #[must_use]
    pub fn credit(mut self, issuer: impl Into<String>, curve: CreditCurve<DiscreteCurve>) -> Self {
        self.credit_curves.insert(issuer.into(), Arc::new(curve));
        self
    }

    /// Adds a government benchmark curve.
    #[must_use]
    pub fn govt(mut self, currency: Currency, curve: impl TermStructure + 'static) -> Self {
        self.govt_curves.insert(currency, Arc::new(curve));
        self
    }

    /// Adds an FX forward curve.
    #[must_use]
    pub fn fx(mut self, pair: CurrencyPair, curve: impl TermStructure + 'static) -> Self {
        self.fx_curves.insert(pair, Arc::new(curve));
        self
    }

    /// Builds the multi-curve environment.
    ///
    /// # Errors
    ///
    /// Returns an error if no curves have been added.
    pub fn build(self) -> CurveResult<MultiCurveEnvironment> {
        if self.discount_curves.is_empty() && self.projection_curves.is_empty() {
            return Err(CurveError::calibration_failed(
                0,
                0.0,
                "No curves provided to environment",
            ));
        }

        Ok(MultiCurveEnvironment {
            reference_date: self.reference_date,
            discount_curves: self.discount_curves,
            projection_curves: self.projection_curves,
            credit_curves: self.credit_curves,
            govt_curves: self.govt_curves,
            fx_curves: self.fx_curves,
        })
    }
}

/// Helper to compute forward rate from a curve reference.
fn compute_forward_rate(
    curve: &dyn TermStructure,
    start: Date,
    end: Date,
) -> CurveResult<f64> {
    use convex_core::types::Compounding;

    let rate_curve = RateCurve::new(CurveWrapper(curve));
    rate_curve.forward_rate(start, end, Compounding::Simple)
}

/// Wrapper to allow using &dyn TermStructure with RateCurve.
struct CurveWrapper<'a>(&'a dyn TermStructure);

impl<'a> TermStructure for CurveWrapper<'a> {
    fn reference_date(&self) -> Date {
        self.0.reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        self.0.value_at(t)
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        self.0.tenor_bounds()
    }

    fn value_type(&self) -> crate::ValueType {
        self.0.value_type()
    }

    fn derivative_at(&self, t: f64) -> Option<f64> {
        self.0.derivative_at(t)
    }

    fn max_date(&self) -> Date {
        self.0.max_date()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InterpolationMethod, ValueType};
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::Compounding;

    fn sample_curve(reference_date: Date, rate: f64) -> DiscreteCurve {
        let tenors: Vec<f64> = vec![0.25, 0.5, 1.0, 2.0, 5.0, 10.0];
        let dfs: Vec<f64> = tenors.iter().map(|&t| (-rate * t).exp()).collect();

        DiscreteCurve::new(
            reference_date,
            tenors,
            dfs,
            ValueType::DiscountFactor,
            InterpolationMethod::LogLinear,
        )
        .unwrap()
    }

    fn sample_credit_curve(reference_date: Date) -> CreditCurve<DiscreteCurve> {
        let tenors: Vec<f64> = vec![0.5, 1.0, 2.0, 5.0, 10.0];
        let survivals: Vec<f64> = tenors.iter().map(|&t| (-0.02 * t).exp()).collect();

        let curve = DiscreteCurve::new(
            reference_date,
            tenors,
            survivals,
            ValueType::SurvivalProbability,
            InterpolationMethod::LogLinear,
        )
        .unwrap();

        CreditCurve::new(curve, 0.40)
    }

    #[test]
    fn test_environment_builder() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let sofr_curve = sample_curve(today, 0.04);

        let env = MultiCurveEnvironment::builder(today)
            .ois_curve(RateIndex::Sofr, sofr_curve)
            .build()
            .unwrap();

        assert_eq!(env.reference_date(), today);
        assert!(env.has_discount(Currency::Usd));
        assert!(env.has_projection(RateIndex::Sofr));
    }

    #[test]
    fn test_discount_lookup() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let sofr_curve = sample_curve(today, 0.04);

        let env = MultiCurveEnvironment::builder(today)
            .discount(Currency::Usd, sofr_curve)
            .build()
            .unwrap();

        let discount = env.discount(Currency::Usd);
        assert!(discount.is_ok());

        // EUR discount should fail
        let eur_discount = env.discount(Currency::Eur);
        assert!(eur_discount.is_err());
    }

    #[test]
    fn test_projection_lookup() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let sofr_curve = sample_curve(today, 0.04);
        let euribor_curve = sample_curve(today, 0.035);

        let env = MultiCurveEnvironment::builder(today)
            .projection(RateIndex::Sofr, sofr_curve)
            .projection(RateIndex::Euribor3M, euribor_curve)
            .build()
            .unwrap();

        assert!(env.projection(RateIndex::Sofr).is_ok());
        assert!(env.projection(RateIndex::Euribor3M).is_ok());
        assert!(env.projection(RateIndex::Sonia).is_err());
    }

    #[test]
    fn test_credit_lookup() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let sofr_curve = sample_curve(today, 0.04);
        let apple_credit = sample_credit_curve(today);

        let env = MultiCurveEnvironment::builder(today)
            .discount(Currency::Usd, sofr_curve)
            .credit("AAPL", apple_credit)
            .build()
            .unwrap();

        assert!(env.has_credit("AAPL"));
        assert!(!env.has_credit("MSFT"));

        let credit = env.credit("AAPL");
        assert!(credit.is_ok());
    }

    #[test]
    fn test_available_lookups() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let sofr_curve = sample_curve(today, 0.04);
        let estr_curve = sample_curve(today, 0.035);

        let env = MultiCurveEnvironment::builder(today)
            .ois_curve(RateIndex::Sofr, sofr_curve)
            .ois_curve(RateIndex::Estr, estr_curve)
            .build()
            .unwrap();

        let currencies = env.available_currencies();
        assert!(currencies.contains(&Currency::Usd));
        assert!(currencies.contains(&Currency::Eur));

        let indices = env.available_indices();
        assert!(indices.contains(&RateIndex::Sofr));
        assert!(indices.contains(&RateIndex::Estr));
    }

    #[test]
    fn test_discount_factor_convenience() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let sofr_curve = sample_curve(today, 0.04);

        let env = MultiCurveEnvironment::builder(today)
            .discount(Currency::Usd, sofr_curve)
            .build()
            .unwrap();

        let date_1y = today.add_days(365);
        let df = env.discount_factor(Currency::Usd, date_1y);
        assert!(df.is_ok());

        let df_val = df.unwrap();
        // Should be approximately exp(-0.04 * 1) ≈ 0.9608
        assert!(df_val > 0.95 && df_val < 0.97);
    }

    #[test]
    fn test_empty_environment_fails() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();

        let result = MultiCurveEnvironment::builder(today).build();
        assert!(result.is_err());
    }

    #[test]
    fn test_with_methods() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let sofr_curve = sample_curve(today, 0.04);
        let sonia_curve = sample_curve(today, 0.045);

        let env = MultiCurveEnvironment::builder(today)
            .discount(Currency::Usd, sofr_curve)
            .build()
            .unwrap();

        assert!(env.has_discount(Currency::Usd));
        assert!(!env.has_discount(Currency::Gbp));

        // Add GBP discount curve
        let env = env.with_discount(Currency::Gbp, Arc::new(sonia_curve));
        assert!(env.has_discount(Currency::Gbp));
    }
}
