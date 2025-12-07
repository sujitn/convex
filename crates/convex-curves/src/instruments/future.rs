//! Rate futures (SOFR, Eurodollar) for curve construction.
//!
//! Futures are exchange-traded and highly liquid, making them important
//! for curve construction, especially in the 1-2 year range.

use convex_core::Date;

use super::{year_fraction_act360, CurveInstrument, InstrumentType};
use crate::error::CurveResult;
use crate::traits::Curve;

/// Type of rate future.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FutureType {
    /// 1-month SOFR futures (CME)
    SOFR1M,
    /// 3-month SOFR futures (CME)
    SOFR3M,
    /// 3-month Eurodollar futures (legacy, CME)
    Eurodollar,
    /// 3-month SONIA futures (ICE)
    SONIA3M,
    /// 3-month EURIBOR futures (Eurex)
    EURIBOR3M,
}

impl FutureType {
    /// Returns the tenor in months.
    #[must_use]
    pub fn tenor_months(&self) -> u32 {
        match self {
            Self::SOFR1M => 1,
            Self::SOFR3M | Self::Eurodollar | Self::SONIA3M | Self::EURIBOR3M => 3,
        }
    }

    /// Returns the contract name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::SOFR1M => "SOFR 1M",
            Self::SOFR3M => "SOFR 3M",
            Self::Eurodollar => "Eurodollar",
            Self::SONIA3M => "SONIA 3M",
            Self::EURIBOR3M => "EURIBOR 3M",
        }
    }
}

impl std::fmt::Display for FutureType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// A rate future contract.
///
/// Rate futures are used for curve construction in the 1-5 year range
/// where they provide the most liquid market quotes.
///
/// # Pricing Convention
///
/// Futures are quoted as: `Price = 100 - Rate`
///
/// Example: Price 94.75 implies Rate = 5.25%
///
/// # Convexity Adjustment
///
/// Futures rates differ from forward rates due to daily margining.
/// The convexity adjustment converts the futures rate to a forward rate:
///
/// ```text
/// Forward Rate = Futures Rate - Convexity Adjustment
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::instruments::{RateFuture, FutureType};
///
/// // December 2025 SOFR 3M future at 94.75 (5.25% implied rate)
/// let future = RateFuture::new(
///     FutureType::SOFR3M,
///     imm_date,           // Third Wednesday of Dec 2025
///     accrual_start,
///     accrual_end,
///     94.75,
/// );
/// ```
#[derive(Debug, Clone)]
pub struct RateFuture {
    /// Future type
    future_type: FutureType,
    /// Last trading date (typically IMM date)
    last_trading_date: Date,
    /// Accrual period start
    accrual_start: Date,
    /// Accrual period end
    accrual_end: Date,
    /// Price (e.g., 94.75)
    price: f64,
    /// Convexity adjustment (subtract from implied rate)
    convexity_adjustment: f64,
    /// Notional (typically $1M for SOFR)
    notional: f64,
}

impl RateFuture {
    /// Creates a new rate future.
    ///
    /// # Arguments
    ///
    /// * `future_type` - Type of futures contract
    /// * `last_trading_date` - Last trading/settlement date (IMM date)
    /// * `accrual_start` - Start of accrual period
    /// * `accrual_end` - End of accrual period
    /// * `price` - Quoted price (e.g., 94.75)
    #[must_use]
    pub fn new(
        future_type: FutureType,
        last_trading_date: Date,
        accrual_start: Date,
        accrual_end: Date,
        price: f64,
    ) -> Self {
        Self {
            future_type,
            last_trading_date,
            accrual_start,
            accrual_end,
            price,
            convexity_adjustment: 0.0,
            notional: 1_000_000.0,
        }
    }

    /// Creates a future with convexity adjustment.
    #[must_use]
    pub fn with_convexity_adjustment(mut self, adjustment: f64) -> Self {
        self.convexity_adjustment = adjustment;
        self
    }

    /// Creates a future with specified notional.
    #[must_use]
    pub fn with_notional(mut self, notional: f64) -> Self {
        self.notional = notional;
        self
    }

    /// Returns the future type.
    #[must_use]
    pub fn future_type(&self) -> FutureType {
        self.future_type
    }

    /// Returns the last trading date.
    #[must_use]
    pub fn last_trading_date(&self) -> Date {
        self.last_trading_date
    }

    /// Returns the accrual start date.
    #[must_use]
    pub fn accrual_start(&self) -> Date {
        self.accrual_start
    }

    /// Returns the accrual end date.
    #[must_use]
    pub fn accrual_end(&self) -> Date {
        self.accrual_end
    }

    /// Returns the quoted price.
    #[must_use]
    pub fn price(&self) -> f64 {
        self.price
    }

    /// Returns the convexity adjustment.
    #[must_use]
    pub fn convexity_adjustment(&self) -> f64 {
        self.convexity_adjustment
    }

    /// Returns the raw implied rate from the price.
    ///
    /// Rate = (100 - Price) / 100
    #[must_use]
    pub fn implied_rate_raw(&self) -> f64 {
        (100.0 - self.price) / 100.0
    }

    /// Returns the forward rate after convexity adjustment.
    ///
    /// Forward = Implied Rate - Convexity Adjustment
    #[must_use]
    pub fn implied_forward_rate(&self) -> f64 {
        self.implied_rate_raw() - self.convexity_adjustment
    }

    /// Returns the accrual period year fraction.
    #[must_use]
    pub fn year_fraction(&self) -> f64 {
        year_fraction_act360(self.accrual_start, self.accrual_end)
    }
}

impl CurveInstrument for RateFuture {
    fn maturity(&self) -> Date {
        self.accrual_end
    }

    fn pillar_date(&self) -> Date {
        self.accrual_end
    }

    fn pv(&self, curve: &dyn Curve) -> CurveResult<f64> {
        // Compare implied forward to curve forward
        let ref_date = curve.reference_date();
        let t_start = year_fraction_act360(ref_date, self.accrual_start);
        let t_end = year_fraction_act360(ref_date, self.accrual_end);

        let df_start = curve.discount_factor(t_start)?;
        let df_end = curve.discount_factor(t_end)?;

        let tau = self.year_fraction();
        let curve_fwd = if tau > 0.0 && df_end > 0.0 {
            (df_start / df_end - 1.0) / tau
        } else {
            0.0
        };

        let implied_fwd = self.implied_forward_rate();

        // PV represents the mismatch
        Ok(self.notional * tau * (curve_fwd - implied_fwd))
    }

    fn implied_df(&self, curve: &dyn Curve, _target_pv: f64) -> CurveResult<f64> {
        // Solve: (DF(start)/DF(end) - 1) / τ = forward_rate
        // DF(end) = DF(start) / (1 + forward_rate × τ)
        let ref_date = curve.reference_date();
        let t_start = year_fraction_act360(ref_date, self.accrual_start);

        let df_start = curve.discount_factor(t_start)?;
        let tau = self.year_fraction();
        let fwd = self.implied_forward_rate();

        Ok(df_start / (1.0 + fwd * tau))
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::Future
    }

    fn description(&self) -> String {
        format!(
            "{} {} @ {:.2} ({:.4}%)",
            self.future_type,
            self.last_trading_date,
            self.price,
            self.implied_forward_rate() * 100.0
        )
    }
}

/// Calculates the IMM date (third Wednesday) for a given month.
///
/// IMM dates are the standard expiry dates for rate futures.
#[must_use]
pub fn imm_date(year: i32, month: u32) -> Option<Date> {
    // Start from the first of the month
    let first = Date::from_ymd(year, month, 1).ok()?;

    // Find the first Wednesday
    // chrono::Weekday: Monday=0, Tuesday=1, Wednesday=2, etc.
    let weekday = i64::from(first.weekday().num_days_from_monday());
    let days_to_wed = (2 - weekday + 7) % 7; // Wednesday = 2 in chrono

    // Third Wednesday = first Wednesday + 14 days
    let third_wed = first.add_days(days_to_wed + 14);
    Some(third_wed)
}

/// Returns the next IMM dates from a given date.
#[must_use]
pub fn next_imm_dates(from: Date, count: usize) -> Vec<Date> {
    let mut dates = Vec::with_capacity(count);
    let mut year = from.year();
    let month = from.month();

    // IMM months: March (3), June (6), September (9), December (12)
    let imm_months = [3, 6, 9, 12];

    // Find the next IMM month
    let mut imm_idx = imm_months
        .iter()
        .position(|&m| m as u32 > month)
        .unwrap_or(0);

    if imm_idx == 0 && month >= 12 {
        year += 1;
    }

    while dates.len() < count {
        let imm_month = imm_months[imm_idx] as u32;
        if let Some(date) = imm_date(year, imm_month) {
            if date > from {
                dates.push(date);
            }
        }

        imm_idx += 1;
        if imm_idx >= imm_months.len() {
            imm_idx = 0;
            year += 1;
        }
    }

    dates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscountCurveBuilder;
    use crate::interpolation::InterpolationMethod;
    use approx::assert_relative_eq;

    #[test]
    fn test_future_implied_rate() {
        let future = RateFuture::new(
            FutureType::SOFR3M,
            Date::from_ymd(2025, 3, 19).unwrap(),
            Date::from_ymd(2025, 3, 19).unwrap(),
            Date::from_ymd(2025, 6, 18).unwrap(),
            94.75,
        );

        // Price 94.75 → Rate 5.25%
        assert_relative_eq!(future.implied_rate_raw(), 0.0525, epsilon = 1e-10);
    }

    #[test]
    fn test_future_with_convexity_adjustment() {
        let future = RateFuture::new(
            FutureType::SOFR3M,
            Date::from_ymd(2025, 3, 19).unwrap(),
            Date::from_ymd(2025, 3, 19).unwrap(),
            Date::from_ymd(2025, 6, 18).unwrap(),
            94.75,
        )
        .with_convexity_adjustment(0.0005); // 5bp adjustment

        // Forward = 5.25% - 0.05% = 5.20%
        assert_relative_eq!(future.implied_forward_rate(), 0.0520, epsilon = 1e-10);
    }

    #[test]
    fn test_future_type() {
        assert_eq!(FutureType::SOFR1M.tenor_months(), 1);
        assert_eq!(FutureType::SOFR3M.tenor_months(), 3);
        assert_eq!(FutureType::Eurodollar.tenor_months(), 3);
    }

    #[test]
    fn test_imm_date() {
        // March 2025: 3rd Wednesday is March 19
        let date = imm_date(2025, 3).unwrap();
        assert_eq!(date.day(), 19);

        // June 2025: 3rd Wednesday is June 18
        let date = imm_date(2025, 6).unwrap();
        assert_eq!(date.day(), 18);
    }

    #[test]
    fn test_next_imm_dates() {
        let from = Date::from_ymd(2025, 1, 15).unwrap();
        let dates = next_imm_dates(from, 4);

        assert_eq!(dates.len(), 4);
        // March, June, September, December 2025
        assert_eq!(dates[0].month(), 3);
        assert_eq!(dates[1].month(), 6);
        assert_eq!(dates[2].month(), 9);
        assert_eq!(dates[3].month(), 12);
    }

    #[test]
    fn test_future_implied_df() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let future = RateFuture::new(
            FutureType::SOFR3M,
            Date::from_ymd(2025, 3, 19).unwrap(),
            Date::from_ymd(2025, 3, 19).unwrap(),
            Date::from_ymd(2025, 6, 18).unwrap(),
            95.00, // 5% implied rate
        );

        let curve = DiscountCurveBuilder::new(ref_date)
            .add_zero_rate(0.25, 0.05)
            .add_zero_rate(1.0, 0.05)
            .with_interpolation(InterpolationMethod::LogLinear)
            .with_extrapolation()
            .build()
            .unwrap();

        let implied = future.implied_df(&curve, 0.0).unwrap();

        // Should be positive and less than DF at accrual start
        let t_start = year_fraction_act360(ref_date, future.accrual_start());
        let df_start = curve.discount_factor(t_start).unwrap();

        assert!(implied > 0.0);
        assert!(implied < df_start);
    }
}
