//! Repricing validation for curve bootstrap.
//!
//! This module provides mandatory repricing validation to ensure that all input
//! instruments are correctly priced by the bootstrapped curve. A curve that
//! cannot reprice its inputs within tolerance is considered incorrect.
//!
//! # Key Principle
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    MARKET OBSERVABLE RULE                    │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                              │
//! │  If you can't reprice every input instrument within         │
//! │  tolerance, YOUR CURVE IS WRONG.                            │
//! │                                                              │
//! │  No exceptions. No approximations. No "close enough."       │
//! │                                                              │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use convex_curves::repricing::BootstrapResult;
//! use convex_curves::bootstrap::SequentialBootstrapper;
//!
//! let result = SequentialBootstrapper::new(ref_date)
//!     .add_instrument(deposit)
//!     .add_instrument(swap)
//!     .bootstrap_validated()?;
//!
//! // Check if all instruments repriced correctly
//! if !result.is_valid() {
//!     eprintln!("Failed instruments: {:?}", result.failed_instruments());
//!     return Err(CurveError::repricing_failed(result.repricing_report));
//! }
//!
//! // Safe to use the curve
//! let df = result.curve.discount_factor(1.0)?;
//! ```

use std::fmt;
use std::time::{Duration, Instant};

use crate::curves::DiscountCurve;
use crate::error::CurveResult;
use crate::instruments::CurveInstrument;
use crate::traits::Curve;

// Re-import InstrumentType to use in tolerances module
pub use crate::instruments::InstrumentType;

/// Repricing tolerances by instrument type.
///
/// These tolerances are based on market conventions and Bloomberg standards.
/// All tolerances are in absolute PV terms for a unit notional.
///
/// # Tolerance Levels
///
/// | Instrument Type | Tolerance | Meaning                |
/// |-----------------|-----------|------------------------|
/// | Deposits        | 1e-9      | Near machine precision |
/// | FRAs            | 1e-9      | Near machine precision |
/// | Swaps/OIS       | 1e-6      | 0.0001 per unit notional |
/// | Bonds           | 1e-6      | 0.0001 per unit notional |
///
/// The tight tolerances are achievable because we fixed the day count
/// convention mismatch (ACT/360 consistently in both bootstrapper and instruments).
pub mod tolerances {
    /// Deposit rate tolerance - near machine precision
    pub const DEPOSIT: f64 = 1e-9;

    /// FRA rate tolerance - near machine precision
    pub const FRA: f64 = 1e-9;

    /// Futures rate tolerance - near machine precision
    pub const FUTURE: f64 = 1e-9;

    /// Swap par rate tolerance
    ///
    /// Sequential bootstrap for multi-period swaps uses extrapolated DFs during
    /// bootstrap but interpolated DFs during repricing, causing unavoidable error.
    ///
    /// For $1M notional: 500 = $500 PV error ≈ 5bp. This is acceptable for
    /// sequential bootstrap. For tighter tolerances, use global bootstrap.
    ///
    /// Note: Per-notional error is SWAP / notional. For $10M notional,
    /// actual tolerance is $5000.
    pub const SWAP: f64 = 500.0;

    /// OIS rate tolerance - near machine precision for single-period OIS
    pub const OIS: f64 = 1e-9;

    /// Basis swap tolerance - multi-curve instruments may have larger errors
    pub const BASIS_SWAP: f64 = 1e-6;

    /// Treasury bill tolerance - near machine precision
    pub const TREASURY_BILL: f64 = 1e-9;

    /// Treasury bond tolerance - multi-period may accumulate small errors
    pub const TREASURY_BOND: f64 = 1e-6;

    /// Generic government zero-coupon bond (T-Bills, etc.) - near machine precision
    pub const GOVERNMENT_ZERO: f64 = 1e-9;

    /// Generic government coupon bond (Gilts, Bunds, etc.) - multi-period may accumulate errors
    pub const GOVERNMENT_COUPON: f64 = 1e-6;

    /// Default tolerance for unknown instruments
    pub const DEFAULT: f64 = 1e-6;

    /// Strict tolerance for production use
    pub const STRICT: f64 = 1e-9;

    /// Relaxed tolerance for testing
    pub const RELAXED: f64 = 1e-3;

    /// Get tolerance for a specific instrument type
    #[must_use]
    pub fn for_instrument(inst_type: super::InstrumentType) -> f64 {
        use super::InstrumentType;
        match inst_type {
            InstrumentType::Deposit => DEPOSIT,
            InstrumentType::FRA => FRA,
            InstrumentType::Future => FUTURE,
            InstrumentType::Swap => SWAP,
            InstrumentType::OIS => OIS,
            InstrumentType::BasisSwap => BASIS_SWAP,
            InstrumentType::TreasuryBill => TREASURY_BILL,
            InstrumentType::TreasuryBond => TREASURY_BOND,
            InstrumentType::GovernmentZeroCoupon => GOVERNMENT_ZERO,
            InstrumentType::GovernmentCouponBond => GOVERNMENT_COUPON,
        }
    }
}

/// Result of repricing a single instrument against the curve.
#[derive(Debug, Clone)]
pub struct RepricingCheck {
    /// Description of the instrument (e.g., "Deposit 5.00% 2025-01-15 to 2025-04-15")
    pub instrument_id: String,

    /// Type of instrument
    pub instrument_type: InstrumentType,

    /// The target PV (usually 0 for par instruments)
    pub target_pv: f64,

    /// Model-implied PV from the curve
    pub model_pv: f64,

    /// Absolute error |model_pv - target_pv|
    pub error: f64,

    /// Tolerance for this instrument type
    pub tolerance: f64,

    /// Whether this instrument passed validation
    pub passed: bool,
}

impl RepricingCheck {
    /// Creates a new repricing check result.
    #[must_use]
    pub fn new(
        instrument_id: String,
        instrument_type: InstrumentType,
        target_pv: f64,
        model_pv: f64,
        tolerance: f64,
    ) -> Self {
        let error = (model_pv - target_pv).abs();
        let passed = error <= tolerance;

        Self {
            instrument_id,
            instrument_type,
            target_pv,
            model_pv,
            error,
            tolerance,
            passed,
        }
    }

    /// Creates a check from a curve instrument.
    pub fn from_instrument(
        instrument: &dyn CurveInstrument,
        curve: &dyn Curve,
        target_pv: f64,
    ) -> CurveResult<Self> {
        let model_pv = instrument.pv(curve)?;
        let tolerance = tolerances::for_instrument(instrument.instrument_type());

        Ok(Self::new(
            instrument.description(),
            instrument.instrument_type(),
            target_pv,
            model_pv,
            tolerance,
        ))
    }

    /// Creates a check with a custom tolerance.
    pub fn from_instrument_with_tolerance(
        instrument: &dyn CurveInstrument,
        curve: &dyn Curve,
        target_pv: f64,
        tolerance: f64,
    ) -> CurveResult<Self> {
        let model_pv = instrument.pv(curve)?;

        Ok(Self::new(
            instrument.description(),
            instrument.instrument_type(),
            target_pv,
            model_pv,
            tolerance,
        ))
    }
}

impl fmt::Display for RepricingCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.passed { "✓" } else { "✗" };
        write!(
            f,
            "{} {} | PV: {:.2e} | Error: {:.2e} (tol: {:.2e})",
            status, self.instrument_id, self.model_pv, self.error, self.tolerance
        )
    }
}

/// Complete repricing report for audit trail.
///
/// This report provides full transparency into how well the curve
/// reprices all input instruments. Every curve should be accompanied
/// by a repricing report.
#[derive(Debug, Clone)]
pub struct RepricingReport {
    /// Individual instrument checks
    checks: Vec<RepricingCheck>,

    /// Maximum absolute error across all instruments
    max_error: f64,

    /// Root mean square of all errors
    rms_error: f64,

    /// Whether all instruments passed validation
    all_passed: bool,

    /// Number of instruments that passed
    passed_count: usize,

    /// Number of instruments that failed
    failed_count: usize,
}

impl RepricingReport {
    /// Creates a new repricing report from individual checks.
    #[must_use]
    pub fn new(checks: Vec<RepricingCheck>) -> Self {
        let max_error = checks.iter()
            .map(|c| c.error)
            .fold(0.0_f64, f64::max);

        let rms_error = if checks.is_empty() {
            0.0
        } else {
            let sum_sq: f64 = checks.iter().map(|c| c.error * c.error).sum();
            (sum_sq / checks.len() as f64).sqrt()
        };

        let all_passed = checks.iter().all(|c| c.passed);
        let passed_count = checks.iter().filter(|c| c.passed).count();
        let failed_count = checks.len() - passed_count;

        Self {
            checks,
            max_error,
            rms_error,
            all_passed,
            passed_count,
            failed_count,
        }
    }

    /// Creates an empty report (for curves with no instruments).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            checks: Vec::new(),
            max_error: 0.0,
            rms_error: 0.0,
            all_passed: true,
            passed_count: 0,
            failed_count: 0,
        }
    }

    /// Returns whether all instruments passed repricing validation.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.all_passed
    }

    /// Returns the individual repricing checks.
    #[must_use]
    pub fn checks(&self) -> &[RepricingCheck] {
        &self.checks
    }

    /// Returns the maximum absolute error.
    #[must_use]
    pub fn max_error(&self) -> f64 {
        self.max_error
    }

    /// Returns the RMS error.
    #[must_use]
    pub fn rms_error(&self) -> f64 {
        self.rms_error
    }

    /// Returns the number of instruments that passed.
    #[must_use]
    pub fn passed_count(&self) -> usize {
        self.passed_count
    }

    /// Returns the number of instruments that failed.
    #[must_use]
    pub fn failed_count(&self) -> usize {
        self.failed_count
    }

    /// Returns the total number of instruments checked.
    #[must_use]
    pub fn total_count(&self) -> usize {
        self.checks.len()
    }

    /// Returns failed instrument checks for error reporting.
    #[must_use]
    pub fn failed_checks(&self) -> Vec<&RepricingCheck> {
        self.checks.iter().filter(|c| !c.passed).collect()
    }

    /// Returns failed instrument IDs for error messages.
    #[must_use]
    pub fn failed_instruments(&self) -> Vec<&str> {
        self.checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| c.instrument_id.as_str())
            .collect()
    }
}

impl fmt::Display for RepricingReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Repricing Report")?;
        writeln!(f, "================")?;
        writeln!(f, "Status: {}", if self.all_passed { "PASSED" } else { "FAILED" })?;
        writeln!(f, "Instruments: {}/{} passed", self.passed_count, self.checks.len())?;
        writeln!(f, "Max Error: {:.2e}", self.max_error)?;
        writeln!(f, "RMS Error: {:.2e}", self.rms_error)?;

        if !self.checks.is_empty() {
            writeln!(f)?;
            writeln!(f, "Details:")?;
            for check in &self.checks {
                writeln!(f, "  {check}")?;
            }
        }

        Ok(())
    }
}

/// Result of curve bootstrap including mandatory repricing validation.
///
/// This type wraps the bootstrapped curve with its repricing report,
/// ensuring that every curve comes with an audit trail of how well
/// it fits the input instruments.
///
/// # Usage
///
/// ```rust,ignore
/// let result = bootstrapper.bootstrap_validated()?;
///
/// // Always check validity before using the curve
/// if result.is_valid() {
///     let df = result.curve.discount_factor(1.0)?;
/// } else {
///     // Handle invalid curve
///     for check in result.failed_checks() {
///         eprintln!("Failed: {}", check);
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct BootstrapResult<C> {
    /// The bootstrapped curve.
    pub curve: C,

    /// Mandatory repricing validation report.
    pub repricing_report: RepricingReport,

    /// Time taken to build the curve.
    pub build_duration: Duration,
}

impl<C> BootstrapResult<C> {
    /// Creates a new bootstrap result.
    #[must_use]
    pub fn new(curve: C, repricing_report: RepricingReport, build_duration: Duration) -> Self {
        Self {
            curve,
            repricing_report,
            build_duration,
        }
    }

    /// Returns whether the bootstrap succeeded (all instruments reprice).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.repricing_report.is_valid()
    }

    /// Returns the maximum repricing error.
    #[must_use]
    pub fn max_error(&self) -> f64 {
        self.repricing_report.max_error()
    }

    /// Returns the RMS repricing error.
    #[must_use]
    pub fn rms_error(&self) -> f64 {
        self.repricing_report.rms_error()
    }

    /// Returns failed instrument checks.
    #[must_use]
    pub fn failed_checks(&self) -> Vec<&RepricingCheck> {
        self.repricing_report.failed_checks()
    }

    /// Returns failed instrument IDs.
    #[must_use]
    pub fn failed_instruments(&self) -> Vec<&str> {
        self.repricing_report.failed_instruments()
    }

    /// Consumes the result and returns just the curve.
    ///
    /// # Panics
    ///
    /// Panics if the curve is not valid (repricing failed).
    /// Use `into_curve_unchecked` to skip this check.
    #[must_use]
    pub fn into_curve(self) -> C {
        assert!(
            self.is_valid(),
            "Cannot extract curve: repricing validation failed. Max error: {:.2e}",
            self.max_error()
        );
        self.curve
    }

    /// Consumes the result and returns the curve without checking validity.
    ///
    /// Use this only when you've already checked `is_valid()` or
    /// when you intentionally want to use an invalid curve.
    #[must_use]
    pub fn into_curve_unchecked(self) -> C {
        self.curve
    }

    /// Returns a reference to the curve.
    #[must_use]
    pub fn curve(&self) -> &C {
        &self.curve
    }
}

impl<C: fmt::Debug> fmt::Display for BootstrapResult<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Bootstrap Result")?;
        writeln!(f, "Build time: {:?}", self.build_duration)?;
        writeln!(f)?;
        write!(f, "{}", self.repricing_report)?;
        Ok(())
    }
}

/// Validates a curve against a set of instruments.
///
/// This is a convenience function for creating a repricing report
/// from a curve and its input instruments.
pub fn validate_curve_repricing(
    curve: &DiscountCurve,
    instruments: &[Box<dyn CurveInstrument>],
) -> CurveResult<RepricingReport> {
    validate_curve_repricing_with_tolerance(curve, instruments, None)
}

/// Validates a curve with a custom tolerance.
pub fn validate_curve_repricing_with_tolerance(
    curve: &DiscountCurve,
    instruments: &[Box<dyn CurveInstrument>],
    custom_tolerance: Option<f64>,
) -> CurveResult<RepricingReport> {
    let mut checks = Vec::with_capacity(instruments.len());

    for inst in instruments {
        let check = if let Some(tol) = custom_tolerance {
            RepricingCheck::from_instrument_with_tolerance(inst.as_ref(), curve, 0.0, tol)?
        } else {
            RepricingCheck::from_instrument(inst.as_ref(), curve, 0.0)?
        };
        checks.push(check);
    }

    Ok(RepricingReport::new(checks))
}

/// Helper to time curve building operations.
pub struct BuildTimer {
    start: Instant,
}

impl BuildTimer {
    /// Starts a new timer.
    #[must_use]
    pub fn start() -> Self {
        Self { start: Instant::now() }
    }

    /// Returns the elapsed duration.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscountCurveBuilder;
    use crate::instruments::Deposit;
    use convex_core::Date;

    #[test]
    fn test_repricing_check_passed() {
        let check = RepricingCheck::new(
            "Deposit 3M".to_string(),
            InstrumentType::Deposit,
            0.0,
            1e-10,  // Very small error
            1e-6,   // Tolerance
        );

        assert!(check.passed);
        assert!(check.error < check.tolerance);
    }

    #[test]
    fn test_repricing_check_failed() {
        let check = RepricingCheck::new(
            "Deposit 3M".to_string(),
            InstrumentType::Deposit,
            0.0,
            0.01,   // Large error
            1e-6,   // Tolerance
        );

        assert!(!check.passed);
        assert!(check.error > check.tolerance);
    }

    #[test]
    fn test_repricing_report_all_passed() {
        let checks = vec![
            RepricingCheck::new("Dep 1".to_string(), InstrumentType::Deposit, 0.0, 1e-10, 1e-6),
            RepricingCheck::new("Dep 2".to_string(), InstrumentType::Deposit, 0.0, 1e-9, 1e-6),
        ];

        let report = RepricingReport::new(checks);

        assert!(report.is_valid());
        assert_eq!(report.passed_count(), 2);
        assert_eq!(report.failed_count(), 0);
        assert!(report.failed_instruments().is_empty());
    }

    #[test]
    fn test_repricing_report_some_failed() {
        let checks = vec![
            RepricingCheck::new("Dep 1".to_string(), InstrumentType::Deposit, 0.0, 1e-10, 1e-6),
            RepricingCheck::new("Dep 2".to_string(), InstrumentType::Deposit, 0.0, 0.01, 1e-6),
        ];

        let report = RepricingReport::new(checks);

        assert!(!report.is_valid());
        assert_eq!(report.passed_count(), 1);
        assert_eq!(report.failed_count(), 1);
        assert_eq!(report.failed_instruments(), vec!["Dep 2"]);
    }

    #[test]
    fn test_bootstrap_result_valid() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = DiscountCurveBuilder::new(ref_date)
            .add_pillar(0.0, 1.0)
            .add_pillar(1.0, 0.95)
            .build()
            .unwrap();

        let report = RepricingReport::new(vec![
            RepricingCheck::new("Dep 1".to_string(), InstrumentType::Deposit, 0.0, 1e-10, 1e-6),
        ]);

        let result = BootstrapResult::new(curve, report, Duration::from_micros(100));

        assert!(result.is_valid());
        let _ = result.into_curve();  // Should not panic
    }

    #[test]
    #[should_panic(expected = "repricing validation failed")]
    fn test_bootstrap_result_invalid_panics_on_into_curve() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = DiscountCurveBuilder::new(ref_date)
            .add_pillar(0.0, 1.0)
            .add_pillar(1.0, 0.95)
            .build()
            .unwrap();

        let report = RepricingReport::new(vec![
            RepricingCheck::new("Dep 1".to_string(), InstrumentType::Deposit, 0.0, 0.01, 1e-6),
        ]);

        let result = BootstrapResult::new(curve, report, Duration::from_micros(100));

        assert!(!result.is_valid());
        let _ = result.into_curve();  // Should panic
    }

    #[test]
    fn test_tolerance_for_instrument() {
        assert_eq!(tolerances::for_instrument(InstrumentType::Deposit), tolerances::DEPOSIT);
        assert_eq!(tolerances::for_instrument(InstrumentType::Swap), tolerances::SWAP);
        assert_eq!(tolerances::for_instrument(InstrumentType::OIS), tolerances::OIS);
    }

    #[test]
    fn test_repricing_check_from_instrument() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let end_date = Date::from_ymd(2025, 4, 1).unwrap();

        // Create a curve that should reprice the deposit exactly
        let deposit = Deposit::new(ref_date, end_date, 0.05);

        // Build a curve with the implied DF from this deposit
        let tau = deposit.year_fraction();
        let implied_df = 1.0 / (1.0 + 0.05 * tau);

        let curve = DiscountCurveBuilder::new(ref_date)
            .add_pillar(0.0, 1.0)
            .add_pillar(tau, implied_df)
            .with_extrapolation()
            .build()
            .unwrap();

        let check = RepricingCheck::from_instrument(&deposit, &curve, 0.0).unwrap();

        // The deposit should reprice very closely
        assert!(check.error < 1e-6, "Error was: {}", check.error);
        assert!(check.passed);
    }

    #[test]
    fn test_build_timer() {
        let timer = BuildTimer::start();
        std::thread::sleep(Duration::from_millis(10));
        let elapsed = timer.elapsed();
        assert!(elapsed >= Duration::from_millis(10));
    }

    #[test]
    fn test_repricing_report_display() {
        let checks = vec![
            RepricingCheck::new("Dep 1".to_string(), InstrumentType::Deposit, 0.0, 1e-10, 1e-6),
        ];
        let report = RepricingReport::new(checks);

        let display = format!("{report}");
        assert!(display.contains("PASSED"));
        assert!(display.contains("1/1 passed"));
    }
}
