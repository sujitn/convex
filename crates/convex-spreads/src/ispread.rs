//! I-Spread (Interpolated Spread) calculation.
//!
//! The I-spread is the spread of a bond's yield to maturity over the interpolated
//! swap rate at the same maturity. It provides a measure of credit spread relative
//! to the swap curve.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_spreads::ispread::ISpreadCalculator;
//! use convex_curves::curves::ZeroCurve;
//!
//! let curve = // ... create swap curve
//! let calculator = ISpreadCalculator::new(&curve);
//!
//! // Calculate I-spread from bond yield
//! let i_spread = calculator.calculate(&bond, settlement)?;
//!
//! // Price bond with a given I-spread
//! let price = calculator.price_with_spread(&bond, dec!(0.015), settlement)?;
//!
//! // Calculate spread DV01
//! let dv01 = calculator.spread_dv01(&bond, settlement)?;
//! ```

use rust_decimal::Decimal;

use convex_bonds::instruments::{Bond, FixedBond};
use convex_core::types::{Date, Price, Spread, SpreadType};
use convex_curves::curves::ZeroCurve;

use crate::error::{SpreadError, SpreadResult};

/// I-Spread calculator.
///
/// Calculates the interpolated spread (I-spread) for a bond. The I-spread
/// is the difference between a bond's yield to maturity and the interpolated
/// swap rate at the bond's maturity.
///
/// # Bloomberg Methodology
///
/// ```text
/// I-Spread = Bond YTM - Swap Rate(maturity)
/// ```
///
/// where the swap rate is linearly interpolated from the swap curve at the
/// bond's exact maturity date.
///
/// Unlike Z-spread which is added to every point on the curve, I-spread is a
/// simple yield difference and thus represents a "parallel" measure of spread.
#[derive(Debug, Clone)]
pub struct ISpreadCalculator<'a> {
    /// Reference to the swap/zero curve.
    swap_curve: &'a ZeroCurve,
    /// Tolerance for yield calculations.
    tolerance: f64,
    /// Maximum iterations for solvers.
    max_iterations: usize,
}

impl<'a> ISpreadCalculator<'a> {
    /// Creates a new I-spread calculator.
    ///
    /// # Arguments
    ///
    /// * `swap_curve` - The swap rate curve to use for spread calculation
    #[must_use]
    pub fn new(swap_curve: &'a ZeroCurve) -> Self {
        Self {
            swap_curve,
            tolerance: 1e-10,
            max_iterations: 100,
        }
    }

    /// Sets the solver tolerance.
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Sets the maximum iterations for the solver.
    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// Calculates I-spread for a bond given its yield to maturity.
    ///
    /// # Arguments
    ///
    /// * `bond` - The fixed-rate bond
    /// * `bond_yield` - The bond's yield to maturity (as decimal, e.g., 0.05 for 5%)
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The I-spread as a `Spread` in basis points.
    pub fn calculate(
        &self,
        bond: &FixedBond,
        bond_yield: Decimal,
        settlement: Date,
    ) -> SpreadResult<Spread> {
        let maturity = bond.maturity();

        if settlement >= maturity {
            return Err(SpreadError::SettlementAfterMaturity {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        self.calculate_from_yield(bond_yield, maturity)
    }

    /// Calculates I-spread directly from a yield and maturity date.
    ///
    /// This is useful when you already have the bond yield and maturity
    /// without needing the full bond object.
    ///
    /// # Arguments
    ///
    /// * `bond_yield` - The bond's yield to maturity (as decimal)
    /// * `maturity` - The bond's maturity date
    ///
    /// # Returns
    ///
    /// The I-spread in basis points.
    pub fn calculate_from_yield(
        &self,
        bond_yield: Decimal,
        maturity: Date,
    ) -> SpreadResult<Spread> {
        // Get swap rate at bond's maturity
        let swap_rate = self
            .swap_curve
            .zero_rate_at(maturity)
            .map_err(|e| SpreadError::curve_error(e.to_string()))?;

        // I-spread = Bond yield - Swap rate
        let spread = bond_yield - swap_rate;
        let spread_bps = (spread * Decimal::from(10_000)).round();

        Ok(Spread::new(spread_bps, SpreadType::ISpread))
    }

    /// Calculates I-spread for a bond given its market price.
    ///
    /// First calculates the yield to maturity from the price, then computes
    /// the I-spread from that yield.
    ///
    /// # Arguments
    ///
    /// * `bond` - The fixed-rate bond
    /// * `market_price` - Market clean price
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The I-spread in basis points.
    pub fn calculate_from_price(
        &self,
        bond: &FixedBond,
        market_price: Price,
        settlement: Date,
    ) -> SpreadResult<Spread> {
        use convex_bonds::pricing::BondPricer;

        let maturity = bond.maturity();

        if settlement >= maturity {
            return Err(SpreadError::SettlementAfterMaturity {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        // Calculate YTM from price
        let ytm = BondPricer::yield_to_maturity(bond, market_price, settlement)
            .map_err(|e| SpreadError::bond_error(e.to_string()))?;

        self.calculate_from_yield(ytm, maturity)
    }

    /// Prices a bond given an I-spread.
    ///
    /// Calculates the implied yield (swap rate + I-spread) at the bond's
    /// maturity, then prices the bond at that yield.
    ///
    /// # Arguments
    ///
    /// * `bond` - The fixed-rate bond
    /// * `i_spread` - I-spread as decimal (e.g., 0.015 for 150 bps)
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The clean price of the bond.
    pub fn price_with_spread(
        &self,
        bond: &FixedBond,
        i_spread: Decimal,
        settlement: Date,
    ) -> SpreadResult<Price> {
        use convex_bonds::pricing::BondPricer;

        let maturity = bond.maturity();

        if settlement >= maturity {
            return Err(SpreadError::SettlementAfterMaturity {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        // Get swap rate at maturity
        let swap_rate = self
            .swap_curve
            .zero_rate_at(maturity)
            .map_err(|e| SpreadError::curve_error(e.to_string()))?;

        // Implied yield = swap rate + I-spread
        let implied_yield = swap_rate + i_spread;

        // Price bond at the implied yield
        let price_result = BondPricer::price_from_yield(bond, implied_yield, settlement)
            .map_err(|e| SpreadError::bond_error(e.to_string()))?;

        Ok(price_result.clean_price)
    }

    /// Calculates spread DV01 (price sensitivity to 1bp spread change).
    ///
    /// This measures how much the price changes for a 1 basis point
    /// increase in the I-spread.
    ///
    /// # Arguments
    ///
    /// * `bond` - The fixed-rate bond
    /// * `i_spread` - Current I-spread as decimal
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The price change (in dollars per 100 face) for a 1 basis point increase in spread.
    pub fn spread_dv01(
        &self,
        bond: &FixedBond,
        i_spread: Decimal,
        settlement: Date,
    ) -> SpreadResult<Decimal> {
        let bump = Decimal::new(1, 4); // 1 bp = 0.0001

        let price_base = self.price_with_spread(bond, i_spread, settlement)?;
        let price_up = self.price_with_spread(bond, i_spread + bump, settlement)?;

        // DV01 is positive (price decrease for spread increase)
        Ok(price_base.as_percentage() - price_up.as_percentage())
    }

    /// Calculates spread duration (percentage price sensitivity).
    ///
    /// Spread duration measures the percentage change in price for a
    /// 100 basis point change in spread.
    ///
    /// # Arguments
    ///
    /// * `bond` - The fixed-rate bond
    /// * `i_spread` - Current I-spread as decimal
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// Spread duration = (DV01 / Price) * 10000
    pub fn spread_duration(
        &self,
        bond: &FixedBond,
        i_spread: Decimal,
        settlement: Date,
    ) -> SpreadResult<Decimal> {
        let price = self.price_with_spread(bond, i_spread, settlement)?;
        let price_decimal = price.as_percentage();

        if price_decimal <= Decimal::ZERO {
            return Ok(Decimal::ZERO);
        }

        let dv01 = self.spread_dv01(bond, i_spread, settlement)?;

        // Duration = (DV01 / Price) * 10000
        Ok(dv01 / price_decimal * Decimal::from(10_000))
    }

    /// Gets the swap rate at the bond's maturity.
    ///
    /// # Arguments
    ///
    /// * `maturity` - The maturity date
    ///
    /// # Returns
    ///
    /// The interpolated swap rate at maturity.
    pub fn swap_rate_at_maturity(&self, maturity: Date) -> SpreadResult<Decimal> {
        self.swap_curve
            .zero_rate_at(maturity)
            .map_err(|e| SpreadError::curve_error(e.to_string()))
    }
}

/// Calculates I-spread for a bond.
///
/// This is a convenience function that wraps `ISpreadCalculator`.
///
/// # Arguments
///
/// * `bond` - The bond to calculate spread for
/// * `swap_curve` - The swap rate curve
/// * `bond_yield` - The bond's yield to maturity
/// * `settlement` - Settlement date
///
/// # Returns
///
/// The I-spread in basis points.
pub fn calculate(
    bond: &FixedBond,
    swap_curve: &ZeroCurve,
    bond_yield: Decimal,
    settlement: Date,
) -> SpreadResult<Spread> {
    ISpreadCalculator::new(swap_curve).calculate(bond, bond_yield, settlement)
}

/// Calculates I-spread from price.
///
/// Convenience function that first calculates YTM from price, then I-spread.
///
/// # Arguments
///
/// * `bond` - The bond to calculate spread for
/// * `swap_curve` - The swap rate curve
/// * `market_price` - Market clean price
/// * `settlement` - Settlement date
///
/// # Returns
///
/// The I-spread in basis points.
pub fn calculate_from_price(
    bond: &FixedBond,
    swap_curve: &ZeroCurve,
    market_price: Price,
    settlement: Date,
) -> SpreadResult<Spread> {
    ISpreadCalculator::new(swap_curve).calculate_from_price(bond, market_price, settlement)
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_bonds::instruments::FixedBondBuilder;
    use convex_core::types::{Currency, Frequency};
    use convex_curves::prelude::{InterpolationMethod, ZeroCurveBuilder};
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_swap_curve() -> ZeroCurve {
        // Create a simple upward-sloping swap curve
        ZeroCurveBuilder::new()
            .reference_date(date(2025, 1, 15))
            .add_rate(date(2025, 4, 15), dec!(0.040)) // 3M: 4.0%
            .add_rate(date(2025, 7, 15), dec!(0.042)) // 6M: 4.2%
            .add_rate(date(2026, 1, 15), dec!(0.045)) // 1Y: 4.5%
            .add_rate(date(2027, 1, 15), dec!(0.047)) // 2Y: 4.7%
            .add_rate(date(2028, 1, 15), dec!(0.050)) // 3Y: 5.0%
            .add_rate(date(2030, 1, 15), dec!(0.052)) // 5Y: 5.2%
            .add_rate(date(2035, 1, 15), dec!(0.055)) // 10Y: 5.5%
            .interpolation(InterpolationMethod::Linear)
            .build()
            .unwrap()
    }

    fn create_test_bond() -> FixedBond {
        // 5% semi-annual bond maturing in 3 years
        FixedBondBuilder::new()
            .isin("TEST123")
            .face_value(dec!(100))
            .coupon_rate(dec!(0.05))
            .maturity(date(2028, 1, 15))
            .issue_date(date(2025, 1, 15))
            .first_coupon_date(date(2025, 7, 15))
            .frequency(Frequency::SemiAnnual)
            .day_count("30/360")
            .currency(Currency::USD)
            .build()
            .unwrap()
    }

    #[test]
    fn test_ispread_calculator_creation() {
        let curve = create_swap_curve();
        let calc = ISpreadCalculator::new(&curve)
            .with_tolerance(1e-8)
            .with_max_iterations(50);

        assert!(calc.tolerance < 1e-7);
        assert_eq!(calc.max_iterations, 50);
    }

    #[test]
    fn test_ispread_from_yield() {
        let curve = create_swap_curve();
        let calc = ISpreadCalculator::new(&curve);
        let bond = create_test_bond();
        let settlement = date(2025, 1, 15);

        // Bond yield of 5.5% with swap rate of 5.0% at 3Y maturity
        // Expected I-spread = 0.5% = 50 bps
        let bond_yield = dec!(0.055);
        let i_spread = calc.calculate(&bond, bond_yield, settlement).unwrap();

        let spread_bps = i_spread.as_bps().to_f64().unwrap();
        assert!(
            (spread_bps - 50.0).abs() < 1.0,
            "Expected ~50 bps, got {} bps",
            spread_bps
        );
    }

    #[test]
    fn test_ispread_from_price() {
        let curve = create_swap_curve();
        let calc = ISpreadCalculator::new(&curve);
        let bond = create_test_bond();
        let settlement = date(2025, 1, 15);

        // Price at par
        let price = Price::new(dec!(100), Currency::USD);
        let result = calc.calculate_from_price(&bond, price, settlement);

        // Should be able to calculate I-spread from price
        assert!(result.is_ok());
    }

    #[test]
    fn test_price_with_spread() {
        let curve = create_swap_curve();
        let calc = ISpreadCalculator::new(&curve);
        let bond = create_test_bond();
        let settlement = date(2025, 1, 15);

        // Price with zero spread
        let price_zero = calc
            .price_with_spread(&bond, Decimal::ZERO, settlement)
            .unwrap();
        assert!(
            price_zero.as_percentage() > dec!(90) && price_zero.as_percentage() < dec!(110)
        );

        // Price with positive spread should be lower
        let price_100bps = calc
            .price_with_spread(&bond, dec!(0.01), settlement)
            .unwrap();
        assert!(price_100bps.as_percentage() < price_zero.as_percentage());

        // Price with negative spread should be higher
        let price_neg = calc
            .price_with_spread(&bond, dec!(-0.01), settlement)
            .unwrap();
        assert!(price_neg.as_percentage() > price_zero.as_percentage());
    }

    #[test]
    fn test_spread_dv01() {
        let curve = create_swap_curve();
        let calc = ISpreadCalculator::new(&curve);
        let bond = create_test_bond();
        let settlement = date(2025, 1, 15);

        let dv01 = calc.spread_dv01(&bond, dec!(0.01), settlement).unwrap();

        // DV01 should be positive and reasonable
        assert!(dv01 > Decimal::ZERO);
        assert!(dv01 < dec!(0.1)); // Less than 10 cents per 100 face for 1bp
    }

    #[test]
    fn test_spread_duration() {
        let curve = create_swap_curve();
        let calc = ISpreadCalculator::new(&curve);
        let bond = create_test_bond();
        let settlement = date(2025, 1, 15);

        let duration = calc
            .spread_duration(&bond, dec!(0.01), settlement)
            .unwrap();

        // Duration should be positive and reasonable (around 2.7 for 3Y bond)
        assert!(duration > Decimal::ZERO);
        assert!(duration < dec!(10));
    }

    #[test]
    fn test_roundtrip_spread_price() {
        let curve = create_swap_curve();
        let calc = ISpreadCalculator::new(&curve);
        let bond = create_test_bond();
        let settlement = date(2025, 1, 15);

        // Test at various spread levels
        for spread_bps in [0i64, 50, 100, 150, 200] {
            let spread_decimal = Decimal::new(spread_bps, 4); // Convert bps to decimal

            // Price with spread
            let price = calc
                .price_with_spread(&bond, spread_decimal, settlement)
                .unwrap();

            // Calculate I-spread from that price
            let calculated_spread = calc
                .calculate_from_price(&bond, price, settlement)
                .unwrap();

            let calculated_bps = calculated_spread.as_bps().to_f64().unwrap();
            let expected_bps = spread_bps as f64;

            assert!(
                (calculated_bps - expected_bps).abs() < 1.0,
                "Spread {}: expected {} bps, got {} bps",
                spread_bps,
                expected_bps,
                calculated_bps
            );
        }
    }

    #[test]
    fn test_swap_rate_at_maturity() {
        let curve = create_swap_curve();
        let calc = ISpreadCalculator::new(&curve);

        // 3Y point should be 5.0%
        let rate_3y = calc.swap_rate_at_maturity(date(2028, 1, 15)).unwrap();
        assert!(
            (rate_3y - dec!(0.05)).abs() < dec!(0.001),
            "Expected ~5.0%, got {}",
            rate_3y
        );
    }

    #[test]
    fn test_settlement_validation() {
        let curve = create_swap_curve();
        let calc = ISpreadCalculator::new(&curve);
        let bond = create_test_bond();

        // Settlement after maturity should fail
        let settlement = date(2030, 1, 15);
        let result = calc.calculate(&bond, dec!(0.05), settlement);

        assert!(result.is_err());
        match result {
            Err(SpreadError::SettlementAfterMaturity { .. }) => {}
            _ => panic!("Expected SettlementAfterMaturity error"),
        }
    }

    #[test]
    fn test_convenience_functions() {
        let curve = create_swap_curve();
        let bond = create_test_bond();
        let settlement = date(2025, 1, 15);

        // Test calculate() convenience function
        let spread = calculate(&bond, &curve, dec!(0.055), settlement).unwrap();
        assert!(spread.as_bps() > Decimal::ZERO);

        // Test calculate_from_price() convenience function
        let price = Price::new(dec!(100), Currency::USD);
        let spread = calculate_from_price(&bond, &curve, price, settlement).unwrap();
        assert!(spread.spread_type() == SpreadType::ISpread);
    }

    #[test]
    fn test_negative_ispread() {
        let curve = create_swap_curve();
        let calc = ISpreadCalculator::new(&curve);
        let bond = create_test_bond();
        let settlement = date(2025, 1, 15);

        // Bond yield below swap rate should give negative I-spread
        // Swap rate at 3Y is 5.0%, so yield of 4.5% gives -50 bps
        let bond_yield = dec!(0.045);
        let i_spread = calc.calculate(&bond, bond_yield, settlement).unwrap();

        let spread_bps = i_spread.as_bps().to_f64().unwrap();
        assert!(
            spread_bps < 0.0,
            "Expected negative spread, got {} bps",
            spread_bps
        );
        assert!(
            (spread_bps - (-50.0)).abs() < 1.0,
            "Expected ~-50 bps, got {} bps",
            spread_bps
        );
    }
}
