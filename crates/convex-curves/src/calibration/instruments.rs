//! Calibration instrument definitions.
//!
//! This module defines the instruments that can be used to calibrate yield curves:
//!
//! - [`Deposit`]: Money market deposit rates
//! - [`Fra`]: Forward rate agreements
//! - [`Future`]: Interest rate futures (with convexity adjustment)
//! - [`Swap`]: Fixed-for-floating interest rate swaps
//! - [`Ois`]: Overnight index swaps
//!
//! Each instrument implements the [`CalibrationInstrument`] trait, which provides:
//! - Maturity and tenor information
//! - Present value calculation given a curve
//! - Sensitivity (DV01) for Newton-based solvers
//!
//! # Curve Instruments
//!
//! For more general curve construction (e.g., with government bonds), the
//! [`CurveInstrument`] trait provides a dynamic interface that works with
//! any curve implementing [`RateCurveDyn`].

use std::fmt;

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Date, Frequency};
use rust_decimal::prelude::ToPrimitive;

use crate::curves::DiscreteCurve;
use crate::error::{CurveError, CurveResult};
use crate::wrappers::{RateCurve, RateCurveDyn};

// ============================================================================
// Instrument Type Enum
// ============================================================================

/// Type of curve calibration instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstrumentType {
    /// Cash deposit
    Deposit,
    /// Forward rate agreement
    Fra,
    /// Interest rate future
    Future,
    /// Interest rate swap
    Swap,
    /// Overnight index swap
    Ois,
    /// Basis swap
    BasisSwap,
    /// Zero-coupon government bond
    GovernmentZeroCoupon,
    /// Fixed coupon government bond
    GovernmentCouponBond,
    /// Treasury bill
    TBill,
    /// Other instrument type
    Other,
}

impl fmt::Display for InstrumentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstrumentType::Deposit => write!(f, "Deposit"),
            InstrumentType::Fra => write!(f, "FRA"),
            InstrumentType::Future => write!(f, "Future"),
            InstrumentType::Swap => write!(f, "Swap"),
            InstrumentType::Ois => write!(f, "OIS"),
            InstrumentType::BasisSwap => write!(f, "Basis Swap"),
            InstrumentType::GovernmentZeroCoupon => write!(f, "Govt Zero"),
            InstrumentType::GovernmentCouponBond => write!(f, "Govt Coupon"),
            InstrumentType::TBill => write!(f, "T-Bill"),
            InstrumentType::Other => write!(f, "Other"),
        }
    }
}

// ============================================================================
// CurveInstrument Trait (Dynamic Interface)
// ============================================================================

/// Trait for instruments used in curve construction.
///
/// This trait provides a dynamic interface for curve instruments, allowing
/// them to work with any curve implementing [`RateCurveDyn`]. It's more
/// flexible than [`CalibrationInstrument`] which uses concrete types.
///
/// # Implementation
///
/// Each instrument should provide methods to:
/// - Report its maturity date
/// - Calculate present value given a curve
/// - Derive the implied discount factor
pub trait CurveInstrument: Send + Sync {
    /// Returns the maturity date of the instrument.
    fn maturity(&self) -> Date;

    /// Returns the pillar date (typically same as maturity).
    ///
    /// For some instruments like swaps, this might differ from maturity.
    fn pillar_date(&self) -> Date {
        self.maturity()
    }

    /// Calculates the present value given a curve.
    ///
    /// For calibration, this should return the difference between
    /// theoretical and market price.
    fn pv(&self, curve: &dyn RateCurveDyn) -> CurveResult<f64>;

    /// Returns the implied discount factor at maturity.
    ///
    /// Given the known portion of the curve, this returns the discount
    /// factor that would make the instrument price correctly.
    fn implied_df(&self, curve: &dyn RateCurveDyn, target_pv: f64) -> CurveResult<f64>;

    /// Returns the instrument type.
    fn instrument_type(&self) -> InstrumentType;

    /// Returns a description of the instrument.
    fn description(&self) -> String;
}

// ============================================================================
// Helper Traits and Functions
// ============================================================================

/// Helper trait extension for DayCountConvention to get year fraction as f64.
trait DayCountExt {
    fn year_fraction_f64(&self, start: Date, end: Date) -> f64;
}

impl DayCountExt for DayCountConvention {
    fn year_fraction_f64(&self, start: Date, end: Date) -> f64 {
        self.to_day_count()
            .year_fraction(start, end)
            .to_f64()
            .unwrap_or(0.0)
    }
}

/// Helper to add fractional years to a date.
/// Approximates using 365.25 days per year.
fn add_years_fraction(date: Date, years: f64) -> Date {
    let days = (years * 365.25).round() as i64;
    date.add_days(days)
}

/// Trait for instruments used in curve calibration.
///
/// Each instrument knows how to:
/// - Calculate its present value given a discount curve
/// - Calculate its sensitivity (DV01) for solver convergence
/// - Provide maturity information for ordering
///
/// Note: Methods use `DiscreteCurve` directly to enable trait object usage.
pub trait CalibrationInstrument: Send + Sync + fmt::Debug {
    /// Reference date for the instrument.
    fn reference_date(&self) -> Date;

    /// Maturity date of the instrument.
    fn maturity(&self) -> Date;

    /// Time to maturity in years.
    fn tenor(&self) -> f64;

    /// Market quote (rate, price, or spread depending on instrument type).
    fn quote(&self) -> f64;

    /// Calculate present value given a discount curve.
    ///
    /// For rate instruments, this typically returns the difference between
    /// the implied rate from the curve and the market quote.
    fn pv(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64>;

    /// Calculate pricing error (PV that should be zero when calibrated).
    ///
    /// This is the residual that the calibration solver minimizes.
    fn pricing_error(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64>;

    /// Calculate DV01 (sensitivity to 1bp rate move).
    ///
    /// Used by Newton-based solvers for faster convergence.
    fn dv01(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64>;

    /// Instrument type description.
    fn instrument_type(&self) -> &'static str;

    /// Returns a description of the instrument for display.
    fn description(&self) -> String {
        format!(
            "{} {} @ {:.4}%",
            self.instrument_type(),
            format_tenor(self.tenor()),
            self.quote() * 100.0
        )
    }
}

/// Formats a tenor in years to a readable string.
fn format_tenor(t: f64) -> String {
    if t < 1.0 {
        let months = (t * 12.0).round() as i32;
        format!("{}M", months)
    } else {
        let years = t.round() as i32;
        format!("{}Y", years)
    }
}

/// Money market deposit.
///
/// A deposit is a simple zero-coupon instrument that pays
/// principal + interest at maturity.
///
/// # Pricing
///
/// For a deposit rate r with day count fraction τ:
/// - Simple interest: PV = 1 / (1 + r × τ) should equal DF(T)
/// - Pricing error = DF(T) × (1 + r × τ) - 1
#[derive(Debug, Clone)]
pub struct Deposit {
    /// Reference date.
    reference_date: Date,
    /// Maturity date.
    maturity: Date,
    /// Deposit rate (as decimal, e.g., 0.05 for 5%).
    rate: f64,
    /// Day count convention.
    day_count: DayCountConvention,
    /// Notional (for PV calculation, typically 1.0).
    notional: f64,
}

impl Deposit {
    /// Creates a new deposit instrument.
    #[must_use]
    pub fn new(
        reference_date: Date,
        maturity: Date,
        rate: f64,
        day_count: DayCountConvention,
    ) -> Self {
        Self {
            reference_date,
            maturity,
            rate,
            day_count,
            notional: 1.0,
        }
    }

    /// Creates a deposit from tenor in years.
    #[must_use]
    pub fn from_tenor(
        reference_date: Date,
        tenor_years: f64,
        rate: f64,
        day_count: DayCountConvention,
    ) -> Self {
        let maturity = add_years_fraction(reference_date, tenor_years);
        Self::new(reference_date, maturity, rate, day_count)
    }

    /// Returns the year fraction using the day count convention.
    fn year_fraction(&self) -> f64 {
        self.day_count
            .year_fraction_f64(self.reference_date, self.maturity)
    }
}

impl CalibrationInstrument for Deposit {
    fn reference_date(&self) -> Date {
        self.reference_date
    }

    fn maturity(&self) -> Date {
        self.maturity
    }

    fn tenor(&self) -> f64 {
        self.year_fraction()
    }

    fn quote(&self) -> f64 {
        self.rate
    }

    fn pv(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let df = curve.discount_factor(self.maturity)?;
        let tau = self.year_fraction();

        // PV of deposit: pay 1 at t=0, receive 1 + r*tau at T
        // PV = -1 + (1 + r*tau) * DF(T)
        Ok(self.notional * (-1.0 + (1.0 + self.rate * tau) * df))
    }

    fn pricing_error(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        // Error is (1 + r*tau) * DF(T) - 1, which should be zero
        let df = curve.discount_factor(self.maturity)?;
        let tau = self.year_fraction();
        Ok((1.0 + self.rate * tau) * df - 1.0)
    }

    fn dv01(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        // DV01 ≈ -τ × DF(T) for a deposit
        let df = curve.discount_factor(self.maturity)?;
        let tau = self.year_fraction();
        Ok(-tau * df * self.notional)
    }

    fn instrument_type(&self) -> &'static str {
        "Deposit"
    }
}

/// Forward Rate Agreement (FRA).
///
/// An FRA is an agreement to exchange a fixed rate for a floating rate
/// over a future period.
///
/// # Pricing
///
/// FRA rate = (DF(T1) / DF(T2) - 1) / τ
/// where τ is the accrual period.
#[derive(Debug, Clone)]
pub struct Fra {
    /// Reference date.
    reference_date: Date,
    /// Start date of the forward period.
    start_date: Date,
    /// End date of the forward period.
    end_date: Date,
    /// FRA rate (as decimal).
    rate: f64,
    /// Day count convention.
    day_count: DayCountConvention,
    /// Notional.
    notional: f64,
}

impl Fra {
    /// Creates a new FRA instrument.
    #[must_use]
    pub fn new(
        reference_date: Date,
        start_date: Date,
        end_date: Date,
        rate: f64,
        day_count: DayCountConvention,
    ) -> Self {
        Self {
            reference_date,
            start_date,
            end_date,
            rate,
            day_count,
            notional: 1.0,
        }
    }

    /// Creates a FRA from start and end tenors (e.g., 3x6 FRA).
    #[must_use]
    pub fn from_tenors(
        reference_date: Date,
        start_months: i32,
        end_months: i32,
        rate: f64,
        day_count: DayCountConvention,
    ) -> Self {
        // Note: unwrap() is safe here as the caller provides valid months
        let start_date = reference_date.add_months(start_months).unwrap();
        let end_date = reference_date.add_months(end_months).unwrap();
        Self::new(reference_date, start_date, end_date, rate, day_count)
    }

    /// Returns the forward period year fraction.
    fn forward_period(&self) -> f64 {
        self.day_count
            .year_fraction_f64(self.start_date, self.end_date)
    }
}

impl CalibrationInstrument for Fra {
    fn reference_date(&self) -> Date {
        self.reference_date
    }

    fn maturity(&self) -> Date {
        self.end_date
    }

    fn tenor(&self) -> f64 {
        self.day_count
            .year_fraction_f64(self.reference_date, self.end_date)
    }

    fn quote(&self) -> f64 {
        self.rate
    }

    fn pv(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let df_start = curve.discount_factor(self.start_date)?;
        let df_end = curve.discount_factor(self.end_date)?;
        let tau = self.forward_period();

        // Implied forward rate
        let implied_fwd = (df_start / df_end - 1.0) / tau;

        // PV = (implied_fwd - fra_rate) × τ × DF(end)
        Ok(self.notional * (implied_fwd - self.rate) * tau * df_end)
    }

    fn pricing_error(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let df_start = curve.discount_factor(self.start_date)?;
        let df_end = curve.discount_factor(self.end_date)?;
        let tau = self.forward_period();

        // Error: implied rate - quoted rate
        let implied_fwd = (df_start / df_end - 1.0) / tau;
        Ok(implied_fwd - self.rate)
    }

    fn dv01(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let df_end = curve.discount_factor(self.end_date)?;
        let tau = self.forward_period();

        // DV01 ≈ τ × DF(end)
        Ok(tau * df_end * self.notional)
    }

    fn instrument_type(&self) -> &'static str {
        "FRA"
    }

    fn description(&self) -> String {
        let start_months = (self
            .day_count
            .year_fraction_f64(self.reference_date, self.start_date)
            * 12.0)
            .round() as i32;
        let end_months = (self
            .day_count
            .year_fraction_f64(self.reference_date, self.end_date)
            * 12.0)
            .round() as i32;
        format!(
            "FRA {}x{} @ {:.4}%",
            start_months,
            end_months,
            self.rate * 100.0
        )
    }
}

/// Interest rate future.
///
/// Futures are quoted as price = 100 - rate. We apply a convexity
/// adjustment to convert from futures rate to forward rate.
#[derive(Debug, Clone)]
pub struct Future {
    /// Reference date.
    reference_date: Date,
    /// IMM date (settlement date).
    imm_date: Date,
    /// End date of the underlying period (typically 3M after IMM).
    end_date: Date,
    /// Future price (e.g., 95.0 for 5% rate).
    price: f64,
    /// Convexity adjustment in basis points.
    convexity_adj_bps: f64,
    /// Day count convention.
    day_count: DayCountConvention,
    /// Notional.
    notional: f64,
}

impl Future {
    /// Creates a new interest rate future.
    #[must_use]
    pub fn new(
        reference_date: Date,
        imm_date: Date,
        end_date: Date,
        price: f64,
        convexity_adj_bps: f64,
        day_count: DayCountConvention,
    ) -> Self {
        Self {
            reference_date,
            imm_date,
            end_date,
            price,
            convexity_adj_bps,
            day_count,
            notional: 1.0,
        }
    }

    /// Returns the futures rate (100 - price) as a decimal.
    #[must_use]
    pub fn futures_rate(&self) -> f64 {
        (100.0 - self.price) / 100.0
    }

    /// Returns the convexity-adjusted forward rate.
    #[must_use]
    pub fn adjusted_rate(&self) -> f64 {
        self.futures_rate() - self.convexity_adj_bps / 10_000.0
    }

    /// Returns the forward period year fraction.
    fn forward_period(&self) -> f64 {
        self.day_count
            .year_fraction_f64(self.imm_date, self.end_date)
    }
}

impl CalibrationInstrument for Future {
    fn reference_date(&self) -> Date {
        self.reference_date
    }

    fn maturity(&self) -> Date {
        self.end_date
    }

    fn tenor(&self) -> f64 {
        self.day_count
            .year_fraction_f64(self.reference_date, self.end_date)
    }

    fn quote(&self) -> f64 {
        self.adjusted_rate()
    }

    fn pv(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let df_start = curve.discount_factor(self.imm_date)?;
        let df_end = curve.discount_factor(self.end_date)?;
        let tau = self.forward_period();

        let implied_fwd = (df_start / df_end - 1.0) / tau;
        let adjusted = self.adjusted_rate();

        // PV difference
        Ok(self.notional * (implied_fwd - adjusted) * tau * df_end)
    }

    fn pricing_error(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let df_start = curve.discount_factor(self.imm_date)?;
        let df_end = curve.discount_factor(self.end_date)?;
        let tau = self.forward_period();

        let implied_fwd = (df_start / df_end - 1.0) / tau;
        Ok(implied_fwd - self.adjusted_rate())
    }

    fn dv01(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let df_end = curve.discount_factor(self.end_date)?;
        let tau = self.forward_period();
        Ok(tau * df_end * self.notional)
    }

    fn instrument_type(&self) -> &'static str {
        "Future"
    }

    fn description(&self) -> String {
        format!(
            "Future @ {:.2} (adj rate {:.4}%)",
            self.price,
            self.adjusted_rate() * 100.0
        )
    }
}

/// Interest rate swap (fixed-for-floating).
///
/// The most important instrument for curve calibration beyond 2Y.
///
/// # Pricing
///
/// Par swap rate = (1 - DF(T)) / Σ(τᵢ × DF(Tᵢ))
/// where the sum is over fixed leg payment dates.
#[derive(Debug, Clone)]
pub struct Swap {
    /// Reference date.
    reference_date: Date,
    /// Effective date (typically T+2).
    effective_date: Date,
    /// Maturity date.
    maturity: Date,
    /// Fixed rate (as decimal).
    fixed_rate: f64,
    /// Fixed leg frequency.
    fixed_frequency: Frequency,
    /// Fixed leg day count.
    fixed_day_count: DayCountConvention,
    /// Notional.
    notional: f64,
}

impl Swap {
    /// Creates a new interest rate swap.
    #[must_use]
    pub fn new(
        reference_date: Date,
        effective_date: Date,
        maturity: Date,
        fixed_rate: f64,
        fixed_frequency: Frequency,
        fixed_day_count: DayCountConvention,
    ) -> Self {
        Self {
            reference_date,
            effective_date,
            maturity,
            fixed_rate,
            fixed_frequency,
            fixed_day_count,
            notional: 1.0,
        }
    }

    /// Creates a swap from tenor.
    #[must_use]
    pub fn from_tenor(
        reference_date: Date,
        tenor_years: f64,
        fixed_rate: f64,
        fixed_frequency: Frequency,
        fixed_day_count: DayCountConvention,
    ) -> Self {
        // Assume T+2 settlement
        let effective_date = reference_date.add_days(2);
        let maturity = add_years_fraction(effective_date, tenor_years);
        Self::new(
            reference_date,
            effective_date,
            maturity,
            fixed_rate,
            fixed_frequency,
            fixed_day_count,
        )
    }

    /// Generates fixed leg payment dates.
    fn fixed_schedule(&self) -> Vec<Date> {
        let periods_per_year = self.fixed_frequency.periods_per_year();
        let total_years = self
            .fixed_day_count
            .year_fraction_f64(self.effective_date, self.maturity);
        let num_periods = (total_years * periods_per_year as f64).round() as i32;

        let mut dates = Vec::with_capacity(num_periods as usize);
        for i in 1..=num_periods {
            let t = i as f64 / periods_per_year as f64;
            dates.push(add_years_fraction(self.effective_date, t));
        }

        // Ensure last date is maturity
        if let Some(last) = dates.last_mut() {
            *last = self.maturity;
        }

        dates
    }

    /// Calculates the annuity (PV01) of the fixed leg.
    fn annuity(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let schedule = self.fixed_schedule();
        let mut annuity = 0.0;
        let mut prev_date = self.effective_date;

        for date in &schedule {
            let tau = self.fixed_day_count.year_fraction_f64(prev_date, *date);
            let df = curve.discount_factor(*date)?;
            annuity += tau * df;
            prev_date = *date;
        }

        Ok(annuity * self.notional)
    }

    /// Calculates the par swap rate from the curve.
    fn par_rate(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let df_eff = curve.discount_factor(self.effective_date)?;
        let df_mat = curve.discount_factor(self.maturity)?;
        let annuity = self.annuity(curve)?;

        if annuity.abs() < 1e-12 {
            return Err(CurveError::calibration_failed(0, 0.0, "Annuity is zero"));
        }

        Ok((df_eff - df_mat) / annuity * self.notional)
    }
}

impl CalibrationInstrument for Swap {
    fn reference_date(&self) -> Date {
        self.reference_date
    }

    fn maturity(&self) -> Date {
        self.maturity
    }

    fn tenor(&self) -> f64 {
        self.fixed_day_count
            .year_fraction_f64(self.reference_date, self.maturity)
    }

    fn quote(&self) -> f64 {
        self.fixed_rate
    }

    fn pv(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let df_eff = curve.discount_factor(self.effective_date)?;
        let df_mat = curve.discount_factor(self.maturity)?;
        let annuity = self.annuity(curve)?;

        // Floating leg PV = DF(eff) - DF(mat) (assuming par floater)
        // Fixed leg PV = fixed_rate × annuity
        // Receiver swap PV = Fixed - Float
        let float_pv = df_eff - df_mat;
        let fixed_pv = self.fixed_rate * annuity;

        Ok(self.notional * (fixed_pv - float_pv))
    }

    fn pricing_error(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        // Error = par_rate - quoted_rate
        let par = self.par_rate(curve)?;
        Ok(par - self.fixed_rate)
    }

    fn dv01(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        // DV01 ≈ annuity (for small rate changes)
        self.annuity(curve)
    }

    fn instrument_type(&self) -> &'static str {
        "Swap"
    }
}

/// Overnight Index Swap (OIS).
///
/// An OIS is a swap where the floating leg pays the compounded overnight rate.
/// Used for building the discount curve in post-LIBOR world.
#[derive(Debug, Clone)]
pub struct Ois {
    /// Reference date.
    reference_date: Date,
    /// Effective date.
    effective_date: Date,
    /// Maturity date.
    maturity: Date,
    /// Fixed rate.
    fixed_rate: f64,
    /// Fixed leg frequency (typically annual for OIS).
    fixed_frequency: Frequency,
    /// Day count convention.
    day_count: DayCountConvention,
    /// Notional.
    notional: f64,
}

impl Ois {
    /// Creates a new OIS.
    #[must_use]
    pub fn new(
        reference_date: Date,
        effective_date: Date,
        maturity: Date,
        fixed_rate: f64,
        fixed_frequency: Frequency,
        day_count: DayCountConvention,
    ) -> Self {
        Self {
            reference_date,
            effective_date,
            maturity,
            fixed_rate,
            fixed_frequency,
            day_count,
            notional: 1.0,
        }
    }

    /// Creates an OIS from tenor.
    #[must_use]
    pub fn from_tenor(
        reference_date: Date,
        tenor_years: f64,
        fixed_rate: f64,
        day_count: DayCountConvention,
    ) -> Self {
        let effective_date = reference_date.add_days(2);
        let maturity = add_years_fraction(effective_date, tenor_years);

        // OIS typically has annual fixed payments, use Annual for all tenors
        // For short tenors (<= 1Y), there's only one payment anyway
        let frequency = Frequency::Annual;

        Self::new(
            reference_date,
            effective_date,
            maturity,
            fixed_rate,
            frequency,
            day_count,
        )
    }

    /// Generates fixed leg payment dates.
    fn fixed_schedule(&self) -> Vec<Date> {
        let periods_per_year = self.fixed_frequency.periods_per_year();

        let total_years = self
            .day_count
            .year_fraction_f64(self.effective_date, self.maturity);
        let num_periods = (total_years * periods_per_year as f64).round() as i32;

        // At least one payment at maturity
        let num_periods = num_periods.max(1);

        let mut dates = Vec::with_capacity(num_periods as usize);
        for i in 1..=num_periods {
            let t = i as f64 / periods_per_year as f64;
            dates.push(add_years_fraction(self.effective_date, t));
        }

        if let Some(last) = dates.last_mut() {
            *last = self.maturity;
        }

        dates
    }

    /// Calculates the annuity.
    fn annuity(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let schedule = self.fixed_schedule();
        let mut annuity = 0.0;
        let mut prev_date = self.effective_date;

        for date in &schedule {
            let tau = self.day_count.year_fraction_f64(prev_date, *date);
            let df = curve.discount_factor(*date)?;
            annuity += tau * df;
            prev_date = *date;
        }

        Ok(annuity * self.notional)
    }

    /// Calculates the par OIS rate.
    fn par_rate(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let df_eff = curve.discount_factor(self.effective_date)?;
        let df_mat = curve.discount_factor(self.maturity)?;
        let annuity = self.annuity(curve)?;

        if annuity.abs() < 1e-12 {
            return Err(CurveError::calibration_failed(0, 0.0, "Annuity is zero"));
        }

        Ok((df_eff - df_mat) / annuity * self.notional)
    }
}

impl CalibrationInstrument for Ois {
    fn reference_date(&self) -> Date {
        self.reference_date
    }

    fn maturity(&self) -> Date {
        self.maturity
    }

    fn tenor(&self) -> f64 {
        self.day_count
            .year_fraction_f64(self.reference_date, self.maturity)
    }

    fn quote(&self) -> f64 {
        self.fixed_rate
    }

    fn pv(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let df_eff = curve.discount_factor(self.effective_date)?;
        let df_mat = curve.discount_factor(self.maturity)?;
        let annuity = self.annuity(curve)?;

        let float_pv = df_eff - df_mat;
        let fixed_pv = self.fixed_rate * annuity;

        Ok(self.notional * (fixed_pv - float_pv))
    }

    fn pricing_error(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let par = self.par_rate(curve)?;
        Ok(par - self.fixed_rate)
    }

    fn dv01(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        self.annuity(curve)
    }

    fn instrument_type(&self) -> &'static str {
        "OIS"
    }
}

/// A collection of calibration instruments.
#[derive(Debug, Default)]
pub struct InstrumentSet {
    /// The instruments, stored as trait objects.
    instruments: Vec<Box<dyn CalibrationInstrument>>,
}

impl InstrumentSet {
    /// Creates a new empty instrument set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an instrument to the set.
    pub fn add<I: CalibrationInstrument + 'static>(&mut self, instrument: I) {
        self.instruments.push(Box::new(instrument));
    }

    /// Adds an instrument and returns self for chaining.
    #[must_use]
    pub fn with<I: CalibrationInstrument + 'static>(mut self, instrument: I) -> Self {
        self.add(instrument);
        self
    }

    /// Returns the number of instruments.
    #[must_use]
    pub fn len(&self) -> usize {
        self.instruments.len()
    }

    /// Returns true if empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instruments.is_empty()
    }

    /// Returns the instruments as a slice.
    #[must_use]
    pub fn instruments(&self) -> &[Box<dyn CalibrationInstrument>] {
        &self.instruments
    }

    /// Sorts instruments by maturity.
    pub fn sort_by_maturity(&mut self) {
        self.instruments
            .sort_by(|a, b| a.tenor().partial_cmp(&b.tenor()).unwrap());
    }

    /// Returns tenors of all instruments.
    #[must_use]
    pub fn tenors(&self) -> Vec<f64> {
        self.instruments.iter().map(|i| i.tenor()).collect()
    }

    /// Returns quotes of all instruments.
    #[must_use]
    pub fn quotes(&self) -> Vec<f64> {
        self.instruments.iter().map(|i| i.quote()).collect()
    }

    /// Calculates pricing errors for all instruments.
    pub fn pricing_errors(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<Vec<f64>> {
        self.instruments
            .iter()
            .map(|i| i.pricing_error(curve))
            .collect()
    }

    /// Calculates RMS error across all instruments.
    pub fn rms_error(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        let errors = self.pricing_errors(curve)?;
        let sum_sq: f64 = errors.iter().map(|e| e * e).sum();
        Ok((sum_sq / errors.len() as f64).sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InterpolationMethod, ValueType};

    fn sample_discount_curve(reference_date: Date) -> DiscreteCurve {
        let tenors: Vec<f64> = vec![0.0, 0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0];
        // Flat 4% curve for simplicity
        let dfs: Vec<f64> = tenors.iter().map(|&t| (-0.04_f64 * t).exp()).collect();

        DiscreteCurve::new(
            reference_date,
            tenors,
            dfs,
            ValueType::DiscountFactor,
            InterpolationMethod::LogLinear,
        )
        .unwrap()
    }

    #[test]
    fn test_deposit_pricing() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let curve = sample_discount_curve(today);
        let rate_curve = RateCurve::new(curve);

        // 6M deposit at 4%
        let deposit = Deposit::from_tenor(today, 0.5, 0.04, DayCountConvention::Act360);

        // On a flat 4% curve, the deposit should price close to par
        let error = deposit.pricing_error(&rate_curve).unwrap();
        assert!(error.abs() < 0.001); // Within 10bps
    }

    #[test]
    fn test_deposit_dv01() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let curve = sample_discount_curve(today);
        let rate_curve = RateCurve::new(curve);

        let deposit = Deposit::from_tenor(today, 1.0, 0.04, DayCountConvention::Act360);
        let dv01 = deposit.dv01(&rate_curve).unwrap();

        // DV01 should be approximately -τ × DF
        assert!(dv01 < 0.0);
        assert!(dv01.abs() > 0.9 && dv01.abs() < 1.1);
    }

    #[test]
    fn test_fra_pricing() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let curve = sample_discount_curve(today);
        let rate_curve = RateCurve::new(curve);

        // 3x6 FRA at 4% (flat curve, so forward = spot)
        let fra = Fra::from_tenors(today, 3, 6, 0.04, DayCountConvention::Act360);

        let error = fra.pricing_error(&rate_curve).unwrap();
        assert!(error.abs() < 0.001);
    }

    #[test]
    fn test_swap_par_rate() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let curve = sample_discount_curve(today);
        let rate_curve = RateCurve::new(curve);

        // 5Y swap - par rate should be close to 4% on flat curve
        let swap = Swap::from_tenor(
            today,
            5.0,
            0.04,
            Frequency::SemiAnnual,
            DayCountConvention::Thirty360US,
        );

        let error = swap.pricing_error(&rate_curve).unwrap();
        // Par rate should be very close to 4%
        assert!(error.abs() < 0.001);
    }

    #[test]
    fn test_swap_pv_at_par() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let curve = sample_discount_curve(today);
        let rate_curve = RateCurve::new(curve);

        // At-par swap should have near-zero PV
        let swap = Swap::from_tenor(
            today,
            5.0,
            0.04,
            Frequency::SemiAnnual,
            DayCountConvention::Thirty360US,
        );

        let pv = swap.pv(&rate_curve).unwrap();
        assert!(pv.abs() < 0.01);
    }

    #[test]
    fn test_ois_pricing() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let curve = sample_discount_curve(today);
        let rate_curve = RateCurve::new(curve);

        let ois = Ois::from_tenor(today, 2.0, 0.04, DayCountConvention::Act360);

        let error = ois.pricing_error(&rate_curve).unwrap();
        assert!(error.abs() < 0.001);
    }

    #[test]
    fn test_instrument_set() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();
        let curve = sample_discount_curve(today);
        let rate_curve = RateCurve::new(curve);

        let mut instruments = InstrumentSet::new();
        instruments.add(Deposit::from_tenor(
            today,
            0.25,
            0.039,
            DayCountConvention::Act360,
        ));
        instruments.add(Deposit::from_tenor(
            today,
            0.5,
            0.04,
            DayCountConvention::Act360,
        ));
        instruments.add(Swap::from_tenor(
            today,
            2.0,
            0.041,
            Frequency::SemiAnnual,
            DayCountConvention::Thirty360US,
        ));

        assert_eq!(instruments.len(), 3);

        let tenors = instruments.tenors();
        assert_eq!(tenors.len(), 3);

        let rms = instruments.rms_error(&rate_curve).unwrap();
        assert!(rms < 0.01); // Should be small on reasonable curve
    }

    #[test]
    fn test_instrument_descriptions() {
        let today = Date::from_ymd(2024, 1, 2).unwrap();

        let deposit = Deposit::from_tenor(today, 0.5, 0.045, DayCountConvention::Act360);
        let desc = deposit.description();
        assert!(desc.contains("Deposit"));
        assert!(desc.contains("6M"));

        let fra = Fra::from_tenors(today, 3, 6, 0.04, DayCountConvention::Act360);
        let desc = fra.description();
        assert!(desc.contains("FRA"));
        assert!(desc.contains("3x6"));
    }
}
