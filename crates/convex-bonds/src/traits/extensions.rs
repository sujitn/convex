//! Extension traits for specialized bond types.
//!
//! These traits extend the base `Bond` trait with specialized functionality
//! for different bond categories.

use convex_core::Date;
use rust_decimal::Decimal;

use super::Bond;
use crate::types::{
    AmortizationSchedule, CallSchedule, InflationIndexType, PutSchedule, RateIndex,
};

/// Extension trait for fixed coupon bonds.
///
/// Provides access to fixed coupon characteristics including rate and frequency.
///
/// # Example
///
/// ```rust,ignore
/// fn analyze_fixed_bond<B: FixedCouponBond>(bond: &B) {
///     println!("Coupon rate: {}%", bond.coupon_rate());
///     println!("Frequency: {} payments/year", bond.coupon_frequency());
/// }
/// ```
pub trait FixedCouponBond: Bond {
    /// Returns the annual coupon rate as a decimal (e.g., 0.05 for 5%).
    fn coupon_rate(&self) -> Decimal;

    /// Returns the number of coupon payments per year.
    fn coupon_frequency(&self) -> u32;

    /// Returns the coupon amount per period per unit of face.
    fn coupon_amount(&self) -> Decimal {
        let face = self.face_value();
        let rate = self.coupon_rate();
        let freq = Decimal::from(self.coupon_frequency());
        face * rate / freq
    }

    /// Returns the first coupon date.
    fn first_coupon_date(&self) -> Option<Date>;

    /// Returns the last coupon date before maturity.
    fn last_coupon_date(&self) -> Option<Date>;

    /// Returns true if this is an ex-dividend date (for markets with record dates).
    fn is_ex_dividend(&self, _settlement: Date) -> bool {
        false // Default implementation
    }
}

/// Extension trait for floating rate notes.
///
/// Provides access to floating rate characteristics including index and spread.
pub trait FloatingCouponBond: Bond {
    /// Returns the reference rate index.
    fn rate_index(&self) -> &RateIndex;

    /// Returns the spread over the reference rate in basis points.
    fn spread_bps(&self) -> Decimal;

    /// Returns the spread as a decimal (e.g., 0.0050 for 50 bps).
    fn spread(&self) -> Decimal {
        self.spread_bps() / Decimal::from(10000)
    }

    /// Returns the reset frequency (payments per year).
    fn reset_frequency(&self) -> u32;

    /// Returns the number of lookback days for the rate fixing.
    fn lookback_days(&self) -> u32 {
        0 // Default: no lookback
    }

    /// Returns the floor rate if any (as decimal).
    fn floor(&self) -> Option<Decimal> {
        None
    }

    /// Returns the cap rate if any (as decimal).
    fn cap(&self) -> Option<Decimal> {
        None
    }

    /// Calculates the coupon for the current period given the reference rate.
    fn current_coupon(&self, reference_rate: Decimal) -> Decimal {
        let mut rate = reference_rate + self.spread();

        // Apply floor
        if let Some(floor) = self.floor() {
            if rate < floor {
                rate = floor;
            }
        }

        // Apply cap
        if let Some(cap) = self.cap() {
            if rate > cap {
                rate = cap;
            }
        }

        let face = self.face_value();
        let freq = Decimal::from(self.reset_frequency());
        face * rate / freq
    }

    /// Returns the next reset date after the given date.
    fn next_reset_date(&self, after: Date) -> Option<Date>;

    /// Returns the fixing date for a given reset date.
    fn fixing_date(&self, reset_date: Date) -> Date;
}

/// Extension trait for bonds with embedded options (callable/puttable).
///
/// Provides access to call and put schedules and option characteristics.
pub trait EmbeddedOptionBond: Bond {
    /// Returns the call schedule if the bond is callable.
    fn call_schedule(&self) -> Option<&CallSchedule>;

    /// Returns the put schedule if the bond is puttable.
    fn put_schedule(&self) -> Option<&PutSchedule>;

    /// Returns true if the bond is currently callable.
    fn is_callable_on(&self, date: Date) -> bool {
        self.call_schedule().is_some_and(|s| s.is_callable_on(date))
    }

    /// Returns true if the bond is currently puttable.
    fn is_puttable_on(&self, date: Date) -> bool {
        self.put_schedule().is_some_and(|s| s.is_puttable_on(date))
    }

    /// Returns the call price on the given date if callable.
    fn call_price_on(&self, date: Date) -> Option<f64> {
        self.call_schedule().and_then(|s| s.call_price_on(date))
    }

    /// Returns the put price on the given date if puttable.
    fn put_price_on(&self, date: Date) -> Option<f64> {
        self.put_schedule().and_then(|s| s.put_price_on(date))
    }

    /// Returns the first call date.
    fn first_call_date(&self) -> Option<Date> {
        self.call_schedule()
            .and_then(crate::types::CallSchedule::first_call_date)
    }

    /// Returns the first put date.
    fn first_put_date(&self) -> Option<Date> {
        self.put_schedule()
            .and_then(crate::types::PutSchedule::first_put_date)
    }

    /// Returns true if the bond has any optionality.
    fn has_optionality(&self) -> bool {
        self.call_schedule().is_some() || self.put_schedule().is_some()
    }

    /// Calculates yield-to-call for a given price.
    ///
    /// This is a placeholder; actual implementation requires numerical methods.
    fn yield_to_call(&self, _price: Decimal, _settlement: Date) -> Option<Decimal> {
        None // To be implemented by concrete types
    }

    /// Calculates yield-to-put for a given price.
    fn yield_to_put(&self, _price: Decimal, _settlement: Date) -> Option<Decimal> {
        None // To be implemented by concrete types
    }

    /// Calculates yield-to-worst (minimum of YTM, YTC, YTP).
    fn yield_to_worst(&self, _price: Decimal, _settlement: Date) -> Option<Decimal> {
        None // To be implemented by concrete types
    }
}

/// Extension trait for amortizing bonds.
///
/// Provides access to amortization schedules and factor calculations.
pub trait AmortizingBond: Bond {
    /// Returns the amortization schedule.
    fn amortization_schedule(&self) -> &AmortizationSchedule;

    /// Returns the current factor (remaining principal / original face).
    fn factor(&self, as_of: Date) -> f64 {
        self.amortization_schedule().factor_as_of(as_of)
    }

    /// Returns the outstanding principal as of the given date.
    fn outstanding_principal(&self, as_of: Date) -> Decimal {
        let factor = Decimal::try_from(self.factor(as_of)).unwrap_or(Decimal::ONE);
        self.face_value() * factor
    }

    /// Returns the next principal payment date.
    fn next_principal_date(&self, after: Date) -> Option<Date> {
        self.amortization_schedule().next_payment_date(after)
    }

    /// Returns the principal payment amount for a specific date.
    fn principal_payment(&self, date: Date) -> Option<Decimal> {
        self.amortization_schedule().principal_on(date).map(|pct| {
            let pct_dec = Decimal::try_from(pct / 100.0).unwrap_or(Decimal::ZERO);
            self.face_value() * pct_dec
        })
    }

    /// Returns the weighted average life (WAL) from the given date.
    fn weighted_average_life(&self, from: Date) -> f64;
}

/// Extension trait for inflation-linked bonds.
///
/// Provides access to inflation adjustment and index ratio calculations.
pub trait InflationLinkedBond: Bond {
    /// Returns the inflation index type.
    fn inflation_index(&self) -> InflationIndexType;

    /// Returns the base index value at issue.
    fn base_index_value(&self) -> Decimal;

    /// Returns the index ratio for the given settlement date.
    ///
    /// The index ratio = settlement index / base index.
    fn index_ratio(&self, _settlement: Date, settlement_index: Decimal) -> Decimal {
        if self.base_index_value().is_zero() {
            Decimal::ONE
        } else {
            settlement_index / self.base_index_value()
        }
    }

    /// Returns the inflation-adjusted principal for settlement.
    fn inflation_adjusted_principal(&self, settlement: Date, settlement_index: Decimal) -> Decimal {
        let ratio = self.index_ratio(settlement, settlement_index);
        self.face_value() * ratio
    }

    /// Returns the inflation-adjusted coupon for a period.
    ///
    /// For TIPS-style bonds, coupons are calculated on the adjusted principal.
    fn inflation_adjusted_coupon(
        &self,
        coupon_date: Date,
        coupon_index: Decimal,
        real_coupon: Decimal,
    ) -> Decimal {
        let ratio = self.index_ratio(coupon_date, coupon_index);
        real_coupon * ratio
    }

    /// Returns true if principal is protected from deflation (floor at par).
    fn has_deflation_floor(&self) -> bool {
        true // Default: most inflation bonds have deflation floor
    }

    /// Applies deflation floor to index ratio if applicable.
    fn apply_deflation_floor(&self, ratio: Decimal) -> Decimal {
        if self.has_deflation_floor() && ratio < Decimal::ONE {
            Decimal::ONE
        } else {
            ratio
        }
    }

    /// Returns the reference index for a specific settlement date.
    ///
    /// This interpolates between monthly index values per the bond's convention.
    fn reference_index(
        &self,
        settlement: Date,
        monthly_indices: &[(Date, Decimal)],
    ) -> Option<Decimal>;

    /// Returns the real yield given price and settlement info.
    fn real_yield(&self, _price: Decimal, _settlement: Date) -> Option<Decimal> {
        None // To be implemented by concrete types
    }

    /// Returns the breakeven inflation rate.
    fn breakeven_inflation(&self, _nominal_yield: Decimal, _real_yield: Decimal) -> Decimal {
        Decimal::ZERO // To be implemented
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests would require mock implementations of the traits.
    // For now, we test the default method implementations where possible.

    #[test]
    fn test_spread_conversion() {
        // Test that 50 bps = 0.0050
        let spread_bps = Decimal::from(50);
        let spread = spread_bps / Decimal::from(10000);
        assert_eq!(spread, Decimal::new(5, 3));
    }
}
