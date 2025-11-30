//! Currency-specific market conventions.
//!
//! This module provides standard market conventions for different currencies,
//! including settlement days, day count conventions, swap conventions, and
//! instrument creation helpers.
//!
//! # Supported Currencies
//!
//! - USD: US Dollar (post-LIBOR, SOFR-based)
//! - EUR: Euro (EURIBOR/ESTR-based)
//! - GBP: British Pound (SONIA-based)
//! - JPY: Japanese Yen (TONAR-based)
//! - CHF: Swiss Franc (SARON-based)
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_curves::conventions::usd;
//! use convex_core::Date;
//!
//! let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
//! let deposit = usd::deposit("3M", 0.05, ref_date);
//! let ois = usd::ois_swap("2Y", 0.045, ref_date);
//! ```

use convex_core::calendars::{Calendar, SIFMACalendar, Target2Calendar, UKCalendar};
use convex_core::daycounts::DayCountConvention;
use convex_core::types::Frequency;
use convex_core::Date;

use crate::error::CurveResult;
use crate::instruments::{Deposit, OIS, Swap};

/// USD (US Dollar) market conventions.
///
/// Post-LIBOR conventions for the US market, using SOFR as the
/// primary overnight rate.
pub mod usd {
    use super::*;

    /// Spot settlement days (T+2).
    pub const SPOT_DAYS: u32 = 2;

    /// Deposit day count convention.
    pub const DEPOSIT_DAY_COUNT: DayCountConvention = DayCountConvention::Act360;

    /// Swap fixed leg frequency.
    pub const SWAP_FIXED_FREQ: Frequency = Frequency::Annual;

    /// Swap fixed leg day count.
    pub const SWAP_FIXED_DAY_COUNT: DayCountConvention = DayCountConvention::Act360;

    /// Swap float leg frequency (SOFR annual compounding).
    pub const SWAP_FLOAT_FREQ: Frequency = Frequency::Annual;

    /// OIS fixed leg frequency.
    pub const OIS_FIXED_FREQ: Frequency = Frequency::Annual;

    /// Returns the SIFMA calendar for USD fixed income.
    #[must_use]
    pub fn calendar() -> SIFMACalendar {
        SIFMACalendar::new()
    }

    /// Creates a USD deposit from tenor.
    ///
    /// # Arguments
    ///
    /// * `tenor` - Deposit tenor (e.g., "O/N", "1M", "3M")
    /// * `rate` - Deposit rate as a decimal
    /// * `reference_date` - Valuation date
    ///
    /// # Returns
    ///
    /// A `Deposit` instrument with USD conventions.
    pub fn deposit(tenor: &str, rate: f64, reference_date: Date) -> CurveResult<Deposit> {
        let cal = calendar();
        let spot = cal.add_business_days(reference_date, SPOT_DAYS as i32);
        Deposit::from_tenor(spot, tenor, rate)
    }

    /// Creates a USD OIS swap from tenor.
    ///
    /// Uses SOFR as the floating rate index.
    ///
    /// # Arguments
    ///
    /// * `tenor` - Swap tenor (e.g., "1Y", "2Y", "5Y")
    /// * `rate` - Fixed rate as a decimal
    /// * `reference_date` - Valuation date
    pub fn ois_swap(tenor: &str, rate: f64, reference_date: Date) -> CurveResult<OIS> {
        OIS::from_tenor(reference_date, tenor, rate)
    }

    /// Creates a USD interest rate swap from tenor.
    ///
    /// # Arguments
    ///
    /// * `tenor` - Swap tenor (e.g., "2Y", "5Y", "10Y")
    /// * `rate` - Fixed rate as a decimal
    /// * `reference_date` - Valuation date
    pub fn swap(tenor: &str, rate: f64, reference_date: Date) -> CurveResult<Swap> {
        Swap::from_tenor(reference_date, tenor, rate, SWAP_FIXED_FREQ)
    }

    /// Market convention summary for USD.
    #[must_use]
    pub fn summary() -> ConventionSummary {
        ConventionSummary {
            currency: "USD",
            spot_days: SPOT_DAYS,
            deposit_day_count: DEPOSIT_DAY_COUNT,
            swap_fixed_freq: SWAP_FIXED_FREQ,
            swap_fixed_day_count: SWAP_FIXED_DAY_COUNT,
            swap_float_freq: SWAP_FLOAT_FREQ,
            overnight_index: "SOFR",
            ibor_index: None,
            calendar_name: "SIFMA",
        }
    }
}

/// EUR (Euro) market conventions.
///
/// Uses EURIBOR for legacy products and ESTR for OIS.
pub mod eur {
    use super::*;

    /// Spot settlement days (T+2).
    pub const SPOT_DAYS: u32 = 2;

    /// Deposit day count convention.
    pub const DEPOSIT_DAY_COUNT: DayCountConvention = DayCountConvention::Act360;

    /// Swap fixed leg frequency.
    pub const SWAP_FIXED_FREQ: Frequency = Frequency::Annual;

    /// Swap fixed leg day count.
    pub const SWAP_FIXED_DAY_COUNT: DayCountConvention = DayCountConvention::Thirty360E;

    /// Swap float leg frequency (6M EURIBOR).
    pub const SWAP_FLOAT_FREQ: Frequency = Frequency::SemiAnnual;

    /// OIS fixed leg frequency.
    pub const OIS_FIXED_FREQ: Frequency = Frequency::Annual;

    /// Returns the TARGET2 calendar for EUR payments.
    #[must_use]
    pub fn calendar() -> Target2Calendar {
        Target2Calendar::new()
    }

    /// Creates a EUR deposit from tenor.
    pub fn deposit(tenor: &str, rate: f64, reference_date: Date) -> CurveResult<Deposit> {
        let cal = calendar();
        let spot = cal.add_business_days(reference_date, SPOT_DAYS as i32);
        Deposit::from_tenor(spot, tenor, rate)
    }

    /// Creates a EUR OIS swap from tenor.
    ///
    /// Uses ESTR as the floating rate index.
    pub fn ois_swap(tenor: &str, rate: f64, reference_date: Date) -> CurveResult<OIS> {
        OIS::from_tenor(reference_date, tenor, rate)
    }

    /// Creates a EUR interest rate swap from tenor.
    ///
    /// Uses 6M EURIBOR on the float leg.
    pub fn swap(tenor: &str, rate: f64, reference_date: Date) -> CurveResult<Swap> {
        Swap::from_tenor(reference_date, tenor, rate, SWAP_FIXED_FREQ)
    }

    /// Market convention summary for EUR.
    #[must_use]
    pub fn summary() -> ConventionSummary {
        ConventionSummary {
            currency: "EUR",
            spot_days: SPOT_DAYS,
            deposit_day_count: DEPOSIT_DAY_COUNT,
            swap_fixed_freq: SWAP_FIXED_FREQ,
            swap_fixed_day_count: SWAP_FIXED_DAY_COUNT,
            swap_float_freq: SWAP_FLOAT_FREQ,
            overnight_index: "ESTR",
            ibor_index: Some("EURIBOR 6M"),
            calendar_name: "TARGET2",
        }
    }
}

/// GBP (British Pound) market conventions.
///
/// Uses SONIA as the overnight rate. GBP has same-day settlement.
pub mod gbp {
    use super::*;

    /// Spot settlement days (T+0 for GBP!).
    pub const SPOT_DAYS: u32 = 0;

    /// Deposit day count convention.
    pub const DEPOSIT_DAY_COUNT: DayCountConvention = DayCountConvention::Act365Fixed;

    /// Swap fixed leg frequency.
    pub const SWAP_FIXED_FREQ: Frequency = Frequency::Annual;

    /// Swap fixed leg day count.
    pub const SWAP_FIXED_DAY_COUNT: DayCountConvention = DayCountConvention::Act365Fixed;

    /// Swap float leg frequency.
    pub const SWAP_FLOAT_FREQ: Frequency = Frequency::Annual;

    /// OIS fixed leg frequency.
    pub const OIS_FIXED_FREQ: Frequency = Frequency::Annual;

    /// Returns the UK calendar.
    #[must_use]
    pub fn calendar() -> UKCalendar {
        UKCalendar::new()
    }

    /// Creates a GBP deposit from tenor.
    pub fn deposit(tenor: &str, rate: f64, reference_date: Date) -> CurveResult<Deposit> {
        let cal = calendar();
        let spot = if SPOT_DAYS == 0 {
            reference_date
        } else {
            cal.add_business_days(reference_date, SPOT_DAYS as i32)
        };
        Deposit::from_tenor(spot, tenor, rate)
    }

    /// Creates a GBP OIS swap from tenor.
    ///
    /// Uses SONIA as the floating rate index.
    pub fn ois_swap(tenor: &str, rate: f64, reference_date: Date) -> CurveResult<OIS> {
        OIS::from_tenor(reference_date, tenor, rate)
    }

    /// Creates a GBP interest rate swap from tenor.
    pub fn swap(tenor: &str, rate: f64, reference_date: Date) -> CurveResult<Swap> {
        Swap::from_tenor(reference_date, tenor, rate, SWAP_FIXED_FREQ)
    }

    /// Market convention summary for GBP.
    #[must_use]
    pub fn summary() -> ConventionSummary {
        ConventionSummary {
            currency: "GBP",
            spot_days: SPOT_DAYS,
            deposit_day_count: DEPOSIT_DAY_COUNT,
            swap_fixed_freq: SWAP_FIXED_FREQ,
            swap_fixed_day_count: SWAP_FIXED_DAY_COUNT,
            swap_float_freq: SWAP_FLOAT_FREQ,
            overnight_index: "SONIA",
            ibor_index: None,
            calendar_name: "UK",
        }
    }
}

/// JPY (Japanese Yen) market conventions.
pub mod jpy {
    use super::*;
    use convex_core::calendars::JapanCalendar;

    /// Spot settlement days (T+2).
    pub const SPOT_DAYS: u32 = 2;

    /// Deposit day count convention.
    pub const DEPOSIT_DAY_COUNT: DayCountConvention = DayCountConvention::Act365Fixed;

    /// Swap fixed leg frequency.
    pub const SWAP_FIXED_FREQ: Frequency = Frequency::SemiAnnual;

    /// Swap fixed leg day count.
    pub const SWAP_FIXED_DAY_COUNT: DayCountConvention = DayCountConvention::Act365Fixed;

    /// Swap float leg frequency (6M TIBOR/TONA).
    pub const SWAP_FLOAT_FREQ: Frequency = Frequency::SemiAnnual;

    /// Returns the Japan calendar.
    #[must_use]
    pub fn calendar() -> JapanCalendar {
        JapanCalendar::new()
    }

    /// Creates a JPY deposit from tenor.
    pub fn deposit(tenor: &str, rate: f64, reference_date: Date) -> CurveResult<Deposit> {
        let cal = calendar();
        let spot = cal.add_business_days(reference_date, SPOT_DAYS as i32);
        Deposit::from_tenor(spot, tenor, rate)
    }

    /// Creates a JPY OIS swap from tenor.
    pub fn ois_swap(tenor: &str, rate: f64, reference_date: Date) -> CurveResult<OIS> {
        OIS::from_tenor(reference_date, tenor, rate)
    }

    /// Market convention summary for JPY.
    #[must_use]
    pub fn summary() -> ConventionSummary {
        ConventionSummary {
            currency: "JPY",
            spot_days: SPOT_DAYS,
            deposit_day_count: DEPOSIT_DAY_COUNT,
            swap_fixed_freq: SWAP_FIXED_FREQ,
            swap_fixed_day_count: SWAP_FIXED_DAY_COUNT,
            swap_float_freq: SWAP_FLOAT_FREQ,
            overnight_index: "TONAR",
            ibor_index: Some("TIBOR 6M"),
            calendar_name: "Japan",
        }
    }
}

/// CHF (Swiss Franc) market conventions.
pub mod chf {
    use super::*;

    /// Spot settlement days (T+2).
    pub const SPOT_DAYS: u32 = 2;

    /// Deposit day count convention.
    pub const DEPOSIT_DAY_COUNT: DayCountConvention = DayCountConvention::Act360;

    /// Swap fixed leg frequency.
    pub const SWAP_FIXED_FREQ: Frequency = Frequency::Annual;

    /// Swap fixed leg day count.
    pub const SWAP_FIXED_DAY_COUNT: DayCountConvention = DayCountConvention::Thirty360E;

    /// Swap float leg frequency (6M SARON).
    pub const SWAP_FLOAT_FREQ: Frequency = Frequency::SemiAnnual;

    /// Market convention summary for CHF.
    #[must_use]
    pub fn summary() -> ConventionSummary {
        ConventionSummary {
            currency: "CHF",
            spot_days: SPOT_DAYS,
            deposit_day_count: DEPOSIT_DAY_COUNT,
            swap_fixed_freq: SWAP_FIXED_FREQ,
            swap_fixed_day_count: SWAP_FIXED_DAY_COUNT,
            swap_float_freq: SWAP_FLOAT_FREQ,
            overnight_index: "SARON",
            ibor_index: None,
            calendar_name: "Zurich",
        }
    }
}

/// Summary of market conventions for a currency.
#[derive(Debug, Clone, Copy)]
pub struct ConventionSummary {
    /// ISO currency code.
    pub currency: &'static str,
    /// Spot settlement days.
    pub spot_days: u32,
    /// Deposit day count convention.
    pub deposit_day_count: DayCountConvention,
    /// Swap fixed leg frequency.
    pub swap_fixed_freq: Frequency,
    /// Swap fixed leg day count.
    pub swap_fixed_day_count: DayCountConvention,
    /// Swap float leg frequency.
    pub swap_float_freq: Frequency,
    /// Overnight index name.
    pub overnight_index: &'static str,
    /// IBOR index name (if applicable).
    pub ibor_index: Option<&'static str>,
    /// Calendar name.
    pub calendar_name: &'static str,
}

impl std::fmt::Display for ConventionSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} Market Conventions:", self.currency)?;
        writeln!(f, "  Spot days: T+{}", self.spot_days)?;
        writeln!(f, "  Deposit DC: {}", self.deposit_day_count)?;
        writeln!(f, "  Swap Fixed: {} / {}", self.swap_fixed_freq, self.swap_fixed_day_count)?;
        writeln!(f, "  Swap Float: {}", self.swap_float_freq)?;
        writeln!(f, "  O/N Index: {}", self.overnight_index)?;
        if let Some(ibor) = self.ibor_index {
            writeln!(f, "  IBOR Index: {}", ibor)?;
        }
        writeln!(f, "  Calendar: {}", self.calendar_name)?;
        Ok(())
    }
}

/// Returns convention summary for a currency code.
#[must_use]
pub fn get_conventions(currency: &str) -> Option<ConventionSummary> {
    match currency.to_uppercase().as_str() {
        "USD" => Some(usd::summary()),
        "EUR" => Some(eur::summary()),
        "GBP" => Some(gbp::summary()),
        "JPY" => Some(jpy::summary()),
        "CHF" => Some(chf::summary()),
        _ => None,
    }
}

/// Lists all supported currencies.
#[must_use]
pub fn supported_currencies() -> &'static [&'static str] {
    &["USD", "EUR", "GBP", "JPY", "CHF"]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruments::CurveInstrument;

    #[test]
    fn test_usd_conventions() {
        let summary = usd::summary();
        assert_eq!(summary.currency, "USD");
        assert_eq!(summary.spot_days, 2);
        assert_eq!(summary.overnight_index, "SOFR");
    }

    #[test]
    fn test_eur_conventions() {
        let summary = eur::summary();
        assert_eq!(summary.currency, "EUR");
        assert_eq!(summary.spot_days, 2);
        assert_eq!(summary.overnight_index, "ESTR");
        assert_eq!(summary.ibor_index, Some("EURIBOR 6M"));
    }

    #[test]
    fn test_gbp_conventions() {
        let summary = gbp::summary();
        assert_eq!(summary.currency, "GBP");
        assert_eq!(summary.spot_days, 0); // Same-day settlement
        assert_eq!(summary.overnight_index, "SONIA");
    }

    #[test]
    fn test_get_conventions() {
        assert!(get_conventions("USD").is_some());
        assert!(get_conventions("usd").is_some()); // Case insensitive
        assert!(get_conventions("XXX").is_none());
    }

    #[test]
    fn test_supported_currencies() {
        let currencies = supported_currencies();
        assert!(currencies.contains(&"USD"));
        assert!(currencies.contains(&"EUR"));
        assert!(currencies.contains(&"GBP"));
    }

    #[test]
    fn test_usd_deposit() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let deposit = usd::deposit("3M", 0.05, ref_date).unwrap();
        assert!(deposit.maturity() > ref_date);
    }

    #[test]
    fn test_eur_deposit() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let deposit = eur::deposit("6M", 0.035, ref_date).unwrap();
        assert!(deposit.maturity() > ref_date);
    }

    #[test]
    fn test_gbp_deposit() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let deposit = gbp::deposit("1M", 0.045, ref_date).unwrap();
        assert!(deposit.maturity() > ref_date);
    }

    #[test]
    fn test_usd_ois() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let ois = usd::ois_swap("2Y", 0.045, ref_date).unwrap();
        assert!(ois.maturity() > ref_date);
    }

    #[test]
    fn test_convention_summary_display() {
        let summary = usd::summary();
        let display = format!("{}", summary);
        assert!(display.contains("USD"));
        assert!(display.contains("SOFR"));
    }
}
