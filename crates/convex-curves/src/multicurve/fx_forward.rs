//! FX forward curve for cross-currency pricing.
//!
//! Implements FX forward rates derived from interest rate parity:
//!
//! ```text
//! F(t) = S × DF_foreign(t) / DF_domestic(t) × basis_adjustment(t)
//! ```

use std::sync::Arc;

use convex_core::Currency;
use serde::{Deserialize, Serialize};

use crate::curves::DiscountCurve;
use crate::error::{CurveError, CurveResult};
use crate::traits::Curve;

/// A currency pair (e.g., EUR/USD).
///
/// The pair represents the exchange rate quote convention:
/// - `base` (foreign): The currency being priced
/// - `quote` (domestic): The currency used for pricing
///
/// For EUR/USD = 1.10, base=EUR, quote=USD, meaning 1 EUR = 1.10 USD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CurrencyPair {
    /// Base (foreign) currency
    pub base: Currency,
    /// Quote (domestic) currency
    pub quote: Currency,
}

impl CurrencyPair {
    /// Creates a new currency pair.
    #[must_use]
    pub fn new(base: Currency, quote: Currency) -> Self {
        Self { base, quote }
    }

    /// Creates EUR/USD pair.
    #[must_use]
    pub fn eurusd() -> Self {
        Self::new(Currency::EUR, Currency::USD)
    }

    /// Creates GBP/USD pair.
    #[must_use]
    pub fn gbpusd() -> Self {
        Self::new(Currency::GBP, Currency::USD)
    }

    /// Creates USD/JPY pair.
    #[must_use]
    pub fn usdjpy() -> Self {
        Self::new(Currency::USD, Currency::JPY)
    }

    /// Creates USD/CHF pair.
    #[must_use]
    pub fn usdchf() -> Self {
        Self::new(Currency::USD, Currency::CHF)
    }

    /// Creates EUR/GBP pair.
    #[must_use]
    pub fn eurgbp() -> Self {
        Self::new(Currency::EUR, Currency::GBP)
    }

    /// Creates AUD/USD pair.
    #[must_use]
    pub fn audusd() -> Self {
        Self::new(Currency::AUD, Currency::USD)
    }

    /// Creates USD/CAD pair.
    #[must_use]
    pub fn usdcad() -> Self {
        Self::new(Currency::USD, Currency::CAD)
    }

    /// Returns the inverse pair (swaps base and quote).
    #[must_use]
    pub fn inverse(&self) -> Self {
        Self::new(self.quote, self.base)
    }

    /// Returns the Bloomberg-style ticker (e.g., "EURUSD").
    #[must_use]
    pub fn ticker(&self) -> String {
        format!("{}{}", self.base.code(), self.quote.code())
    }

    /// Returns the slash notation (e.g., "EUR/USD").
    #[must_use]
    pub fn display(&self) -> String {
        format!("{}/{}", self.base.code(), self.quote.code())
    }
}

impl std::fmt::Display for CurrencyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.base.code(), self.quote.code())
    }
}

/// FX forward curve derived from interest rate parity.
///
/// Uses the covered interest rate parity relationship:
///
/// ```text
/// F(t) = S × DF_foreign(t) / DF_domestic(t) × basis_adjustment(t)
/// ```
///
/// where:
/// - `S` is the FX spot rate
/// - `DF_foreign` is the discount factor in the foreign (base) currency
/// - `DF_domestic` is the discount factor in the domestic (quote) currency
/// - `basis_adjustment` accounts for cross-currency basis spread
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::multicurve::*;
///
/// // EUR/USD forward curve
/// let fx_curve = FxForwardCurveBuilder::new(CurrencyPair::eurusd())
///     .spot_rate(1.10)
///     .domestic_curve(usd_ois_curve)  // USD discount curve
///     .foreign_curve(eur_ois_curve)   // EUR discount curve
///     .basis_curve(xccy_basis_curve)  // Cross-currency basis
///     .build()?;
///
/// // Get 1Y forward rate
/// let fwd_1y = fx_curve.forward_rate(1.0)?;
/// ```
#[derive(Clone)]
pub struct FxForwardCurve {
    /// Currency pair
    pair: CurrencyPair,
    /// FX spot rate
    spot_rate: f64,
    /// Domestic (quote) currency discount curve
    domestic_curve: Arc<DiscountCurve>,
    /// Foreign (base) currency discount curve
    foreign_curve: Arc<DiscountCurve>,
    /// Cross-currency basis spread (additive, optional)
    basis_spread: Option<BasisSpread>,
}

impl std::fmt::Debug for FxForwardCurve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FxForwardCurve")
            .field("pair", &self.pair)
            .field("spot_rate", &self.spot_rate)
            .field("basis_spread", &self.basis_spread)
            .finish()
    }
}

/// Cross-currency basis spread.
#[derive(Debug, Clone)]
pub enum BasisSpread {
    /// Constant basis spread
    Constant(f64),
    /// Term structure of basis spreads (time, spread)
    TermStructure(Vec<(f64, f64)>),
}

impl FxForwardCurve {
    /// Returns the currency pair.
    #[must_use]
    pub fn pair(&self) -> CurrencyPair {
        self.pair
    }

    /// Returns the spot rate.
    #[must_use]
    pub fn spot_rate(&self) -> f64 {
        self.spot_rate
    }

    /// Returns the FX forward rate at time t.
    ///
    /// Uses covered interest rate parity with optional basis adjustment.
    pub fn forward_rate(&self, t: f64) -> CurveResult<f64> {
        if t <= 0.0 {
            return Ok(self.spot_rate);
        }

        // Get discount factors
        let df_domestic = self.domestic_curve.discount_factor(t)?;
        let df_foreign = self.foreign_curve.discount_factor(t)?;

        if df_domestic <= 0.0 {
            return Err(CurveError::invalid_data("Invalid domestic discount factor"));
        }

        // Basic forward from interest rate parity
        // F(t) = S × DF_for(t) / DF_dom(t)
        let mut forward = self.spot_rate * df_foreign / df_domestic;

        // Apply basis adjustment if present
        if let Some(ref basis) = self.basis_spread {
            let basis_adj = self.get_basis_adjustment(t, basis)?;
            // Basis is quoted as rate spread, convert to multiplicative factor
            // F_adj = F × exp(-basis × t) approximately
            forward *= (-basis_adj * t).exp();
        }

        Ok(forward)
    }

    /// Returns the forward points at time t.
    ///
    /// Forward points = Forward - Spot
    pub fn forward_points(&self, t: f64) -> CurveResult<f64> {
        let fwd = self.forward_rate(t)?;
        Ok(fwd - self.spot_rate)
    }

    /// Returns the forward points in pips (1 pip = 0.0001 for most pairs).
    pub fn forward_points_pips(&self, t: f64) -> CurveResult<f64> {
        let points = self.forward_points(t)?;
        let pip_size = self.pip_size();
        Ok(points / pip_size)
    }

    /// Returns the pip size for this pair.
    #[must_use]
    pub fn pip_size(&self) -> f64 {
        // JPY pairs have pip = 0.01, others = 0.0001
        if self.pair.quote == Currency::JPY || self.pair.base == Currency::JPY {
            0.01
        } else {
            0.0001
        }
    }

    /// Returns the implied forward rate differential.
    ///
    /// This is approximately: r_domestic - r_foreign
    pub fn implied_rate_differential(&self, t: f64) -> CurveResult<f64> {
        if t <= 0.0 {
            return Ok(0.0);
        }

        let fwd = self.forward_rate(t)?;
        // F = S × exp((r_dom - r_for) × t)
        // => r_dom - r_for = ln(F/S) / t
        Ok((fwd / self.spot_rate).ln() / t)
    }

    /// Returns the domestic discount curve.
    #[must_use]
    pub fn domestic_curve(&self) -> &DiscountCurve {
        &self.domestic_curve
    }

    /// Returns the foreign discount curve.
    #[must_use]
    pub fn foreign_curve(&self) -> &DiscountCurve {
        &self.foreign_curve
    }

    /// Interpolates basis spread at time t.
    fn get_basis_adjustment(&self, t: f64, basis: &BasisSpread) -> CurveResult<f64> {
        match basis {
            BasisSpread::Constant(spread) => Ok(*spread),
            BasisSpread::TermStructure(points) => {
                if points.is_empty() {
                    return Ok(0.0);
                }

                // Simple linear interpolation
                if t <= points[0].0 {
                    return Ok(points[0].1);
                }
                if t >= points[points.len() - 1].0 {
                    return Ok(points[points.len() - 1].1);
                }

                for i in 0..points.len() - 1 {
                    let (t0, s0) = points[i];
                    let (t1, s1) = points[i + 1];
                    if t >= t0 && t <= t1 {
                        let w = (t - t0) / (t1 - t0);
                        return Ok(s0 + w * (s1 - s0));
                    }
                }

                Ok(0.0)
            }
        }
    }
}

/// Builder for [`FxForwardCurve`].
pub struct FxForwardCurveBuilder {
    pair: CurrencyPair,
    spot_rate: Option<f64>,
    domestic_curve: Option<Arc<DiscountCurve>>,
    foreign_curve: Option<Arc<DiscountCurve>>,
    basis_spread: Option<BasisSpread>,
}

impl FxForwardCurveBuilder {
    /// Creates a new builder for the given currency pair.
    #[must_use]
    pub fn new(pair: CurrencyPair) -> Self {
        Self {
            pair,
            spot_rate: None,
            domestic_curve: None,
            foreign_curve: None,
            basis_spread: None,
        }
    }

    /// Sets the FX spot rate.
    #[must_use]
    pub fn spot_rate(mut self, rate: f64) -> Self {
        self.spot_rate = Some(rate);
        self
    }

    /// Sets the domestic (quote) currency discount curve.
    #[must_use]
    pub fn domestic_curve(mut self, curve: DiscountCurve) -> Self {
        self.domestic_curve = Some(Arc::new(curve));
        self
    }

    /// Sets the domestic curve from an Arc.
    #[must_use]
    pub fn domestic_curve_arc(mut self, curve: Arc<DiscountCurve>) -> Self {
        self.domestic_curve = Some(curve);
        self
    }

    /// Sets the foreign (base) currency discount curve.
    #[must_use]
    pub fn foreign_curve(mut self, curve: DiscountCurve) -> Self {
        self.foreign_curve = Some(Arc::new(curve));
        self
    }

    /// Sets the foreign curve from an Arc.
    #[must_use]
    pub fn foreign_curve_arc(mut self, curve: Arc<DiscountCurve>) -> Self {
        self.foreign_curve = Some(curve);
        self
    }

    /// Sets a constant cross-currency basis spread.
    #[must_use]
    pub fn constant_basis(mut self, spread: f64) -> Self {
        self.basis_spread = Some(BasisSpread::Constant(spread));
        self
    }

    /// Sets a constant basis spread in basis points.
    #[must_use]
    pub fn constant_basis_bps(mut self, bps: f64) -> Self {
        self.basis_spread = Some(BasisSpread::Constant(bps / 10000.0));
        self
    }

    /// Sets a term structure of basis spreads.
    #[must_use]
    pub fn basis_term_structure(mut self, points: Vec<(f64, f64)>) -> Self {
        self.basis_spread = Some(BasisSpread::TermStructure(points));
        self
    }

    /// Builds the FX forward curve.
    pub fn build(self) -> CurveResult<FxForwardCurve> {
        let spot = self
            .spot_rate
            .ok_or_else(|| CurveError::invalid_data("Spot rate is required"))?;

        let domestic = self
            .domestic_curve
            .ok_or_else(|| CurveError::invalid_data("Domestic curve is required"))?;

        let foreign = self
            .foreign_curve
            .ok_or_else(|| CurveError::invalid_data("Foreign curve is required"))?;

        if spot <= 0.0 {
            return Err(CurveError::invalid_data("Spot rate must be positive"));
        }

        Ok(FxForwardCurve {
            pair: self.pair,
            spot_rate: spot,
            domestic_curve: domestic,
            foreign_curve: foreign,
            basis_spread: self.basis_spread,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscountCurveBuilder;
    use crate::interpolation::InterpolationMethod;

    fn usd_curve(ref_date: convex_core::Date) -> DiscountCurve {
        // USD curve ~5% rate
        DiscountCurveBuilder::new(ref_date)
            .add_pillar(0.0, 1.0)
            .add_pillar(1.0, 0.9524)  // ≈ exp(-0.05)
            .add_pillar(5.0, 0.7788)  // ≈ exp(-0.05*5)
            .with_interpolation(InterpolationMethod::LogLinear)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    fn eur_curve(ref_date: convex_core::Date) -> DiscountCurve {
        // EUR curve ~3% rate
        DiscountCurveBuilder::new(ref_date)
            .add_pillar(0.0, 1.0)
            .add_pillar(1.0, 0.9704)  // ≈ exp(-0.03)
            .add_pillar(5.0, 0.8607)  // ≈ exp(-0.03*5)
            .with_interpolation(InterpolationMethod::LogLinear)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    #[test]
    fn test_currency_pair() {
        let pair = CurrencyPair::eurusd();
        assert_eq!(pair.base, Currency::EUR);
        assert_eq!(pair.quote, Currency::USD);
        assert_eq!(pair.ticker(), "EURUSD");
        assert_eq!(pair.display(), "EUR/USD");
        assert_eq!(format!("{}", pair), "EUR/USD");
    }

    #[test]
    fn test_currency_pair_inverse() {
        let eurusd = CurrencyPair::eurusd();
        let usdeur = eurusd.inverse();
        assert_eq!(usdeur.base, Currency::USD);
        assert_eq!(usdeur.quote, Currency::EUR);
    }

    #[test]
    fn test_fx_forward_spot() {
        let ref_date = convex_core::Date::from_ymd(2025, 1, 1).unwrap();

        let fx = FxForwardCurveBuilder::new(CurrencyPair::eurusd())
            .spot_rate(1.10)
            .domestic_curve(usd_curve(ref_date))
            .foreign_curve(eur_curve(ref_date))
            .build()
            .unwrap();

        // At t=0, forward should equal spot
        let fwd_0 = fx.forward_rate(0.0).unwrap();
        assert!((fwd_0 - 1.10).abs() < 1e-10);
    }

    #[test]
    fn test_fx_forward_interest_rate_parity() {
        let ref_date = convex_core::Date::from_ymd(2025, 1, 1).unwrap();

        let fx = FxForwardCurveBuilder::new(CurrencyPair::eurusd())
            .spot_rate(1.10)
            .domestic_curve(usd_curve(ref_date))
            .foreign_curve(eur_curve(ref_date))
            .build()
            .unwrap();

        // 1Y forward: USD rate > EUR rate, so EUR/USD should appreciate
        // F = S × DF_EUR / DF_USD = 1.10 × 0.9704 / 0.9524 ≈ 1.121
        let fwd_1y = fx.forward_rate(1.0).unwrap();
        assert!(fwd_1y > fx.spot_rate());  // EUR appreciates vs USD

        // Forward points should be positive
        let points = fx.forward_points(1.0).unwrap();
        assert!(points > 0.0);
    }

    #[test]
    fn test_fx_forward_with_basis() {
        let ref_date = convex_core::Date::from_ymd(2025, 1, 1).unwrap();

        let fx_no_basis = FxForwardCurveBuilder::new(CurrencyPair::eurusd())
            .spot_rate(1.10)
            .domestic_curve(usd_curve(ref_date))
            .foreign_curve(eur_curve(ref_date.clone()))
            .build()
            .unwrap();

        let fx_with_basis = FxForwardCurveBuilder::new(CurrencyPair::eurusd())
            .spot_rate(1.10)
            .domestic_curve(usd_curve(ref_date))
            .foreign_curve(eur_curve(ref_date))
            .constant_basis_bps(-20.0)  // 20bp negative basis
            .build()
            .unwrap();

        let fwd_no_basis = fx_no_basis.forward_rate(1.0).unwrap();
        let fwd_with_basis = fx_with_basis.forward_rate(1.0).unwrap();

        // Negative basis should lower the forward
        assert!(fwd_with_basis > fwd_no_basis);
    }

    #[test]
    fn test_implied_rate_differential() {
        let ref_date = convex_core::Date::from_ymd(2025, 1, 1).unwrap();

        let fx = FxForwardCurveBuilder::new(CurrencyPair::eurusd())
            .spot_rate(1.10)
            .domestic_curve(usd_curve(ref_date))
            .foreign_curve(eur_curve(ref_date))
            .build()
            .unwrap();

        // Rate differential should be approximately USD rate - EUR rate ≈ 5% - 3% = 2%
        let diff = fx.implied_rate_differential(1.0).unwrap();
        assert!((diff - 0.02).abs() < 0.005);
    }

    #[test]
    fn test_pip_size() {
        let eurusd = FxForwardCurveBuilder::new(CurrencyPair::eurusd())
            .spot_rate(1.10)
            .domestic_curve(usd_curve(convex_core::Date::from_ymd(2025, 1, 1).unwrap()))
            .foreign_curve(eur_curve(convex_core::Date::from_ymd(2025, 1, 1).unwrap()))
            .build()
            .unwrap();

        let usdjpy = FxForwardCurveBuilder::new(CurrencyPair::usdjpy())
            .spot_rate(145.0)
            .domestic_curve(usd_curve(convex_core::Date::from_ymd(2025, 1, 1).unwrap()))
            .foreign_curve(usd_curve(convex_core::Date::from_ymd(2025, 1, 1).unwrap()))
            .build()
            .unwrap();

        assert!((eurusd.pip_size() - 0.0001).abs() < 1e-10);
        assert!((usdjpy.pip_size() - 0.01).abs() < 1e-10);
    }
}
