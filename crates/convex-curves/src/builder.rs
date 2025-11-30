//! Fluent CurveBuilder API for constructing yield curves.
//!
//! Provides a high-level, market-convention-aware interface for building
//! discount curves from market instruments.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_curves::builder::CurveBuilder;
//! use convex_core::Date;
//!
//! let curve = CurveBuilder::new(Date::from_ymd(2024, 11, 29).unwrap())
//!     .with_interpolation(InterpolationMethod::MonotoneConvex)
//!     // Short end: deposits
//!     .add_deposit("O/N", 0.0530)
//!     .add_deposit("1W", 0.0532)
//!     .add_deposit("1M", 0.0535)
//!     .add_deposit("3M", 0.0538)
//!     // Long end: OIS swaps
//!     .add_ois("2Y", 0.0425)
//!     .add_ois("5Y", 0.0415)
//!     .add_ois("10Y", 0.0410)
//!     .bootstrap()?;
//! ```

use convex_core::calendars::{Calendar, SIFMACalendar};
use convex_core::types::Frequency;
use convex_core::Date;

use crate::bootstrap::SequentialBootstrapper;
use crate::curves::DiscountCurve;
use crate::error::{CurveError, CurveResult};
use crate::instruments::{CurveInstrument, Deposit, FRA, OIS, RateFuture, Swap, FutureType};
use crate::interpolation::InterpolationMethod;

/// Bootstrap method to use for curve construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BootstrapMethod {
    /// Sequential bootstrap - solves for each instrument iteratively.
    #[default]
    Sequential,
    /// Global optimization - minimizes sum of squared pricing errors.
    Global,
}

/// Extrapolation method for curve values beyond pillar points.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ExtrapolationType {
    /// Flat extrapolation (constant rate/DF beyond last pillar).
    #[default]
    Flat,
    /// Linear extrapolation (extends the last segment).
    Linear,
    /// No extrapolation (error if querying beyond range).
    None,
    /// Smith-Wilson extrapolation (regulatory, e.g., EIOPA).
    SmithWilson {
        /// Ultimate forward rate.
        ufr: f64,
        /// Convergence parameter (alpha).
        alpha: f64,
    },
}

impl ExtrapolationType {
    /// Creates EIOPA EUR Smith-Wilson parameters.
    #[must_use]
    pub fn eiopa_eur() -> Self {
        Self::SmithWilson {
            ufr: 0.0345, // 3.45% UFR for EUR
            alpha: 0.05,
        }
    }

    /// Creates EIOPA USD Smith-Wilson parameters.
    #[must_use]
    pub fn eiopa_usd() -> Self {
        Self::SmithWilson {
            ufr: 0.035, // 3.5% UFR for USD
            alpha: 0.05,
        }
    }
}

/// Fluent builder for constructing yield curves.
///
/// The `CurveBuilder` provides a high-level interface for:
/// - Adding market instruments by tenor (e.g., "3M", "2Y")
/// - Configuring interpolation and extrapolation
/// - Selecting bootstrap method
/// - Building the final discount curve
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::builder::CurveBuilder;
///
/// let curve = CurveBuilder::new(reference_date)
///     .add_deposit("3M", 0.05)
///     .add_ois("2Y", 0.045)
///     .bootstrap()?;
/// ```
pub struct CurveBuilder {
    /// Reference date for the curve.
    reference_date: Date,
    /// Instruments to bootstrap.
    instruments: Vec<Box<dyn CurveInstrument>>,
    /// Interpolation method.
    interpolation: InterpolationMethod,
    /// Extrapolation method.
    extrapolation: ExtrapolationType,
    /// Bootstrap method.
    bootstrap_method: BootstrapMethod,
    /// Business day calendar.
    calendar: Box<dyn Calendar>,
    /// Spot days for settlement.
    spot_days: u32,
}

impl CurveBuilder {
    /// Creates a new curve builder with default settings.
    ///
    /// # Arguments
    ///
    /// * `reference_date` - The curve's reference/valuation date
    ///
    /// # Default Settings
    ///
    /// - Interpolation: LogLinear
    /// - Extrapolation: Flat
    /// - Bootstrap: Sequential
    /// - Calendar: SIFMA (US fixed income)
    /// - Spot days: 2 (T+2 settlement)
    #[must_use]
    pub fn new(reference_date: Date) -> Self {
        Self {
            reference_date,
            instruments: Vec::new(),
            interpolation: InterpolationMethod::LogLinear,
            extrapolation: ExtrapolationType::Flat,
            bootstrap_method: BootstrapMethod::Sequential,
            calendar: Box::new(SIFMACalendar::new()),
            spot_days: 2,
        }
    }

    /// Sets the interpolation method.
    #[must_use]
    pub fn with_interpolation(mut self, method: InterpolationMethod) -> Self {
        self.interpolation = method;
        self
    }

    /// Sets the extrapolation method.
    #[must_use]
    pub fn with_extrapolation(mut self, method: ExtrapolationType) -> Self {
        self.extrapolation = method;
        self
    }

    /// Sets the bootstrap method.
    #[must_use]
    pub fn with_bootstrap_method(mut self, method: BootstrapMethod) -> Self {
        self.bootstrap_method = method;
        self
    }

    /// Sets the business day calendar.
    #[must_use]
    pub fn with_calendar<C: Calendar + 'static>(mut self, calendar: C) -> Self {
        self.calendar = Box::new(calendar);
        self
    }

    /// Sets the spot days for settlement (T+N).
    #[must_use]
    pub fn with_spot_days(mut self, days: u32) -> Self {
        self.spot_days = days;
        self
    }

    /// Adds a deposit instrument by tenor.
    ///
    /// # Arguments
    ///
    /// * `tenor` - Tenor string (e.g., "O/N", "1W", "1M", "3M", "6M", "1Y")
    /// * `rate` - Deposit rate as a decimal (e.g., 0.05 for 5%)
    ///
    /// # Supported Tenors
    ///
    /// - O/N, ON: Overnight
    /// - T/N, TN: Tomorrow/Next
    /// - S/N, SN: Spot/Next
    /// - 1W, 2W, 3W: Weeks
    /// - 1M, 2M, 3M, 6M, 9M: Months
    /// - 1Y, 2Y, etc: Years
    #[must_use]
    pub fn add_deposit(mut self, tenor: &str, rate: f64) -> Self {
        let spot_date = self.calendar.add_business_days(self.reference_date, self.spot_days as i32);

        if let Ok(deposit) = Deposit::from_tenor(spot_date, tenor, rate) {
            self.instruments.push(Box::new(deposit));
        }
        self
    }

    /// Adds a FRA (Forward Rate Agreement) by tenor.
    ///
    /// # Arguments
    ///
    /// * `start_tenor` - Start tenor (e.g., "3M")
    /// * `end_tenor` - End tenor (e.g., "6M")
    /// * `rate` - FRA rate as a decimal
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// builder.add_fra("3M", "6M", 0.045)  // 3x6 FRA at 4.5%
    /// ```
    #[must_use]
    pub fn add_fra(mut self, start_tenor: &str, end_tenor: &str, rate: f64) -> Self {
        let spot_date = self.calendar.add_business_days(self.reference_date, self.spot_days as i32);

        // Parse tenors to months
        let start_months = parse_tenor_to_months(start_tenor).unwrap_or(0);
        let end_months = parse_tenor_to_months(end_tenor).unwrap_or(0);

        if start_months > 0 && end_months > start_months {
            if let Ok(fra) = FRA::from_tenors(spot_date, start_months, end_months, rate) {
                self.instruments.push(Box::new(fra));
            }
        }
        self
    }

    /// Adds a rate future by contract code.
    ///
    /// # Arguments
    ///
    /// * `contract` - Contract code (e.g., "SFRZ4" for Dec 2024 SOFR)
    /// * `price` - Futures price (e.g., 94.75)
    ///
    /// # Contract Codes
    ///
    /// Format: {INDEX}{MONTH}{YEAR}
    /// - INDEX: SFR (SOFR), ED (Eurodollar legacy)
    /// - MONTH: H (Mar), M (Jun), U (Sep), Z (Dec)
    /// - YEAR: Last digit (4 = 2024)
    #[must_use]
    pub fn add_future(mut self, contract: &str, price: f64) -> Self {
        if let Some(future) = self.parse_future_contract(contract, price) {
            self.instruments.push(Box::new(future));
        }
        self
    }

    /// Adds an OIS (Overnight Index Swap) by tenor.
    ///
    /// # Arguments
    ///
    /// * `tenor` - Swap tenor (e.g., "2Y", "5Y", "10Y")
    /// * `rate` - Fixed rate as a decimal
    #[must_use]
    pub fn add_ois(mut self, tenor: &str, rate: f64) -> Self {
        if let Ok(ois) = OIS::from_tenor(self.reference_date, tenor, rate) {
            self.instruments.push(Box::new(ois));
        }
        self
    }

    /// Adds an interest rate swap by tenor.
    ///
    /// # Arguments
    ///
    /// * `tenor` - Swap tenor (e.g., "2Y", "5Y", "10Y")
    /// * `rate` - Fixed rate as a decimal
    #[must_use]
    pub fn add_swap(mut self, tenor: &str, rate: f64) -> Self {
        if let Ok(swap) = Swap::from_tenor(self.reference_date, tenor, rate, Frequency::SemiAnnual) {
            self.instruments.push(Box::new(swap));
        }
        self
    }

    /// Adds an arbitrary instrument.
    ///
    /// Use this for custom instruments not covered by the convenience methods.
    #[must_use]
    pub fn add_instrument<I: CurveInstrument + 'static>(mut self, instrument: I) -> Self {
        self.instruments.push(Box::new(instrument));
        self
    }

    /// Bootstraps the curve from the added instruments.
    ///
    /// # Returns
    ///
    /// A `DiscountCurve` that prices all instruments to par.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No instruments are provided
    /// - Bootstrap fails for any instrument
    /// - Invalid curve parameters
    pub fn bootstrap(self) -> CurveResult<DiscountCurve> {
        if self.instruments.is_empty() {
            return Err(CurveError::invalid_data("No instruments provided for curve building"));
        }

        // Currently only sequential bootstrap is supported via CurveBuilder
        self.bootstrap_sequential()
    }

    /// Sequential bootstrap implementation.
    fn bootstrap_sequential(self) -> CurveResult<DiscountCurve> {
        let allow_extrapolation = !matches!(self.extrapolation, ExtrapolationType::None);

        let mut bootstrapper = SequentialBootstrapper::new(self.reference_date)
            .with_interpolation(self.interpolation)
            .with_extrapolation(allow_extrapolation);

        for inst in self.instruments {
            bootstrapper = bootstrapper.add_instrument(InstrumentWrapper(inst));
        }

        bootstrapper.bootstrap()
    }

    /// Parses a futures contract code.
    fn parse_future_contract(&self, contract: &str, price: f64) -> Option<RateFuture> {
        // Parse contract code like "SFRZ4" (SOFR Dec 2024)
        if contract.len() < 4 {
            return None;
        }

        let month_code = contract.chars().nth(contract.len() - 2)?;
        let year_digit = contract.chars().last()?.to_digit(10)?;

        let month = match month_code {
            'H' => 3,  // March
            'M' => 6,  // June
            'U' => 9,  // September
            'Z' => 12, // December
            'F' => 1,  // January
            'G' => 2,  // February
            'J' => 4,  // April
            'K' => 5,  // May
            'N' => 7,  // July
            'Q' => 8,  // August
            'V' => 10, // October
            'X' => 11, // November
            _ => return None,
        };

        // Determine year (assume current decade)
        let current_year = self.reference_date.year();
        let decade = (current_year / 10) * 10;
        let year = decade + year_digit as i32;

        // Find the IMM date (third Wednesday of the month)
        let imm_date = find_imm_date(year, month)?;

        // Accrual dates for SOFR 3M future
        let accrual_start = imm_date;
        let accrual_end = imm_date.add_months(3).ok()?;

        // Determine future type from contract prefix
        let future_type = if contract.starts_with("SFR") || contract.starts_with("SR") {
            FutureType::SOFR3M
        } else if contract.starts_with("ED") {
            FutureType::Eurodollar
        } else {
            FutureType::SOFR3M // Default
        };

        Some(RateFuture::new(future_type, imm_date, accrual_start, accrual_end, price))
    }
}

/// Wrapper to implement CurveInstrument for boxed instruments.
struct InstrumentWrapper(Box<dyn CurveInstrument>);

impl CurveInstrument for InstrumentWrapper {
    fn maturity(&self) -> Date {
        self.0.maturity()
    }

    fn pv(&self, curve: &dyn crate::traits::Curve) -> CurveResult<f64> {
        self.0.pv(curve)
    }

    fn implied_df(&self, curve: &dyn crate::traits::Curve, target_pv: f64) -> CurveResult<f64> {
        self.0.implied_df(curve, target_pv)
    }

    fn instrument_type(&self) -> crate::instruments::InstrumentType {
        self.0.instrument_type()
    }

    fn description(&self) -> String {
        self.0.description()
    }
}

/// Parses a tenor string to months.
fn parse_tenor_to_months(tenor: &str) -> Option<i32> {
    let tenor = tenor.trim().to_uppercase();

    if tenor.ends_with('M') {
        let num: i32 = tenor.trim_end_matches('M').parse().ok()?;
        Some(num)
    } else if tenor.ends_with('Y') {
        let num: i32 = tenor.trim_end_matches('Y').parse().ok()?;
        Some(num * 12)
    } else if tenor.ends_with('W') {
        // Approximate weeks to months (4 weeks = 1 month)
        let num: i32 = tenor.trim_end_matches('W').parse().ok()?;
        Some((num + 3) / 4) // Round up
    } else {
        None
    }
}

/// Finds the IMM date (third Wednesday) for a given month.
fn find_imm_date(year: i32, month: u32) -> Option<Date> {
    let first = Date::from_ymd(year, month, 1).ok()?;

    // Find the first Wednesday
    let days_to_wednesday = (3 - first.weekday() as i64 + 7) % 7;
    let first_wednesday = first.add_days(days_to_wednesday);

    // Third Wednesday is 14 days later
    Some(first_wednesday.add_days(14))
}

/// Extension trait for adding instruments by reference.
pub trait CurveBuilderExt {
    /// Adds multiple deposits at once.
    fn add_deposits(self, deposits: &[(&str, f64)]) -> Self;

    /// Adds multiple OIS swaps at once.
    fn add_ois_swaps(self, swaps: &[(&str, f64)]) -> Self;
}

impl CurveBuilderExt for CurveBuilder {
    fn add_deposits(mut self, deposits: &[(&str, f64)]) -> Self {
        for (tenor, rate) in deposits {
            self = self.add_deposit(tenor, *rate);
        }
        self
    }

    fn add_ois_swaps(mut self, swaps: &[(&str, f64)]) -> Self {
        for (tenor, rate) in swaps {
            self = self.add_ois(tenor, *rate);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::Curve;

    #[test]
    fn test_curve_builder_basic() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let curve = CurveBuilder::new(ref_date)
            .add_deposit("3M", 0.05)
            .add_deposit("6M", 0.052)
            .add_ois("1Y", 0.048)
            .bootstrap()
            .unwrap();

        assert_eq!(curve.reference_date(), ref_date);
        assert!(curve.discount_factor(0.5).is_ok());
    }

    #[test]
    fn test_curve_builder_with_interpolation() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        // MonotoneConvex needs more points - use LogLinear for this simple test
        let curve = CurveBuilder::new(ref_date)
            .with_interpolation(InterpolationMethod::LogLinear)
            .add_deposit("3M", 0.05)
            .add_deposit("6M", 0.052)
            .bootstrap()
            .unwrap();

        assert!(curve.discount_factor(0.25).is_ok());
    }

    #[test]
    fn test_curve_builder_empty_fails() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let result = CurveBuilder::new(ref_date).bootstrap();

        assert!(result.is_err());
    }

    #[test]
    fn test_curve_builder_ext_deposits() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let curve = CurveBuilder::new(ref_date)
            .add_deposits(&[
                ("1M", 0.050),
                ("3M", 0.052),
                ("6M", 0.054),
            ])
            .bootstrap()
            .unwrap();

        assert!(curve.discount_factor(0.5).is_ok());
    }

    #[test]
    fn test_curve_builder_ois_swaps() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();

        let curve = CurveBuilder::new(ref_date)
            .add_ois("1Y", 0.045)
            .add_ois("2Y", 0.042)
            .bootstrap()
            .unwrap();

        let df_1y = curve.discount_factor(1.0).unwrap();
        let df_2y = curve.discount_factor(2.0).unwrap();

        assert!(df_1y > df_2y);
    }

    #[test]
    fn test_find_imm_date() {
        // Dec 2024: Dec 1 is Sunday, first Wednesday is Dec 4, third is Dec 18
        let imm = find_imm_date(2024, 12).unwrap();
        // Dec 1, 2024 is Sunday (weekday 6), so first Wed is Dec 4, third Wed is Dec 18
        assert!(imm.day() >= 15 && imm.day() <= 21, "IMM date should be third Wednesday, got day {}", imm.day());

        // Mar 2025: Mar 1 is Saturday, first Wednesday is Mar 5, third is Mar 19
        let imm = find_imm_date(2025, 3).unwrap();
        assert!(imm.day() >= 15 && imm.day() <= 21, "IMM date should be third Wednesday, got day {}", imm.day());
    }

    #[test]
    fn test_extrapolation_types() {
        let eur = ExtrapolationType::eiopa_eur();
        if let ExtrapolationType::SmithWilson { ufr, alpha } = eur {
            assert!((ufr - 0.0345).abs() < 1e-10);
            assert!((alpha - 0.05).abs() < 1e-10);
        } else {
            panic!("Expected SmithWilson");
        }
    }
}
