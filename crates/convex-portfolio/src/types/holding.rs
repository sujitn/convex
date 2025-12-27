//! Holding representation with pre-calculated analytics.

use super::{Classification, WeightingMethod};
use convex_analytics::risk::KeyRateDurations;
use convex_bonds::types::BondIdentifiers;
use convex_core::types::Currency;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Pre-calculated analytics for a holding.
///
/// The caller provides these values; the portfolio module aggregates them.
/// All values are optional to support partial analytics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HoldingAnalytics {
    // =========================================================================
    // YIELDS
    // =========================================================================
    /// Yield to maturity (as decimal, e.g., 0.05 for 5%).
    pub ytm: Option<f64>,

    /// Yield to worst (for callable bonds).
    pub ytw: Option<f64>,

    /// Yield to call (for callable bonds).
    pub ytc: Option<f64>,

    /// Current yield (annual coupon / price).
    pub current_yield: Option<f64>,

    // =========================================================================
    // DURATION
    // =========================================================================
    /// Modified duration.
    pub modified_duration: Option<f64>,

    /// Effective duration (for bonds with embedded options).
    pub effective_duration: Option<f64>,

    /// Macaulay duration.
    pub macaulay_duration: Option<f64>,

    /// Spread duration.
    pub spread_duration: Option<f64>,

    // =========================================================================
    // CONVEXITY
    // =========================================================================
    /// Convexity.
    pub convexity: Option<f64>,

    /// Effective convexity (for bonds with embedded options).
    pub effective_convexity: Option<f64>,

    // =========================================================================
    // DV01
    // =========================================================================
    /// DV01 per unit of par value.
    pub dv01: Option<f64>,

    // =========================================================================
    // KEY RATE DURATIONS
    // =========================================================================
    /// Key rate duration profile.
    pub key_rate_durations: Option<KeyRateDurations>,

    // =========================================================================
    // SPREADS (in basis points)
    // =========================================================================
    /// Z-spread.
    pub z_spread: Option<f64>,

    /// Option-adjusted spread.
    pub oas: Option<f64>,

    /// G-spread (vs government benchmark).
    pub g_spread: Option<f64>,

    /// I-spread (vs swap curve).
    pub i_spread: Option<f64>,

    /// Asset swap spread.
    pub asw: Option<f64>,

    // =========================================================================
    // CREDIT SPREAD SENSITIVITY
    // =========================================================================
    /// Credit spread DV01 (CS01).
    pub cs01: Option<f64>,

    // =========================================================================
    // LIQUIDITY
    // =========================================================================
    /// Bid-ask spread in basis points.
    pub bid_ask_spread: Option<f64>,

    /// Liquidity score (0-100, higher is more liquid).
    pub liquidity_score: Option<f64>,

    // =========================================================================
    // MATURITY
    // =========================================================================
    /// Years to maturity.
    pub years_to_maturity: Option<f64>,
}

impl HoldingAnalytics {
    /// Creates new empty analytics.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the yield to maturity.
    #[must_use]
    pub fn with_ytm(mut self, ytm: f64) -> Self {
        self.ytm = Some(ytm);
        self
    }

    /// Sets the yield to worst.
    #[must_use]
    pub fn with_ytw(mut self, ytw: f64) -> Self {
        self.ytw = Some(ytw);
        self
    }

    /// Sets the modified duration.
    #[must_use]
    pub fn with_modified_duration(mut self, duration: f64) -> Self {
        self.modified_duration = Some(duration);
        self
    }

    /// Sets the effective duration.
    #[must_use]
    pub fn with_effective_duration(mut self, duration: f64) -> Self {
        self.effective_duration = Some(duration);
        self
    }

    /// Sets the convexity.
    #[must_use]
    pub fn with_convexity(mut self, convexity: f64) -> Self {
        self.convexity = Some(convexity);
        self
    }

    /// Sets the DV01.
    #[must_use]
    pub fn with_dv01(mut self, dv01: f64) -> Self {
        self.dv01 = Some(dv01);
        self
    }

    /// Sets the Z-spread.
    #[must_use]
    pub fn with_z_spread(mut self, z_spread: f64) -> Self {
        self.z_spread = Some(z_spread);
        self
    }

    /// Sets the OAS.
    #[must_use]
    pub fn with_oas(mut self, oas: f64) -> Self {
        self.oas = Some(oas);
        self
    }

    /// Sets the years to maturity.
    #[must_use]
    pub fn with_years_to_maturity(mut self, years: f64) -> Self {
        self.years_to_maturity = Some(years);
        self
    }

    /// Returns the best available duration measure.
    /// Prefers effective duration for callable bonds.
    #[must_use]
    pub fn best_duration(&self) -> Option<f64> {
        self.effective_duration.or(self.modified_duration)
    }

    /// Returns the best available yield measure.
    /// Prefers yield to worst for callable bonds.
    #[must_use]
    pub fn best_yield(&self) -> Option<f64> {
        self.ytw.or(self.ytm)
    }

    /// Returns the best available spread measure.
    /// Prefers OAS for callable bonds.
    #[must_use]
    pub fn best_spread(&self) -> Option<f64> {
        self.oas.or(self.z_spread)
    }
}

/// A single holding in a portfolio.
///
/// Represents a bond position with quantity, pricing, and pre-calculated analytics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Holding {
    /// Unique identifier for this position.
    pub id: String,

    /// Bond identifiers (ISIN, CUSIP, etc.).
    pub identifiers: BondIdentifiers,

    /// Par/face amount held.
    pub par_amount: Decimal,

    /// Market price (clean, as % of par, e.g., 98.50).
    pub market_price: Decimal,

    /// Accrued interest per 100 par.
    pub accrued_interest: Decimal,

    /// FX rate to portfolio base currency.
    /// 1.0 if the bond is in the base currency.
    pub fx_rate: Decimal,

    /// Currency of the bond.
    pub currency: Currency,

    /// Pre-calculated analytics (caller provides).
    pub analytics: HoldingAnalytics,

    /// Classification metadata.
    pub classification: Classification,
}

impl Holding {
    /// Creates a new holding builder.
    #[must_use]
    pub fn builder() -> HoldingBuilder {
        HoldingBuilder::new()
    }

    /// Returns the market value in the bond's local currency.
    #[must_use]
    pub fn market_value_local(&self) -> Decimal {
        self.par_amount * self.market_price / Decimal::ONE_HUNDRED
    }

    /// Returns the market value in the portfolio's base currency.
    #[must_use]
    pub fn market_value(&self) -> Decimal {
        self.market_value_local() * self.fx_rate
    }

    /// Returns the accrued interest amount in local currency.
    #[must_use]
    pub fn accrued_amount_local(&self) -> Decimal {
        self.par_amount * self.accrued_interest / Decimal::ONE_HUNDRED
    }

    /// Returns the accrued interest amount in base currency.
    #[must_use]
    pub fn accrued_amount(&self) -> Decimal {
        self.accrued_amount_local() * self.fx_rate
    }

    /// Returns the total value (market value + accrued) in local currency.
    #[must_use]
    pub fn total_value_local(&self) -> Decimal {
        self.market_value_local() + self.accrued_amount_local()
    }

    /// Returns the total value (market value + accrued) in base currency.
    #[must_use]
    pub fn total_value(&self) -> Decimal {
        self.total_value_local() * self.fx_rate
    }

    /// Returns the dirty price (clean price + accrued).
    #[must_use]
    pub fn dirty_price(&self) -> Decimal {
        self.market_price + self.accrued_interest
    }

    /// Returns true if this holding is in the portfolio's base currency.
    #[must_use]
    pub fn is_base_currency(&self) -> bool {
        self.fx_rate == Decimal::ONE
    }

    /// Returns the weight of this holding for the given weighting method.
    ///
    /// For MarketValue, returns the market value.
    /// For ParValue, returns the par amount.
    /// For EqualWeight, returns 1.0.
    #[must_use]
    pub fn weight_value(&self, method: WeightingMethod) -> Decimal {
        match method {
            WeightingMethod::MarketValue => self.market_value(),
            WeightingMethod::ParValue => self.par_amount * self.fx_rate,
            WeightingMethod::EqualWeight => Decimal::ONE,
        }
    }

    /// Returns the total DV01 for this holding (DV01 per par × par amount).
    #[must_use]
    pub fn total_dv01(&self) -> Option<Decimal> {
        self.analytics.dv01.map(|dv01_per_par| {
            let dv01 = Decimal::from_f64_retain(dv01_per_par).unwrap_or(Decimal::ZERO);
            dv01 * self.par_amount / Decimal::ONE_HUNDRED * self.fx_rate
        })
    }
}

/// Builder for constructing a Holding.
#[derive(Debug, Clone, Default)]
pub struct HoldingBuilder {
    id: Option<String>,
    identifiers: Option<BondIdentifiers>,
    par_amount: Option<Decimal>,
    market_price: Option<Decimal>,
    accrued_interest: Decimal,
    fx_rate: Decimal,
    currency: Currency,
    analytics: HoldingAnalytics,
    classification: Classification,
}

impl HoldingBuilder {
    /// Creates a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fx_rate: Decimal::ONE,
            accrued_interest: Decimal::ZERO,
            currency: Currency::USD,
            ..Self::default()
        }
    }

    /// Sets the holding ID.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the bond identifiers.
    #[must_use]
    pub fn identifiers(mut self, identifiers: BondIdentifiers) -> Self {
        self.identifiers = Some(identifiers);
        self
    }

    /// Sets the par amount.
    #[must_use]
    pub fn par_amount(mut self, amount: Decimal) -> Self {
        self.par_amount = Some(amount);
        self
    }

    /// Sets the market price (clean, as % of par).
    #[must_use]
    pub fn market_price(mut self, price: Decimal) -> Self {
        self.market_price = Some(price);
        self
    }

    /// Sets the accrued interest per 100 par.
    #[must_use]
    pub fn accrued_interest(mut self, accrued: Decimal) -> Self {
        self.accrued_interest = accrued;
        self
    }

    /// Sets the FX rate to base currency.
    #[must_use]
    pub fn fx_rate(mut self, rate: Decimal) -> Self {
        self.fx_rate = rate;
        self
    }

    /// Sets the currency.
    #[must_use]
    pub fn currency(mut self, currency: Currency) -> Self {
        self.currency = currency;
        self
    }

    /// Sets the pre-calculated analytics.
    #[must_use]
    pub fn analytics(mut self, analytics: HoldingAnalytics) -> Self {
        self.analytics = analytics;
        self
    }

    /// Sets the classification.
    #[must_use]
    pub fn classification(mut self, classification: Classification) -> Self {
        self.classification = classification;
        self
    }

    /// Builds the holding.
    ///
    /// # Errors
    ///
    /// Returns an error if required fields are missing.
    pub fn build(self) -> crate::PortfolioResult<Holding> {
        let id = self
            .id
            .ok_or_else(|| crate::PortfolioError::missing_field("id"))?;

        let identifiers = self
            .identifiers
            .ok_or_else(|| crate::PortfolioError::missing_field("identifiers"))?;

        let par_amount = self
            .par_amount
            .ok_or_else(|| crate::PortfolioError::missing_field("par_amount"))?;

        let market_price = self
            .market_price
            .ok_or_else(|| crate::PortfolioError::missing_field("market_price"))?;

        // Validate par amount
        if par_amount <= Decimal::ZERO {
            return Err(crate::PortfolioError::invalid_holding(
                &id,
                "par_amount must be positive",
            ));
        }

        // Validate market price
        if market_price < Decimal::ZERO {
            return Err(crate::PortfolioError::invalid_holding(
                &id,
                "market_price cannot be negative",
            ));
        }

        // Validate FX rate
        if self.fx_rate <= Decimal::ZERO {
            return Err(crate::PortfolioError::InvalidFxRate {
                currency: self.currency.to_string(),
                rate: self.fx_rate.to_string(),
            });
        }

        Ok(Holding {
            id,
            identifiers,
            par_amount,
            market_price,
            accrued_interest: self.accrued_interest,
            fx_rate: self.fx_rate,
            currency: self.currency,
            analytics: self.analytics,
            classification: self.classification,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn create_test_identifiers() -> BondIdentifiers {
        BondIdentifiers::from_isin_str("US912828Z229").unwrap()
    }

    fn create_test_holding() -> Holding {
        Holding::builder()
            .id("TEST001")
            .identifiers(create_test_identifiers())
            .par_amount(dec!(1_000_000))
            .market_price(dec!(98.50))
            .accrued_interest(dec!(1.25))
            .analytics(
                HoldingAnalytics::new()
                    .with_ytm(0.05)
                    .with_modified_duration(5.0)
                    .with_dv01(0.05),
            )
            .build()
            .unwrap()
    }

    #[test]
    fn test_market_value() {
        let holding = create_test_holding();

        // MV = 1,000,000 × 98.50 / 100 = 985,000
        assert_eq!(holding.market_value_local(), dec!(985_000));
        assert_eq!(holding.market_value(), dec!(985_000)); // No FX conversion
    }

    #[test]
    fn test_accrued_amount() {
        let holding = create_test_holding();

        // Accrued = 1,000,000 × 1.25 / 100 = 12,500
        assert_eq!(holding.accrued_amount_local(), dec!(12_500));
    }

    #[test]
    fn test_total_value() {
        let holding = create_test_holding();

        // Total = 985,000 + 12,500 = 997,500
        assert_eq!(holding.total_value_local(), dec!(997_500));
    }

    #[test]
    fn test_dirty_price() {
        let holding = create_test_holding();

        // Dirty = 98.50 + 1.25 = 99.75
        assert_eq!(holding.dirty_price(), dec!(99.75));
    }

    #[test]
    fn test_fx_conversion() {
        let holding = Holding::builder()
            .id("EUR001")
            .identifiers(create_test_identifiers())
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .currency(Currency::EUR)
            .fx_rate(dec!(1.10)) // 1 EUR = 1.10 USD
            .build()
            .unwrap();

        // Local MV = 1,000,000
        assert_eq!(holding.market_value_local(), dec!(1_000_000));

        // Base MV = 1,000,000 × 1.10 = 1,100,000
        assert_eq!(holding.market_value(), dec!(1_100_000));

        assert!(!holding.is_base_currency());
    }

    #[test]
    fn test_weight_value() {
        let holding = create_test_holding();

        // Market value weight
        assert_eq!(
            holding.weight_value(WeightingMethod::MarketValue),
            dec!(985_000)
        );

        // Par value weight
        assert_eq!(
            holding.weight_value(WeightingMethod::ParValue),
            dec!(1_000_000)
        );

        // Equal weight
        assert_eq!(
            holding.weight_value(WeightingMethod::EqualWeight),
            Decimal::ONE
        );
    }

    #[test]
    fn test_total_dv01() {
        let holding = create_test_holding();

        // DV01 = 0.05 per 100 par × 1,000,000 / 100 = 500
        let dv01 = holding.total_dv01().unwrap();
        assert!((dv01 - dec!(500)).abs() < dec!(0.01));
    }

    #[test]
    fn test_builder_validation() {
        // Missing ID
        let result = Holding::builder()
            .identifiers(create_test_identifiers())
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .build();
        assert!(result.is_err());

        // Negative par
        let result = Holding::builder()
            .id("TEST")
            .identifiers(create_test_identifiers())
            .par_amount(dec!(-1_000_000))
            .market_price(dec!(100))
            .build();
        assert!(result.is_err());

        // Zero FX rate
        let result = Holding::builder()
            .id("TEST")
            .identifiers(create_test_identifiers())
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .fx_rate(Decimal::ZERO)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn test_analytics_best_values() {
        let analytics = HoldingAnalytics::new()
            .with_ytm(0.05)
            .with_ytw(0.045)
            .with_modified_duration(5.0)
            .with_effective_duration(4.8)
            .with_z_spread(100.0)
            .with_oas(95.0);

        // Best yield prefers YTW
        assert_eq!(analytics.best_yield(), Some(0.045));

        // Best duration prefers effective
        assert_eq!(analytics.best_duration(), Some(4.8));

        // Best spread prefers OAS
        assert_eq!(analytics.best_spread(), Some(95.0));
    }

    #[test]
    fn test_analytics_fallback() {
        let analytics = HoldingAnalytics::new()
            .with_ytm(0.05)
            .with_modified_duration(5.0)
            .with_z_spread(100.0);

        // Falls back to YTM when YTW not available
        assert_eq!(analytics.best_yield(), Some(0.05));

        // Falls back to modified when effective not available
        assert_eq!(analytics.best_duration(), Some(5.0));

        // Falls back to Z-spread when OAS not available
        assert_eq!(analytics.best_spread(), Some(100.0));
    }
}
